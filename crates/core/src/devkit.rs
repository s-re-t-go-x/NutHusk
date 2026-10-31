//! 插件开发者工具链：脚手架生成（`new`）、协议与目录校验（`check`）、打包 `.nut`（`pack`）、无头动作调用（`run`）。

use crate::manifest::{is_plugin_local_command, is_valid_id, CommandSpec, Manifest};
use crate::package;
use crate::signature::SIGNATURE_FILE;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
use zip::write::SimpleFileOptions;

const MAX_ICON_BYTES: u64 = 512 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub manifest: Manifest,
    pub file_count: usize,
    pub total_bytes: u64,
    pub backend_tested: bool,
    pub backend_command: Option<String>,
    pub backend_response: Option<Value>,
}

/// 生成插件脚手架目录。
/// `lang` 支持：`"js"` / `"node"`, `"python"` / `"py"`, `"rust"` / `"rs"`, `"ui-only"` / `"ui"`.
pub fn scaffold_plugin(
    parent_dir: &Path,
    id: &str,
    name: Option<&str>,
    lang: &str,
) -> Result<PathBuf, String> {
    if !is_valid_id(id) {
        return Err(format!(
            "插件 id 不合法: {id}（应形如 com.example.my-tool）"
        ));
    }
    let short_name = id.rsplit('.').next().unwrap_or(id);
    let display_name = name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(short_name);
    let norm_lang = match lang.trim().to_ascii_lowercase().as_str() {
        "js" | "node" | "javascript" => "js",
        "py" | "python" | "python3" => "python",
        "rs" | "rust" => "rust",
        "ui" | "ui-only" | "html" | "frontend" => "ui-only",
        other => {
            return Err(format!(
                "不支持的模板语言: {other}（可选: js, python, rust, ui-only）"
            ))
        }
    };

    let dest = if parent_dir.file_name().and_then(|s| s.to_str()) == Some(short_name)
        || parent_dir.file_name().and_then(|s| s.to_str()) == Some(id)
    {
        parent_dir.to_path_buf()
    } else {
        parent_dir.join(short_name)
    };

    if dest.exists()
        && fs::read_dir(&dest)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
    {
        return Err(format!("目标目录已存在且非空: {}", dest.display()));
    }
    fs::create_dir_all(&dest).map_err(|e| format!("无法创建插件目录: {e}"))?;

    let backend_json = match norm_lang {
        "js" => Some(json!({ "command": "node", "args": ["main.js"] })),
        "python" => Some(json!({
            "command": "python3",
            "args": ["main.py"],
            "windows": { "command": "python", "args": ["main.py"] }
        })),
        "rust" => Some(json!({
            "command": "./backend",
            "windows": { "command": "./backend.exe" }
        })),
        _ => None,
    };

    let mut manifest_obj = json!({
        "id": id,
        "name": display_name,
        "version": "0.1.0",
        "apiVersion": "^1.1.0",
        "description": format!("{display_name}（由 NutHusk CLI 生成的 {norm_lang} 插件模板）"),
        "icon": "icon.svg",
        "category": "开发",
        "keywords": [short_name],
        "ui": "ui.html",
        "permissions": [],
        "match": {
            "minLength": 1,
            "maxLength": 10000,
            "label": format!("用 {display_name} 处理")
        },
        "actions": [
            {
                "id": "transform",
                "name": format!("{display_name} 转换"),
                "method": "transform",
                "description": "对输入文本执行默认转换"
            }
        ]
    });
    if let Some(b) = backend_json {
        manifest_obj
            .as_object_mut()
            .unwrap()
            .insert("backend".into(), b);
    }

    fs::write(
        dest.join("plugin.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest_obj).map_err(|e| e.to_string())?
        ),
    )
    .map_err(|e| format!("写入 plugin.json 失败: {e}"))?;

    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" fill="none">
  <rect width="64" height="64" rx="14" fill="#3B82F6"/>
  <path d="M20 24L12 32L20 40M44 24L52 32L44 40M36 18L28 46" stroke="white" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
"##;
    fs::write(dest.join("icon.svg"), svg).map_err(|e| format!("写入 icon.svg 失败: {e}"))?;

    let ui_html = template_ui_html(display_name, norm_lang != "ui-only");
    fs::write(dest.join("ui.html"), ui_html).map_err(|e| format!("写入 ui.html 失败: {e}"))?;

    match norm_lang {
        "js" => {
            let js = r##"// NutHusk 插件后端（Node.js）：从 stdin 逐行读取 JSON-RPC 2.0 请求，向 stdout 逐行输出响应。
const readline = require('node:readline');

const methods = {
  transform({ text = '' }) {
    return String(text).trim().toUpperCase();
  },
};

const send = (obj) => process.stdout.write(JSON.stringify(obj) + '\n');

readline.createInterface({ input: process.stdin }).on('line', (line) => {
  if (!line.trim()) return;
  let msg;
  try { msg = JSON.parse(line); } catch { return; }
  try {
    if (!Object.hasOwn(methods, msg.method)) throw new Error(`未知方法: ${msg.method}`);
    send({ jsonrpc: '2.0', id: msg.id, result: methods[msg.method](msg.params || {}) });
  } catch (e) {
    send({ jsonrpc: '2.0', id: msg.id, error: { code: -32000, message: e.message } });
  }
});
"##;
            fs::write(dest.join("main.js"), js).map_err(|e| format!("写入 main.js 失败: {e}"))?;
        }
        "python" => {
            let py = r##"# NutHusk 插件后端（Python）：从 stdin 逐行读取 JSON-RPC 2.0 请求，向 stdout 逐行输出响应。
# 注意：每次 print 必须带上 flush=True，否则输出停留在缓冲区会导致宿主等待超时。
import sys
import json

def handle_transform(params):
    text = str((params or {}).get("text", ""))
    return text.strip().upper()

METHODS = {
    "transform": handle_transform,
}

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
    except Exception:
        continue
    req_id = req.get("id")
    method = req.get("method")
    params = req.get("params") or {}
    try:
        if method not in METHODS:
            raise ValueError(f"未知方法: {method}")
        result = METHODS[method](params)
        print(json.dumps({"jsonrpc": "2.0", "id": req_id, "result": result}), flush=True)
    except Exception as e:
        print(json.dumps({"jsonrpc": "2.0", "id": req_id, "error": {"code": -32000, "message": str(e)}}), flush=True)
"##;
            fs::write(dest.join("main.py"), py).map_err(|e| format!("写入 main.py 失败: {e}"))?;
        }
        "rust" => {
            let rs = r##"// NutHusk 插件后端（Rust 最小骨架）：编译后将二进制放到插件根目录的 backend（Windows 为 backend.exe）。
// 编译命令：rustc main.rs -O -o backend.exe（或使用 Cargo 项目编译）
use std::io::{self, BufRead, Write};

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines().flatten() {
        if line.trim().is_empty() {
            continue;
        }
        // 最小示例：提取 id 并返回结果（正式项目推荐使用 serde_json）
        let _ = writeln!(stdout, "{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"OK from Rust backend\"}}");
        let _ = stdout.flush();
    }
}
"##;
            fs::write(dest.join("main.rs"), rs).map_err(|e| format!("写入 main.rs 失败: {e}"))?;
        }
        _ => {}
    }

    Ok(dest)
}

fn template_ui_html(title: &str, has_backend: bool) -> String {
    let run_logic = if has_backend {
        r##"nuthusk.call('transform', { text: src }).then(function (res) {
        $('output').value = typeof res === 'string' ? res : JSON.stringify(res, null, 2);
        setStatus('ok', '处理完成');
      }).catch(function (err) {
        setStatus('err', err.message || String(err));
      });"##
    } else {
        r##"var res = src.trim().toUpperCase();
      $('output').value = res;
      setStatus('ok', '处理完成');"##
    };

    format!(
        r##"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <link rel="stylesheet" href="/_sdk/ui.css" />
</head>
<body>
  <div class="nh-page">
    <div class="nh-toolbar">
      <button id="go" class="primary">执行转换</button>
      <span class="nh-spacer"></span>
      <button id="send" class="small">投递到…</button>
    </div>
    <div class="nh-panes">
      <section class="nh-pane">
        <header class="nh-pane-head"><span>输入</span></header>
        <textarea id="input" class="nh-pane-body mono" spellcheck="false" placeholder="在此输入文本…"></textarea>
      </section>
      <section class="nh-pane">
        <header class="nh-pane-head"><span>输出</span></header>
        <textarea id="output" class="nh-pane-body mono" spellcheck="false" readonly></textarea>
      </section>
    </div>
    <div id="status" class="nh-status">{title} 就绪</div>
  </div>
  <script src="/_sdk/ui.js"></script>
  <script>
    var $ = function (id) {{ return document.getElementById(id); }};
    function setStatus(kind, text) {{
      $('status').className = 'nh-status ' + kind;
      $('status').textContent = text;
    }}
    function execute() {{
      var src = $('input').value;
      if (!src.trim()) {{ setStatus('', '请先输入内容'); return; }}
      {run_logic}
    }}
    $('go').onclick = execute;
    $('send').onclick = function () {{
      var out = $('output').value || $('input').value;
      if (out.trim() && window.nuthusk && nuthusk.sendTo) nuthusk.sendTo(out);
    }};
    if (window.nuthusk) {{
      nuthusk.onInput(function (text) {{
        $('input').value = text;
        execute();
      }});
      nuthusk.onAction(function (method, params) {{
        var text = String((params && params.text) || '');
        return text.trim().toUpperCase();
      }});
    }}
  </script>
</body>
</html>
"##
    )
}

fn collect_plugin_files(dir: &Path) -> Result<Vec<(PathBuf, PathBuf, u64)>, String> {
    let mut out = Vec::new();
    fn walk(root: &Path, cur: &Path, out: &mut Vec<(PathBuf, PathBuf, u64)>) -> Result<(), String> {
        let entries =
            fs::read_dir(cur).map_err(|e| format!("读取目录失败 {}: {e}", cur.display()))?;
        let mut items: Vec<_> = entries.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for item in items {
            let name = item.file_name().to_string_lossy().into_owned();
            if name.starts_with('.')
                || matches!(
                    name.as_str(),
                    "node_modules" | "target" | "__pycache__" | "__MACOSX"
                )
                || name.ends_with(".nut")
            {
                continue;
            }
            let path = item.path();
            let ft = item
                .file_type()
                .map_err(|e| format!("读取文件类型失败 {}: {e}", path.display()))?;
            if ft.is_symlink() {
                return Err(format!("插件目录内不允许符号链接: {}", path.display()));
            }
            if ft.is_dir() {
                walk(root, &path, out)?;
            } else if ft.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_path_buf();
                let size = item.metadata().map(|m| m.len()).unwrap_or(0);
                out.push((rel, path, size));
            }
        }
        Ok(())
    }
    walk(dir, dir, &mut out)?;
    Ok(out)
}

/// 静态校验插件目录，并在 `test_backend = true` 且存在后端配置时启动一次后端进程验证 stdio JSON-RPC 协议。
pub fn check_plugin_dir(dir: &Path, test_backend: bool) -> Result<CheckReport, String> {
    let manifest_path = dir.join("plugin.json");
    if !manifest_path.is_file() {
        return Err(format!("找不到 plugin.json: {}", manifest_path.display()));
    }
    let raw =
        fs::read_to_string(&manifest_path).map_err(|e| format!("读取 plugin.json 失败: {e}"))?;
    let manifest: Manifest =
        serde_json::from_str(&raw).map_err(|e| format!("plugin.json 解析失败: {e}"))?;
    manifest.validate()?;
    manifest.check_compatible()?;

    if let Some(ui) = &manifest.ui {
        if !dir.join(ui).is_file() {
            return Err(format!("manifest 声明的界面文件不存在: {ui}"));
        }
    }
    if let Some(icon) = &manifest.icon {
        let icon_path = dir.join(icon);
        if !icon_path.is_file() {
            return Err(format!("manifest 声明的图标文件不存在: {icon}"));
        }
        let size = fs::metadata(&icon_path).map(|m| m.len()).unwrap_or(0);
        if size > MAX_ICON_BYTES {
            return Err("图标文件过大（上限 512 KB）".into());
        }
    }
    if let Some(b) = &manifest.backend {
        for v in b.variants() {
            if is_plugin_local_command(&v.command) && !dir.join(&v.command[2..]).is_file() {
                return Err(format!("manifest 声明的本地后端文件不存在: {}", v.command));
            }
        }
    }

    let files = collect_plugin_files(dir)?;
    let total_bytes: u64 = files.iter().map(|(_, _, sz)| *sz).sum();

    let spec = manifest.backend.as_ref().and_then(|b| b.for_current_os());
    let backend_command = spec.as_ref().map(|c| {
        std::iter::once(c.command.as_str())
            .chain(c.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    });

    let mut backend_tested = false;
    let mut backend_response = None;
    if test_backend {
        if let Some(cmd_spec) = &spec {
            let (method, params) = if let Some(first_act) = manifest.actions.first() {
                let mut p = first_act
                    .default_params
                    .clone()
                    .and_then(|v| v.as_object().cloned())
                    .unwrap_or_default();
                p.entry("text".to_string())
                    .or_insert_with(|| Value::String("{\"hello\":\"nuthusk\"}".into()));
                (first_act.effective_method().to_string(), Value::Object(p))
            } else {
                ("__nuthusk_ping__".into(), json!({ "text": "ping" }))
            };
            let resp =
                call_backend_once(dir, cmd_spec, &method, params, manifest.actions.is_empty())?;
            backend_tested = true;
            backend_response = Some(resp);
        }
    }

    Ok(CheckReport {
        manifest,
        file_count: files.len(),
        total_bytes,
        backend_tested,
        backend_command,
        backend_response,
    })
}

fn call_backend_once(
    dir: &Path,
    spec: &CommandSpec,
    method: &str,
    params: Value,
    allow_rpc_error: bool,
) -> Result<Value, String> {
    let prog = if is_plugin_local_command(&spec.command) {
        dir.join(&spec.command[2..])
    } else {
        PathBuf::from(&spec.command)
    };
    let mut cmd = Command::new(&prog);
    cmd.args(&spec.args)
        .current_dir(dir)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("无法启动后端进程 {}: {e}", spec.command))?;
    let mut stdin = child.stdin.take().ok_or("无法获取后端 stdin")?;
    let stdout = child.stdout.take().ok_or("无法获取后端 stdout")?;

    let req_line = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        })
    );
    const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
    if req_line.len() as u64 > MAX_RESPONSE_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err("后端请求超过 16 MB 上限".into());
    }
    if let Some(mut stderr) = child.stderr.take() {
        std::thread::spawn(move || { let _ = std::io::copy(&mut stderr, &mut std::io::sink()); });
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let res = (|| -> std::io::Result<String> {
            // Writing is also covered by the parent's deadline: a backend may never read stdin.
            stdin.write_all(req_line.as_bytes())?;
            stdin.flush()?;
            let mut reader = BufReader::new(stdout).take(MAX_RESPONSE_BYTES + 1);
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line.len() as u64 > MAX_RESPONSE_BYTES {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "后端响应超过 16 MB 上限"));
            }
            Ok(line)
        })();
        let _ = tx.send(res);
    });

    let line_res = rx.recv_timeout(PROBE_TIMEOUT);
    let _ = child.kill();
    let _ = child.wait();

    let line = match line_res {
        Ok(Ok(l)) if !l.trim().is_empty() => l,
        Ok(Ok(_)) => return Err("后端进程未输出任何响应即退出".into()),
        Ok(Err(e)) => return Err(format!("读取后端 stdout 失败: {e}")),
        Err(_) => {
            return Err(
                "等待后端 JSON-RPC 响应超时（5s），请检查是否按行输出 JSON 并及时 flush stdout"
                    .into(),
            )
        }
    };

    let msg: Value = serde_json::from_str(line.trim()).map_err(|e| {
        format!("后端 stdout 输出不是合法的 JSON: {e}（为避免泄露处理内容，未记录原文）")
    })?;
    if msg.get("id").and_then(Value::as_u64) != Some(1) {
        return Err("后端响应缺少匹配的 id=1".to_string());
    }
    if let Some(err) = msg.get("error") {
        if allow_rpc_error {
            return Ok(msg);
        }
        let err_msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        return Err(format!("后端执行 {method} 返回错误: {err_msg}"));
    }
    Ok(msg.get("result").cloned().unwrap_or(Value::Null))
}

fn reject_private_key(bytes: &[u8], rel: &Path) -> Result<(), String> {
    let marker = crate::signature::PRIVATE_KEY_PREFIX.as_bytes();
    if bytes.windows(marker.len()).any(|part| part == marker) {
        return Err(format!("检测到 NutHusk 私钥，已拒绝打包：{}（请将私钥移出插件目录）", rel.display()));
    }
    Ok(())
}

fn check_pack_secrets(rel: &Path, full: &Path) -> Result<(), String> {
    let mut file = File::open(full).map_err(|e| format!("读取 {} 失败: {e}", rel.display()))?;
    let mut chunk = [0u8; 8192];
    let mut tail = Vec::new();
    loop {
        let n = file.read(&mut chunk).map_err(|e| format!("读取 {} 失败: {e}", rel.display()))?;
        if n == 0 { return Ok(()); }
        tail.extend_from_slice(&chunk[..n]);
        reject_private_key(&tail, rel)?;
        let keep = crate::signature::PRIVATE_KEY_PREFIX.len() - 1;
        if tail.len() > keep { tail.drain(..tail.len() - keep); }
    }
}

/// 将插件目录打包为 `.nut` 文件，并在打包后自动通过 `package::inspect` 校验完整性。
pub fn pack_plugin_dir(dir: &Path, out_path: Option<&Path>) -> Result<PathBuf, String> {
    let report = check_plugin_dir(dir, false)?;
    let files = collect_plugin_files(dir)?;
    // Complete the secret preflight before creating or overwriting the output.
    for (rel, full, _) in &files {
        if rel != Path::new(SIGNATURE_FILE) { check_pack_secrets(rel, full)?; }
    }
    let dst = match out_path {
        Some(p) => p.to_path_buf(),
        None => {
            let short = report
                .manifest
                .id
                .rsplit('.')
                .next()
                .unwrap_or(&report.manifest.id);
            PathBuf::from(format!("{short}-{}.nut", report.manifest.version))
        }
    };
    if let Some(parent) = dst.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("无法创建输出目录: {e}"))?;
        }
    }

    let file =
        File::create(&dst).map_err(|e| format!("无法创建插件包文件 {}: {e}", dst.display()))?;
    let mut zw = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for (rel, full, _) in files {
        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if rel_str == SIGNATURE_FILE {
            continue;
        }
        let mut f =
            File::open(&full).map_err(|e| format!("读取文件失败 {}: {e}", full.display()))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| format!("读取文件失败 {}: {e}", full.display()))?;
        // Recheck the exact bytes being archived in case a file changed after preflight.
        reject_private_key(&buf, &rel)?;
        zw.start_file(&rel_str, options)
            .map_err(|e| format!("写入压缩包条目失败 {rel_str}: {e}"))?;
        zw.write_all(&buf)
            .map_err(|e| format!("写入压缩包失败 {rel_str}: {e}"))?;
    }
    zw.finish().map_err(|e| format!("完成压缩包失败: {e}"))?;

    // 用宿主安装器相同的逻辑做最终验证
    package::inspect(&dst, &[])?;
    Ok(dst)
}

/// 无头运行插件动作：根据 `action_or_method` 查找 `manifest.actions`（或直接作为后端 method 名），
/// 传入 `{ text, ...defaultParams, ...extraParams }` 并返回插件后端的执行结果。
pub fn run_headless_action(
    plugin_dir: &Path,
    action_or_method: &str,
    input_text: &str,
    extra_params: Option<Value>,
) -> Result<Value, String> {
    let report = check_plugin_dir(plugin_dir, false)?;
    let spec = report
        .manifest
        .backend
        .as_ref()
        .and_then(|b| b.for_current_os())
        .ok_or("该插件没有适用于当前系统的后端进程，无法在命令行无头运行")?;

    let matched_action = report
        .manifest
        .actions
        .iter()
        .find(|a| a.id == action_or_method || a.effective_method() == action_or_method);

    let method = matched_action
        .map(|a| a.effective_method().to_string())
        .unwrap_or_else(|| action_or_method.to_string());

    let mut params_map = serde_json::Map::new();
    if let Some(act) = matched_action {
        if let Some(Value::Object(def)) = &act.default_params {
            for (k, v) in def {
                params_map.insert(k.clone(), v.clone());
            }
        }
    }
    if let Some(Value::Object(ext)) = extra_params {
        for (k, v) in ext {
            params_map.insert(k, v);
        }
    }
    params_map.insert("text".into(), Value::String(input_text.to_string()));

    call_backend_once(plugin_dir, &spec, &method, Value::Object(params_map), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_rejects_private_material_before_creating_output() {
        let tmp = std::env::temp_dir().join(format!("nuthusk-pack-secrets-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let dir = scaffold_plugin(&tmp, "com.example.secret-test", None, "ui-only").unwrap();
        let output = tmp.join("test.nut");
        // A nested, renamed binary file; marker crosses the streaming scan boundary.
        fs::create_dir_all(dir.join("assets")).unwrap();
        let file = dir.join("assets/data.bin");
        let mut bytes = vec![0xff; 8180];
        bytes.extend_from_slice(b"nuthusk-ed25519-private:SYNTHETIC_TEST_ONLY");
        fs::write(&file, bytes).unwrap();
        let err = pack_plugin_dir(&dir, Some(&output)).unwrap_err();
        assert!(err.contains("data.bin") && err.contains("私钥"));
        assert!(!err.contains("SYNTHETIC_TEST_ONLY"));
        assert!(!output.exists());
        fs::write(&output, b"existing package").unwrap();
        assert!(pack_plugin_dir(&dir, Some(&output)).is_err());
        assert_eq!(fs::read(&output).unwrap(), b"existing package");
        fs::remove_file(file).unwrap();
        // The suffix alone is not a secret: ordinary data and public keys remain usable.
        fs::write(dir.join("ordinary.key"), b"non-secret application data").unwrap();
        fs::write(dir.join("public.pub"), b"nuthusk-ed25519-public:test").unwrap();
        pack_plugin_dir(&dir, Some(&output)).unwrap();
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scaffold_check_and_pack_ui_only_and_js() {
        let tmp = std::env::temp_dir().join(format!("nuthusk-devkit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // 1. UI-only 插件脚手架 + check + pack
        let ui_dir =
            scaffold_plugin(&tmp, "com.example.demo-ui", Some("演示纯前端"), "ui-only").unwrap();
        let rep = check_plugin_dir(&ui_dir, true).unwrap();
        assert_eq!(rep.manifest.id, "com.example.demo-ui");
        assert!(!rep.backend_tested);
        assert_eq!(rep.manifest.actions.len(), 1);

        let nut_path = tmp.join("demo-ui.nut");
        let packed = pack_plugin_dir(&ui_dir, Some(&nut_path)).unwrap();
        assert!(packed.is_file());

        // 2. JS 后端插件脚手架 + check(含后端探测) + run
        let js_dir = scaffold_plugin(&tmp, "com.example.demo-js", Some("演示 JS"), "js").unwrap();
        if package::command_on_path("node") {
            let rep_js = check_plugin_dir(&js_dir, true).unwrap();
            assert!(rep_js.backend_tested);
            let out = run_headless_action(&js_dir, "transform", "  hello world  ", None).unwrap();
            assert_eq!(out, Value::String("HELLO WORLD".into()));
        }

        let _ = fs::remove_dir_all(&tmp);
    }
}
