//! 插件包（.nut，本质是 zip）的检查与安装。
//!
//! 包布局：plugin.json 位于压缩包根目录（或唯一的顶层文件夹内）。
//! 安全措施：拒绝路径穿越与符号链接；限制条目数、单文件和总解压大小（按实际读出的字节计，不信任声明值）；
//! 先解压到临时目录，通过后再整体替换，失败时回滚；签名无效的包拒绝安装。

use crate::manifest::{is_plugin_local_command, is_safe_relative, Manifest};
use crate::signature::{self, digest_from_hashes, hash_reader, Trust, TrustLevel, TrustedKey, SIGNATURE_FILE};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Serialize;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use zip::write::SimpleFileOptions;

const MAX_ENTRIES: usize = 5000;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_ICON_BYTES: u64 = 512 * 1024;
/// 通过拖放暂存到临时目录的插件包大小上限。
pub const MAX_PACKAGE_BYTES: usize = 300 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInfo {
    pub manifest: Manifest,
    pub sha256: String,
    pub package_size: u64,
    pub file_count: usize,
    pub unpacked_size: u64,
    /// 宿主 API 不满足插件要求时的原因；Some 表示不能安装。
    pub compat_error: Option<String>,
    /// 当前系统下用于展示的后端启动命令，如 "node main.js"。
    pub backend_command: Option<String>,
    /// 后端程序是否找得到：包内文件，或应用的 PATH 里的程序。无后端时为 None。
    pub backend_found: Option<bool>,
    /// 已安装的同 id 插件的版本（由 PluginManager 填写）。
    pub installed_version: Option<String>,
    /// 来源可信度：可信 / 未经安全验证 / 签名无效。
    pub trust: Trust,
    /// 插件图标（data URI），供安装确认框显示。
    pub icon_data: Option<String>,
    /// 将要安装到的目录与其名称（由 PluginManager 填写）。
    pub install_target: Option<String>,
    pub install_target_label: Option<String>,
    /// 后端程序找不到但可以自动下载运行环境时的提示（由 PluginManager 填写）。
    pub runtime_hint: Option<String>,
}

struct Opened {
    archive: zip::ZipArchive<File>,
    prefix: PathBuf,
    manifest: Manifest,
    signature: Option<String>,
    files: HashSet<PathBuf>,
    unpacked: u64,
}

fn is_symlink_mode(mode: Option<u32>) -> bool {
    mode.map(|m| m & 0o170000 == 0o120000).unwrap_or(false)
}

fn rel_string(rel: &Path) -> String {
    rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

fn read_entry(archive: &mut zip::ZipArchive<File>, target: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let mut index = None;
    for i in 0..archive.len() {
        let f = archive.by_index(i).map_err(|e| e.to_string())?;
        if f.enclosed_name().as_deref() == Some(target) {
            index = Some(i);
            break;
        }
    }
    let Some(i) = index else { return Ok(None) };
    let mut buf = Vec::new();
    archive
        .by_index(i)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("读取 {} 失败: {e}", target.display()))?;
    if buf.len() as u64 > limit {
        return Err(format!("{} 过大", target.display()));
    }
    Ok(Some(buf))
}

fn open(path: &Path) -> Result<Opened, String> {
    let file = File::open(path).map_err(|e| format!("无法打开文件: {e}"))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_PACKAGE_BYTES as u64 {
        return Err("插件包超过 300 MB 上限".into());
    }
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("不是有效的插件包（需要 zip 格式）: {e}"))?;
    if archive.len() > MAX_ENTRIES {
        return Err(format!("包内文件过多（超过 {MAX_ENTRIES} 个）"));
    }

    let mut entries: Vec<(PathBuf, bool, u64)> = Vec::with_capacity(archive.len());
    let mut seen_paths = HashSet::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        let f = archive.by_index(i).map_err(|e| format!("读取包内条目失败: {e}"))?;
        let entry_name = f.name().map_err(|e| format!("包内文件名无法解析: {e}"))?.into_owned();
        let name = f
            .enclosed_name()
            .ok_or_else(|| format!("包内含不安全的路径: {}", entry_name))?;
        let normalized = rel_string(&name);
        let path_key = if cfg!(windows) { normalized.to_ascii_lowercase() } else { normalized.clone() };
        if entry_name.contains('\\') || !is_safe_relative(entry_name.trim_end_matches('/')) || !is_safe_relative(&normalized) {
            return Err(format!("包内含不安全的路径: {}", entry_name));
        }
        if !seen_paths.insert(path_key) {
            return Err(format!("包内含重复路径: {}", entry_name));
        }
        if is_symlink_mode(f.unix_mode()) {
            return Err(format!("包内不允许符号链接: {}", entry_name));
        }
        if f.size() > MAX_FILE_BYTES {
            return Err(format!("包内文件过大: {}", entry_name));
        }
        total = total.saturating_add(f.size());
        entries.push((name, f.is_dir(), f.size()));
    }
    if total > MAX_TOTAL_BYTES {
        return Err("解压后体积过大".into());
    }

    // plugin.json 在根目录，或在唯一的顶层文件夹里（忽略 macOS 压缩产生的 __MACOSX）
    let prefix = if entries.iter().any(|(n, d, _)| !*d && n == Path::new("plugin.json")) {
        PathBuf::new()
    } else {
        let tops: HashSet<_> = entries
            .iter()
            .filter_map(|(n, _, _)| n.components().next().map(|c| c.as_os_str().to_owned()))
            .filter(|t| t != "__MACOSX")
            .collect();
        let mut it = tops.iter();
        match (it.next(), it.next()) {
            (Some(top), None)
                if entries
                    .iter()
                    .any(|(n, d, _)| !*d && n == &Path::new(top).join("plugin.json")) =>
            {
                PathBuf::from(top)
            }
            _ => return Err("包内找不到 plugin.json（应位于压缩包根目录）".into()),
        }
    };

    let manifest_bytes = read_entry(&mut archive, &prefix.join("plugin.json"), MAX_MANIFEST_BYTES)?
        .ok_or("包内找不到 plugin.json")?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("plugin.json 无法解析: {e}"))?;
    manifest.validate()?;
    let signature = read_entry(&mut archive, &prefix.join(SIGNATURE_FILE), 16 * 1024)?
        .map(|b| String::from_utf8_lossy(&b).into_owned());

    let mut files = HashSet::new();
    let mut unpacked = 0u64;
    let mut icon_size = None;
    for (n, is_dir, size) in &entries {
        if *is_dir {
            continue;
        }
        if let Ok(rel) = n.strip_prefix(&prefix) {
            if manifest.icon.as_deref().map(Path::new) == Some(rel) {
                icon_size = Some(*size);
            }
            files.insert(rel.to_path_buf());
            unpacked += size;
        }
    }
    if let Some(ui) = &manifest.ui {
        if !files.contains(Path::new(ui)) {
            return Err(format!("manifest 声明的界面文件不在包内: {ui}"));
        }
    }
    if let Some(icon) = &manifest.icon {
        if !files.contains(Path::new(icon)) {
            return Err(format!("manifest 声明的图标文件不在包内: {icon}"));
        }
        if icon_size.unwrap_or(0) > MAX_ICON_BYTES {
            return Err("图标文件过大（上限 512 KB）".into());
        }
    }
    if let Some(b) = &manifest.backend {
        for v in b.variants() {
            if is_plugin_local_command(&v.command) && !files.contains(Path::new(&v.command[2..])) {
                return Err(format!("manifest 声明的后端程序不在包内: {}", v.command));
            }
        }
    }

    Ok(Opened { archive, prefix, manifest, signature, files, unpacked })
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let f = File::open(path).map_err(|e| format!("无法打开文件: {e}"))?;
    let h = hash_reader(f).map_err(|e| format!("读取文件失败: {e}"))?;
    Ok(h.iter().map(|b| format!("{b:02x}")).collect())
}

/// 命令是否能在应用的 PATH 里找到。注意：从桌面图标启动的应用（尤其 macOS）
/// 看到的 PATH 往往比终端里的短，终端里能运行不代表这里能找到。
pub fn command_on_path(cmd: &str) -> bool {
    if cmd.contains('/') || cmd.contains('\\') {
        return Path::new(cmd).is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else { return false };
    let mut exts: Vec<String> = vec![String::new()];
    if cfg!(windows) {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into());
        exts.extend(pathext.split(';').filter(|s| !s.is_empty()).map(|s| s.to_ascii_lowercase()));
    }
    // Windows 的 WindowsApps 目录里有 python.exe 的应用商店占位程序，运行它只会跳转到商店，不算已安装
    let skip_store_stub = cfg!(windows) && cmd.to_ascii_lowercase().starts_with("python");
    std::env::split_paths(&paths)
        .filter(|dir| !(skip_store_stub && dir.to_string_lossy().to_ascii_lowercase().contains("windowsapps")))
        .any(|dir| exts.iter().any(|ext| dir.join(format!("{cmd}{ext}")).is_file()))
}

/// 包内容摘要（不含 signature.json），与 `signature::digest_dir` 对解压后的目录算出的结果一致。
fn digest_zip(path: &Path, prefix: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut ar = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for i in 0..ar.len() {
        let f = ar.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let Some(name) = f.enclosed_name() else { continue };
        let Ok(rel) = name.strip_prefix(prefix) else { continue };
        let rel = rel_string(rel);
        if rel.is_empty() || rel == SIGNATURE_FILE {
            continue;
        }
        let hash = hash_reader(f.take(MAX_FILE_BYTES + 1)).map_err(|e| format!("读取 {rel} 失败: {e}"))?;
        out.push((rel, hash));
    }
    Ok(digest_from_hashes(out))
}

fn trust_of(o: &Opened, path: &Path, trusted: &[TrustedKey]) -> Trust {
    signature::verify(
        o.signature.as_deref(),
        &o.manifest.id,
        &o.manifest.version,
        || digest_zip(path, &o.prefix),
        trusted,
    )
}

fn icon_data_uri(o: &mut Opened) -> Option<String> {
    let icon = o.manifest.icon.clone()?;
    let bytes = read_entry(&mut o.archive, &o.prefix.join(&icon), MAX_ICON_BYTES).ok()??;
    let mime = match Path::new(&icon).extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        _ => return None,
    };
    Some(format!("data:{mime};base64,{}", B64.encode(bytes)))
}

/// 只读检查：不解压、不安装。
pub fn inspect(path: &Path, trusted: &[TrustedKey]) -> Result<PackageInfo, String> {
    let mut o = open(path)?;
    let compat_error = o.manifest.check_compatible().err();
    let (backend_command, backend_found) = match o.manifest.backend.as_ref().and_then(|b| b.for_current_os()) {
        None => (None, None),
        Some(c) => {
            let shown = std::iter::once(c.command.as_str())
                .chain(c.args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            let found = is_plugin_local_command(&c.command) || command_on_path(&c.command);
            (Some(shown), Some(found))
        }
    };
    let trust = trust_of(&o, path, trusted);
    let icon_data = icon_data_uri(&mut o);
    Ok(PackageInfo {
        sha256: sha256_file(path)?,
        package_size: fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        file_count: o.files.len(),
        unpacked_size: o.unpacked,
        compat_error,
        backend_command,
        backend_found,
        installed_version: None,
        trust,
        icon_data,
        install_target: None,
        install_target_label: None,
        runtime_hint: None,
        manifest: o.manifest,
    })
}

/* ---------- 给签名工具用 ---------- */

pub struct PackageMeta {
    pub manifest: Manifest,
    /// 顶层文件夹名（plugin.json 在根目录时为空）。
    pub prefix: String,
}

/// 读取包的清单和内容摘要（不含 signature.json）。
pub fn digest_of(path: &Path) -> Result<(PackageMeta, String), String> {
    let o = open(path)?;
    let digest = digest_zip(path, &o.prefix)?;
    let prefix = rel_string(&o.prefix);
    Ok((PackageMeta { manifest: o.manifest, prefix }, digest))
}

/// 复制插件包并写入（或替换）signature.json；其他条目原样复制，不重新压缩。
pub fn write_signed(src: &Path, dst: &Path, signature_json: &str) -> Result<(), String> {
    let o = open(src)?;
    let sig_path = o.prefix.join(SIGNATURE_FILE);
    let sig_name = rel_string(&sig_path);
    let mut archive = o.archive;
    let mut w = zip::ZipWriter::new(File::create(dst).map_err(|e| format!("无法创建输出文件: {e}"))?);
    for i in 0..archive.len() {
        let f = archive.by_index_raw(i).map_err(|e| e.to_string())?;
        if f.enclosed_name().as_deref() == Some(sig_path.as_path()) {
            continue;
        }
        w.raw_copy_file(f).map_err(|e| e.to_string())?;
    }
    w.start_file(sig_name, SimpleFileOptions::default()).map_err(|e| e.to_string())?;
    w.write_all(signature_json.as_bytes()).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/* ---------- 解压与安装 ---------- */

fn extract_to(path: &Path, dest: &Path) -> Result<Manifest, String> {
    let mut o = open(path)?;
    fs::create_dir_all(dest).map_err(|e| format!("无法创建目录: {e}"))?;
    let mut written = 0u64;
    for i in 0..o.archive.len() {
        let mut f = o.archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = f.enclosed_name() else { continue };
        let Ok(rel) = name.strip_prefix(&o.prefix) else { continue };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        if f.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut w = File::create(&out).map_err(|e| format!("无法写入 {}: {e}", rel.display()))?;
        // 按实际读出的字节限流，不信任 zip 里声明的大小
        let n = io::copy(&mut f.by_ref().take(MAX_FILE_BYTES + 1), &mut w).map_err(|e| e.to_string())?;
        if n > MAX_FILE_BYTES {
            return Err(format!("包内文件过大: {}", rel.display()));
        }
        written += n;
        if written > MAX_TOTAL_BYTES {
            return Err("解压后体积过大".into());
        }
        #[cfg(unix)]
        if let Some(mode) = f.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&out, fs::Permissions::from_mode(mode & 0o777));
        }
    }
    Ok(o.manifest)
}

/// 已解压并校验通过、等待替换到位的插件。
pub struct Staged {
    pub manifest: Manifest,
    pub trust: Trust,
    pub dest: PathBuf,
    tmp: PathBuf,
    backup: PathBuf,
}

/// 第一阶段：把包复制到私有位置、校验哈希与签名、解压到临时目录。
/// 复制是为了避免"校验之后文件又被改动"。此阶段不影响已安装的任何内容。
/// `parent` 是插件目录（临时目录建在它下面），`dest` 是最终位置。
pub fn stage(
    path: &Path,
    expected_sha256: &str,
    parent: &Path,
    dest: PathBuf,
    trusted: &[TrustedKey],
) -> Result<Staged, String> {
    fs::create_dir_all(parent).map_err(|e| format!("无法创建插件目录: {e}"))?;
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let copy = parent.join(format!(".pkg-{}-{seq}.tmp", std::process::id()));
    let mut input = File::open(path).map_err(|e| format!("无法读取插件包: {e}"))?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut output = options.open(&copy).map_err(|e| format!("无法暂存插件包: {e}"))?;
    let copied = io::copy(&mut Read::by_ref(&mut input).take(MAX_PACKAGE_BYTES as u64 + 1), &mut output);
    drop(output);
    match copied {
        Ok(n) if n <= MAX_PACKAGE_BYTES as u64 => {},
        result => {
            let _ = fs::remove_file(&copy);
            return Err(match result { Ok(_) => "插件包超过 300 MB 上限".into(), Err(e) => format!("无法暂存插件包: {e}") });
        }
    }

    let result = (|| {
        let actual = sha256_file(&copy)?;
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            return Err("插件包在确认之后发生了变化，请重新导入".to_string());
        }
        let o = open(&copy)?;
        o.manifest.check_compatible()?;
        let trust = trust_of(&o, &copy, trusted);
        if trust.level == TrustLevel::Invalid {
            return Err(format!("签名校验失败，已拒绝安装：{}", trust.reason));
        }
        let id = o.manifest.id.clone();
        drop(o);
        let tmp = parent.join(format!(".installing-{id}"));
        let _ = fs::remove_dir_all(&tmp);
        match extract_to(&copy, &tmp) {
            Ok(manifest) => Ok(Staged {
                manifest,
                trust,
                dest,
                backup: parent.join(format!(".backup-{id}")),
                tmp,
            }),
            Err(e) => {
                let _ = fs::remove_dir_all(&tmp);
                Err(e)
            }
        }
    })();
    let _ = fs::remove_file(&copy);
    result
}

/// 第二阶段：用新目录替换旧目录。旧版本先改名备份，失败则恢复。
pub fn commit(s: Staged) -> Result<(), String> {
    let _ = fs::remove_dir_all(&s.backup);
    if s.dest.exists() {
        fs::rename(&s.dest, &s.backup).map_err(|e| {
            let _ = fs::remove_dir_all(&s.tmp);
            format!("无法替换已安装的版本（文件可能被占用）: {e}")
        })?;
    }
    if let Err(e) = fs::rename(&s.tmp, &s.dest) {
        if s.backup.exists() {
            let _ = fs::rename(&s.backup, &s.dest);
        }
        let _ = fs::remove_dir_all(&s.tmp);
        return Err(format!("安装失败: {e}"));
    }
    let _ = fs::remove_dir_all(&s.backup);
    Ok(())
}

/* ---------- 拖放进来的插件包：暂存到临时目录 ---------- */

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 把拖进窗口的文件内容存成临时文件，之后和选择文件得到的路径走同一套流程。
pub fn save_temp_package(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err("文件过大，不像是插件包".into());
    }
    fs::create_dir_all(dir).map_err(|e| format!("无法创建临时目录: {e}"))?;
    let base = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("package.nut");
    let clean: String = base
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') { c } else { '_' })
        .collect();
    let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = dir.join(format!("{}-{n}-{clean}", std::process::id()));
    fs::write(&path, bytes).map_err(|e| format!("无法写入临时文件: {e}"))?;
    Ok(path)
}

/// 删除临时文件。只会删除临时目录里的文件。
pub fn discard_temp_package(dir: &Path, path: &Path) -> Result<(), String> {
    let (Ok(d), Ok(p)) = (dir.canonicalize(), path.canonicalize()) else { return Ok(()) };
    if !p.starts_with(&d) {
        return Err("只能删除临时目录里的文件".into());
    }
    fs::remove_file(p).map_err(|e| e.to_string())
}

pub fn clean_temp_dir(dir: &Path) {
    let _ = fs::remove_dir_all(dir);
}
