use crate::layout::is_writable;
use crate::manifest::{is_plugin_local_command, CommandSpec, Manifest, MatchSpec, Permission};
use crate::package::{self, PackageInfo};
use crate::runtime::{Prepared, RuntimeManager, MISSING_PREFIX};
use crate::signature::{self, Trust, TrustedKey, SIGNATURE_FILE};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex as AsyncMutex};

const CALL_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_LOG_LINES: usize = 500;
const MAX_LOG_LINE_CHARS: usize = 2000;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;
type LogSink = Arc<Mutex<VecDeque<String>>>;

fn push_log(sink: &LogSink, line: impl AsRef<str>) {
    let line: String = line.as_ref().chars().take(MAX_LOG_LINE_CHARS).collect();
    let mut q = sink.lock().unwrap();
    if q.len() >= MAX_LOG_LINES {
        q.pop_front();
    }
    q.push_back(line);
}

/// 一组插件目录。两个目录里的插件地位相同；组内可以有多个物理路径（例如程序目录与安装资源目录），
/// 其中第一个是安装时使用的目录。
#[derive(Debug, Clone)]
pub struct PluginDir {
    pub key: String,
    pub label: String,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirPathInfo {
    pub path: String,
    pub exists: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirInfo {
    pub key: String,
    pub label: String,
    pub paths: Vec<DirPathInfo>,
    pub plugin_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// 插件目录内的图标文件（相对路径）。
    pub icon: Option<String>,
    pub category: String,
    pub keywords: Vec<String>,
    pub ui: Option<String>,
    pub permissions: Vec<Permission>,
    /// 智能内容匹配规则（可选）。
    #[serde(rename = "match")]
    pub r#match: Option<MatchSpec>,
    pub enabled: bool,
    pub error: Option<String>,
    pub has_backend: bool,
    pub backend_command: Option<String>,
    /// 后端进程当前是否正在运行，以及其操作系统 PID。
    pub backend_running: bool,
    pub backend_pid: Option<u32>,
    /// 所在的目录组（"appdata" / "program"）及其名称、完整路径。
    pub group: String,
    pub group_label: String,
    pub path: String,
    pub trust: Trust,
    /// 另一个优先级较低的目录里还有同 id 的插件，已被本插件覆盖。
    pub shadowed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallResult {
    pub id: String,
    pub path: String,
    pub group_label: String,
    /// 安装后实际生效的是不是这一份（优先级更高的目录里若有同 id 插件，它会生效）。
    pub active: bool,
}

struct Entry {
    id: String,
    name: String,
    version: String,
    dir: PathBuf,
    group: String,
    manifest: Option<Manifest>,
    error: Option<String>,
    trust: Trust,
    shadowed: Vec<String>,
}

/// 一个正在运行的插件后端进程，使用按行分隔的 JSON-RPC 与宿主通信。
struct Backend {
    pid: Option<u32>,
    started_at: Instant,
    last_used_ms: AtomicU64,
    stdin: AsyncMutex<ChildStdin>,
    child: AsyncMutex<Child>,
    pending: Pending,
    next_id: AtomicU64,
    alive: Arc<AtomicBool>,
}

fn resolve_command(dir: &Path, command: &str) -> PathBuf {
    if is_plugin_local_command(command) {
        dir.join(&command[2..])
    } else {
        PathBuf::from(command)
    }
}

fn spawn_backend(
    id: &str,
    dir: &Path,
    spec: &CommandSpec,
    log: LogSink,
    prepared: Prepared,
) -> Result<Backend, String> {
    let mut cmd = match &prepared {
        Prepared::Managed { program, path_dirs } => {
            let mut c = Command::new(program);
            let mut dirs: Vec<PathBuf> = path_dirs.clone();
            if let Some(old) = std::env::var_os("PATH") {
                dirs.extend(std::env::split_paths(&old));
            }
            if let Ok(joined) = std::env::join_paths(dirs) {
                c.env("PATH", joined);
            }
            c
        }
        _ => Command::new(resolve_command(dir, &spec.command)),
    };
    cmd.args(&spec.args)
        .current_dir(dir)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW，避免弹出控制台窗口

    match &prepared {
        Prepared::Managed { program, .. } => push_log(
            &log,
            format!("[宿主] 启动（使用已安装的运行时）: {} {}", program.display(), spec.args.join(" ")),
        ),
        _ => push_log(&log, format!("[宿主] 启动: {} {}", spec.command, spec.args.join(" "))),
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("无法启动插件后端 {}: {e}", spec.command))?;
    let pid = child.id();
    let stdin = child.stdin.take().ok_or("无法获取插件 stdin")?;
    let stdout = child.stdout.take().ok_or("无法获取插件 stdout")?;
    let stderr = child.stderr.take().ok_or("无法获取插件 stderr")?;

    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let alive = Arc::new(AtomicBool::new(true));

    {
        let pending = pending.clone();
        let alive = alive.clone();
        let log = log.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    push_log(&log, format!("[stdout 非 JSON] {line}"));
                    continue;
                };
                let Some(rid) = msg.get("id").and_then(Value::as_u64) else { continue };
                let outcome = match msg.get("error") {
                    Some(err) => Err(err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("插件返回错误")
                        .to_string()),
                    None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                };
                if let Some(tx) = pending.lock().unwrap().remove(&rid) {
                    let _ = tx.send(outcome);
                }
            }
            // 先标记为已退出，再清空等待中的请求，避免新请求在清空后无人应答
            alive.store(false, Ordering::SeqCst);
            push_log(&log, "[宿主] 后端进程已退出");
            for (_, tx) in pending.lock().unwrap().drain() {
                let _ = tx.send(Err("插件进程已退出".into()));
            }
        });
    }

    let tag = id.to_string();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("[{tag}] {line}");
            push_log(&log, line);
        }
    });

    Ok(Backend {
        pid,
        started_at: Instant::now(),
        last_used_ms: AtomicU64::new(0),
        stdin: AsyncMutex::new(stdin),
        child: AsyncMutex::new(child),
        pending,
        next_id: AtomicU64::new(1),
        alive,
    })
}

impl Backend {
    fn touch(&self) {
        let ms = self.started_at.elapsed().as_millis() as u64;
        self.last_used_ms.store(ms, Ordering::SeqCst);
    }

    fn idle_duration(&self) -> Duration {
        if !self.pending.lock().unwrap().is_empty() {
            return Duration::ZERO;
        }
        let elapsed = self.started_at.elapsed();
        let last = Duration::from_millis(self.last_used_ms.load(Ordering::SeqCst));
        elapsed.saturating_sub(last)
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.touch();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        if !self.alive.load(Ordering::SeqCst) {
            self.pending.lock().unwrap().remove(&id);
            return Err("插件进程已退出".into());
        }

        let mut line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        line.push('\n');
        let written = {
            let mut stdin = self.stdin.lock().await;
            match stdin.write_all(line.as_bytes()).await {
                Ok(()) => stdin.flush().await,
                Err(e) => Err(e),
            }
        };
        if let Err(e) = written {
            self.pending.lock().unwrap().remove(&id);
            return Err(format!("写入插件失败: {e}"));
        }

        let res = match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err("插件进程已退出".into()),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err("插件响应超时".into())
            }
        };
        self.touch();
        res
    }

    async fn kill(&self) {
        self.alive.store(false, Ordering::SeqCst);
        let _ = self.child.lock().await.kill().await;
    }
}

pub struct PluginManager {
    groups: RwLock<Vec<PluginDir>>,
    state_file: PathBuf,
    trusted: Vec<TrustedKey>,
    entries: RwLock<HashMap<String, Entry>>,
    disabled: RwLock<HashSet<String>>,
    running: Mutex<HashMap<String, Arc<Backend>>>,
    logs: RwLock<HashMap<String, LogSink>>,
    runtimes: Option<Arc<RuntimeManager>>,
}

impl PluginManager {
    /// `groups` 的顺序就是优先级：靠前的目录组里的插件优先生效，新插件也优先安装到靠前且可写的目录。
    pub fn new(groups: Vec<PluginDir>, state_file: PathBuf) -> Self {
        let disabled = std::fs::read_to_string(&state_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| {
                v.get("disabled").and_then(Value::as_array).map(|a| {
                    a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<HashSet<_>>()
                })
            })
            .unwrap_or_default();
        Self {
            groups: RwLock::new(groups),
            state_file,
            trusted: Vec::new(),
            entries: RwLock::new(HashMap::new()),
            disabled: RwLock::new(disabled),
            running: Mutex::new(HashMap::new()),
            logs: RwLock::new(HashMap::new()),
            runtimes: None,
        }
    }

    /// 运行时管理器：启动后端时优先使用已安装的运行时，缺失时返回 `RuntimeMissing` 错误。
    pub fn with_runtimes(mut self, runtimes: Arc<RuntimeManager>) -> Self {
        self.runtimes = Some(runtimes);
        self
    }

    /// 受信任的签名公钥。由这些密钥签名且校验通过的插件会标注为"可信来源"。
    pub fn with_trusted_keys(mut self, keys: Vec<TrustedKey>) -> Self {
        self.trusted = keys;
        self
    }

    pub fn trusted_keys(&self) -> &[TrustedKey] {
        &self.trusted
    }

    /// 发现阶段：读取并校验 plugin.json 和签名，不执行任何插件代码。以 "." 开头的目录（安装中的临时目录）会被忽略。
    /// 同 id 的插件，优先级高的目录里的生效。
    pub fn scan(&self) {
        let groups = self.groups.read().unwrap().clone();
        let mut found: HashMap<String, Entry> = HashMap::new();
        for g in &groups {
            for base in &g.paths {
                let Ok(read) = std::fs::read_dir(base) else { continue };
                let mut items: Vec<_> = read.flatten().collect();
                items.sort_by_key(|e| e.file_name());
                for item in items {
                    let dir_name = item.file_name().to_string_lossy().into_owned();
                    if dir_name.starts_with('.') {
                        continue;
                    }
                    let dir = item.path();
                    let file = dir.join("plugin.json");
                    if !file.is_file() {
                        continue;
                    }
                    let parsed = std::fs::read_to_string(&file)
                        .map_err(|e| e.to_string())
                        .and_then(|s| serde_json::from_str::<Manifest>(&s).map_err(|e| e.to_string()))
                        .and_then(|m| m.validate().map(|_| m));
                    let entry = match parsed {
                        Ok(m) => {
                            let error = m.check_compatible().err();
                            let sig = std::fs::read_to_string(dir.join(SIGNATURE_FILE)).ok();
                            let trust = signature::verify(
                                sig.as_deref(),
                                &m.id,
                                &m.version,
                                || signature::digest_dir(&dir),
                                &self.trusted,
                            );
                            Entry {
                                id: m.id.clone(),
                                name: m.name.clone(),
                                version: m.version.clone(),
                                dir,
                                group: g.key.clone(),
                                manifest: Some(m),
                                error,
                                trust,
                                shadowed: Vec::new(),
                            }
                        }
                        Err(e) => Entry {
                            id: format!("invalid:{dir_name}"),
                            name: dir_name,
                            version: "?".into(),
                            dir,
                            group: g.key.clone(),
                            manifest: None,
                            error: Some(format!("manifest 无效: {e}")),
                            trust: Trust::unverified("manifest 无效，无法验证"),
                            shadowed: Vec::new(),
                        },
                    };
                    match found.get_mut(&entry.id) {
                        Some(winner) => winner.shadowed.push(entry.dir.display().to_string()),
                        None => {
                            found.insert(entry.id.clone(), entry);
                        }
                    }
                }
            }
        }
        *self.entries.write().unwrap() = found;
    }

    /// 重新扫描，并停掉已经不存在的插件的后端。
    pub async fn reload(&self) {
        self.scan();
        let present: HashSet<String> = self.entries.read().unwrap().keys().cloned().collect();
        let stale: Vec<String> = self
            .running
            .lock()
            .unwrap()
            .keys()
            .filter(|k| !present.contains(*k))
            .cloned()
            .collect();
        for id in stale {
            self.stop(&id).await;
        }
    }

    fn group_label(&self, key: &str) -> String {
        self.groups
            .read()
            .unwrap()
            .iter()
            .find(|g| g.key == key)
            .map(|g| g.label.clone())
            .unwrap_or_else(|| key.to_string())
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        let disabled = self.disabled.read().unwrap();
        let mut running = self.running.lock().unwrap();
        running.retain(|_, b| b.alive.load(Ordering::SeqCst));
        let mut out: Vec<PluginInfo> = self
            .entries
            .read()
            .unwrap()
            .values()
            .map(|e| {
                let m = e.manifest.as_ref();
                let backend = m
                    .and_then(|m| m.backend.as_ref())
                    .map(|b| b.for_current_os());
                let (backend_running, backend_pid) = running
                    .get(&e.id)
                    .map(|b| (true, b.pid))
                    .unwrap_or((false, None));
                PluginInfo {
                    id: e.id.clone(),
                    name: e.name.clone(),
                    version: e.version.clone(),
                    description: m.map(|m| m.description.clone()).unwrap_or_default(),
                    icon: m.and_then(|m| m.icon.clone()),
                    category: m.map(|m| m.category.clone()).unwrap_or_else(|| "其他".into()),
                    keywords: m.map(|m| m.keywords.clone()).unwrap_or_default(),
                    ui: m.and_then(|m| m.ui.clone()),
                    permissions: m.map(|m| m.permissions.clone()).unwrap_or_default(),
                    r#match: m.and_then(|m| m.r#match.clone()),
                    enabled: !disabled.contains(&e.id),
                    error: e.error.clone(),
                    has_backend: backend.is_some(),
                    backend_command: backend.flatten().map(|c| {
                        std::iter::once(c.command.as_str())
                            .chain(c.args.iter().map(String::as_str))
                            .collect::<Vec<_>>()
                            .join(" ")
                    }),
                    backend_running,
                    backend_pid,
                    group: e.group.clone(),
                    group_label: self.group_label(&e.group),
                    path: e.dir.display().to_string(),
                    trust: e.trust.clone(),
                    shadowed: e.shadowed.clone(),
                }
            })
            .collect();
        out.sort_by(|a, b| a.category.cmp(&b.category).then(a.name.cmp(&b.name)));
        out
    }

    /// 供资源协议使用：只有可用且已启用的插件才允许读取其文件。
    pub fn plugin_dir(&self, id: &str) -> Option<PathBuf> {
        if self.disabled.read().unwrap().contains(id) {
            return None;
        }
        let entries = self.entries.read().unwrap();
        let e = entries.get(id)?;
        if e.error.is_some() {
            return None;
        }
        Some(e.dir.clone())
    }

    /// 宿主能力的统一检查入口：新增宿主 API 时先调用它。
    pub fn has_permission(&self, id: &str, perm: Permission) -> bool {
        self.entries
            .read()
            .unwrap()
            .get(id)
            .and_then(|e| e.manifest.as_ref())
            .map(|m| m.permissions.contains(&perm))
            .unwrap_or(false)
    }

    /* ---------- 目录与优先级 ---------- */

    pub fn dirs_info(&self) -> Vec<DirInfo> {
        let groups = self.groups.read().unwrap().clone();
        let entries = self.entries.read().unwrap();
        groups
            .iter()
            .map(|g| DirInfo {
                key: g.key.clone(),
                label: g.label.clone(),
                paths: g
                    .paths
                    .iter()
                    .map(|p| DirPathInfo { path: p.display().to_string(), exists: p.is_dir(), writable: is_writable(p) })
                    .collect(),
                plugin_count: entries.values().filter(|e| e.group == g.key).count(),
            })
            .collect()
    }

    /// 调整目录优先级。`order` 里没提到的目录组按原顺序排在最后；出现未知的键则报错。
    pub fn set_order(&self, order: &[String]) -> Result<(), String> {
        {
            let mut groups = self.groups.write().unwrap();
            if let Some(bad) = order.iter().find(|k| !groups.iter().any(|g| &g.key == *k)) {
                return Err(format!("未知的插件目录: {bad}"));
            }
            let mut sorted: Vec<PluginDir> = Vec::new();
            for k in order {
                if !sorted.iter().any(|g| &g.key == k) {
                    if let Some(g) = groups.iter().find(|g| &g.key == k) {
                        sorted.push(g.clone());
                    }
                }
            }
            for g in groups.iter() {
                if !sorted.iter().any(|s| s.key == g.key) {
                    sorted.push(g.clone());
                }
            }
            *groups = sorted;
        }
        self.scan();
        Ok(())
    }

    /// 新插件的安装位置：按优先级排第一、且可写的目录组的第一个路径。
    fn install_dir(&self) -> Result<(String, PathBuf), String> {
        for g in self.groups.read().unwrap().iter() {
            if let Some(p) = g.paths.first() {
                if is_writable(p) {
                    return Ok((g.label.clone(), p.clone()));
                }
            }
        }
        Err("没有可写的插件目录，请检查权限，或在设置里调整目录优先级".into())
    }

    /* ---------- 启用、日志、调用 ---------- */

    fn persist(&self, disabled: &HashSet<String>) -> Result<(), String> {
        if let Some(parent) = self.state_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut ids: Vec<&String> = disabled.iter().collect();
        ids.sort();
        std::fs::write(&self.state_file, json!({ "disabled": ids }).to_string())
            .map_err(|e| format!("保存状态失败: {e}"))
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        {
            let mut disabled = self.disabled.write().unwrap();
            if enabled {
                disabled.remove(id);
            } else {
                disabled.insert(id.to_string());
            }
            self.persist(&disabled)?;
        }
        if !enabled {
            self.stop(id).await;
        }
        Ok(())
    }

    /// 后端的最近输出（stderr、启动失败原因、退出记录），最多 500 行。
    pub fn logs(&self, id: &str) -> Vec<String> {
        self.logs
            .read()
            .unwrap()
            .get(id)
            .map(|s| s.lock().unwrap().iter().cloned().collect())
            .unwrap_or_default()
    }

    fn sink(&self, id: &str) -> LogSink {
        self.logs.write().unwrap().entry(id.to_string()).or_default().clone()
    }

    /// 调用插件后端的某个方法。后端在第一次调用时才会启动（懒加载）。
    pub async fn invoke(&self, id: &str, method: &str, params: Value) -> Result<Value, String> {
        let (dir, spec) = {
            let entries = self.entries.read().unwrap();
            let e = entries.get(id).ok_or_else(|| format!("插件不存在: {id}"))?;
            if let Some(err) = &e.error {
                return Err(format!("插件不可用: {err}"));
            }
            let spec = e
                .manifest
                .as_ref()
                .and_then(|m| m.backend.as_ref())
                .ok_or("该插件没有后端")?
                .for_current_os()
                .ok_or("该插件没有适用于当前系统的后端")?;
            (e.dir.clone(), spec)
        };
        if self.disabled.read().unwrap().contains(id) {
            return Err("插件已禁用".into());
        }
        let backend = self.get_or_start(id, &dir, &spec)?;
        backend.call(method, params).await
    }

    fn get_or_start(&self, id: &str, dir: &Path, spec: &CommandSpec) -> Result<Arc<Backend>, String> {
        let mut running = self.running.lock().unwrap();
        if let Some(b) = running.get(id) {
            if b.alive.load(Ordering::SeqCst) {
                return Ok(b.clone());
            }
            running.remove(id); // 进程已崩溃或退出，下面重新拉起
        }
        let log = self.sink(id);
        let prepared = match (&self.runtimes, is_plugin_local_command(&spec.command)) {
            (Some(rt), false) => rt.prepare(&spec.command),
            _ => Prepared::System,
        };
        if let Prepared::Missing(m) = &prepared {
            push_log(&log, format!("[宿主] 缺少运行时: {} {}", m.name, m.version));
            let detail = serde_json::to_string(m).unwrap_or_default();
            return Err(format!("{MISSING_PREFIX}{detail}"));
        }
        let backend = match spawn_backend(id, dir, spec, log.clone(), prepared) {
            Ok(b) => Arc::new(b),
            Err(e) => {
                push_log(&log, format!("[宿主] {e}"));
                return Err(e);
            }
        };
        running.insert(id.to_string(), backend.clone());
        Ok(backend)
    }

    pub async fn stop(&self, id: &str) {
        let backend = self.running.lock().unwrap().remove(id);
        if let Some(b) = backend {
            b.kill().await;
        }
    }

    pub async fn stop_all(&self) -> usize {
        let all: Vec<Arc<Backend>> = self
            .running
            .lock()
            .unwrap()
            .drain()
            .map(|(_, b)| b)
            .filter(|b| b.alive.load(Ordering::SeqCst))
            .collect();
        let count = all.len();
        for b in all {
            b.kill().await;
        }
        count
    }

    /// 结束闲置时间超过 `max_idle` 的后端进程，返回被停止的插件 ID 列表。
    pub async fn reap_idle(&self, max_idle: Duration) -> Vec<String> {
        let idle: Vec<(String, Arc<Backend>)> = {
            let mut running = self.running.lock().unwrap();
            running.retain(|_, b| b.alive.load(Ordering::SeqCst));
            let ids: Vec<String> = running
                .iter()
                .filter(|(_, b)| b.idle_duration() >= max_idle)
                .map(|(k, _)| k.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| running.remove(&id).map(|b| (id, b)))
                .collect()
        };
        let mut stopped = Vec::with_capacity(idle.len());
        for (id, b) in idle {
            push_log(&self.sink(&id), "[宿主] 后端闲置超时，已自动休眠");
            b.kill().await;
            stopped.push(id);
        }
        stopped
    }

    /* ---------- 插件包：检查 / 安装 / 卸载 ---------- */

    /// 检查插件包（不安装）。返回内容里带有已安装的同 id 版本、将要安装到的目录、来源可信度。
    pub async fn inspect_package(&self, path: &Path) -> Result<PackageInfo, String> {
        let p = path.to_path_buf();
        let trusted = self.trusted.clone();
        let mut info = tokio::task::spawn_blocking(move || package::inspect(&p, &trusted))
            .await
            .map_err(|e| e.to_string())??;
        info.installed_version = self
            .entries
            .read()
            .unwrap()
            .get(&info.manifest.id)
            .map(|e| e.version.clone());
        if let (Some(rt), Some(cmd)) = (&self.runtimes, info.backend_command.as_deref()) {
            let program = cmd.split_whitespace().next().unwrap_or_default();
            if info.backend_found == Some(false) && rt.resolve(program).is_some() {
                info.backend_found = Some(true);
            }
            if info.backend_found == Some(false) && rt.has_recipe(program) {
                info.runtime_hint = Some(format!("系统里没有 {program}，首次使用这个插件时可以自动下载运行环境"));
            }
        }
        if let Ok((label, dir)) = self.install_dir() {
            info.install_target = Some(dir.display().to_string());
            info.install_target_label = Some(label);
        }
        Ok(info)
    }

    /// 安装插件包。`expected_sha256` 必须是 `inspect_package` 返回的哈希，
    /// 这样用户确认的就是实际安装的那一份文件。签名无效的包会被拒绝。
    pub async fn install_package(&self, path: &Path, expected_sha256: &str) -> Result<InstallResult, String> {
        let (label, parent) = self.install_dir()?;
        let (p, sha, trusted) = (path.to_path_buf(), expected_sha256.to_string(), self.trusted.clone());

        // 先读出 id，决定最终位置：目标目录里已有这个插件（哪怕文件夹名不同）就原地替换
        let p2 = p.clone();
        let tr2 = trusted.clone();
        let id = tokio::task::spawn_blocking(move || package::inspect(&p2, &tr2).map(|i| i.manifest.id))
            .await
            .map_err(|e| e.to_string())??;
        let dest = {
            let entries = self.entries.read().unwrap();
            match entries.get(&id) {
                Some(e) if e.dir.parent() == Some(parent.as_path()) => e.dir.clone(),
                _ => parent.join(&id),
            }
        };

        let (parent2, dest2) = (parent.clone(), dest.clone());
        let staged = tokio::task::spawn_blocking(move || package::stage(&p, &sha, &parent2, dest2, &trusted))
            .await
            .map_err(|e| e.to_string())??;
        self.stop(&id).await; // 替换文件前先停掉旧后端（Windows 上运行中的文件无法替换）
        let committed = tokio::task::spawn_blocking(move || package::commit(staged))
            .await
            .map_err(|e| e.to_string())?;
        committed?;
        self.scan();

        let active = self.entries.read().unwrap().get(&id).map(|e| e.dir == dest).unwrap_or(false);
        Ok(InstallResult { id, path: dest.display().to_string(), group_label: label, active })
    }

    /// 卸载插件（删除它的文件夹）。两个目录里的插件地位相同；没有权限时会给出提示。
    /// 损坏的、无法识别的插件目录也可以删除。
    pub async fn uninstall(&self, id: &str) -> Result<(), String> {
        let dir = {
            let entries = self.entries.read().unwrap();
            entries.get(id).ok_or("插件不存在")?.dir.clone()
        };
        self.stop(id).await;
        let d = dir.clone();
        tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&d))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("删除失败（可能没有权限，或文件被占用）: {e}"))?;
        {
            let mut disabled = self.disabled.write().unwrap();
            if disabled.remove(id) {
                let _ = self.persist(&disabled);
            }
        }
        self.logs.write().unwrap().remove(id);
        self.scan();
        Ok(())
    }
}
