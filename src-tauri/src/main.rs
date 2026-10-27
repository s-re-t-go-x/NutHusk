#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use nuthusk_core::{
    package, DataRoot, IndexResult, InstallResult, InstalledRuntime, Market, NutProfile,
    PackageInfo, Permission, PluginInfo, PluginManager, ProfilePlugin, ProfileSource,
    ProfileWorkspace, RootPathInfo, Roots, RuntimeManager, RuntimeStatus, Settings, Source,
    CONFIG_DIR, PROFILE_FORMAT, PROFILE_VERSION,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{
    http,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};

const SDK_JS: &str = include_str!("../../sdk/ui.js");
const SDK_CSS: &str = include_str!("../../sdk/ui.css");
/// 受信任的签名公钥。默认为空：此时所有插件都显示"未经安全验证"。
/// 生成密钥并填入公钥的方法见 docs/SIGNING.md。
const TRUSTED_KEYS_JSON: &str = include_str!("../trusted_keys.json");

struct AppState {
    manager: Arc<PluginManager>,
    settings: Arc<Settings>,
    roots: Arc<Roots>,
    runtimes: Arc<RuntimeManager>,
    market: Arc<Market>,
    /// 拖进窗口的插件包、从插件下载页下载的插件包都会先暂存在这里，处理完就删除。
    drop_dir: PathBuf,
    /// 正在进行的下载：key → 取消标志。
    jobs: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// 从托盘菜单点击"退出"时置为 true，放行窗口关闭。
    quitting: AtomicBool,
}

/// 下载进度事件（"download-progress"）的内容。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    key: String,
    downloaded: u64,
    total: Option<u64>,
}

fn start_job(state: &AppState, key: &str) -> Result<Arc<AtomicBool>, String> {
    let mut jobs = state.jobs.lock().unwrap();
    if jobs.contains_key(key) {
        return Err("这个下载正在进行中".into());
    }
    let flag = Arc::new(AtomicBool::new(false));
    jobs.insert(key.to_string(), flag.clone());
    Ok(flag)
}

fn end_job(state: &AppState, key: &str) {
    state.jobs.lock().unwrap().remove(key);
}

fn emitter(app: &AppHandle, key: &str) -> impl FnMut(u64, Option<u64>) {
    let (app, key) = (app.clone(), key.to_string());
    move |downloaded, total| {
        let _ = app.emit(
            "download-progress",
            Progress {
                key: key.clone(),
                downloaded,
                total,
            },
        );
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Signer {
    name: String,
    key_id: String,
}

#[tauri::command]
fn list_plugins(state: State<'_, AppState>) -> Vec<PluginInfo> {
    state.manager.list()
}

#[tauri::command]
async fn set_plugin_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    state.manager.set_enabled(&id, enabled).await
}

/// 壳界面把插件界面发来的调用转发给对应插件的后端进程。
#[tauri::command]
async fn plugin_call(
    state: State<'_, AppState>,
    id: String,
    method: String,
    params: Value,
) -> Result<Value, String> {
    state.manager.invoke(&id, &method, params).await
}

/// 结束指定插件的后端进程。
#[tauri::command]
async fn stop_plugin_backend(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.manager.stop(&id).await;
    Ok(())
}

/// 结束所有正在运行的插件后端进程，返回被停止的进程数。
#[tauri::command]
async fn stop_all_backends(state: State<'_, AppState>) -> Result<usize, String> {
    Ok(state.manager.stop_all().await)
}

/// 设置主窗口是否置顶（Always on Top），并同步保存到设置中。
#[tauri::command]
fn set_always_on_top(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    on_top: bool,
) -> Result<bool, String> {
    window
        .set_always_on_top(on_top)
        .map_err(|e| format!("无法设置窗口置顶: {e}"))?;
    let _ = state
        .settings
        .patch(&serde_json::json!({ "alwaysOnTop": on_top }));
    Ok(on_top)
}

/// 检查插件包但不安装，返回清单、权限、后端命令、来源可信度和 SHA-256，供用户确认。
#[tauri::command]
async fn inspect_plugin_package(
    state: State<'_, AppState>,
    path: String,
) -> Result<PackageInfo, String> {
    state.manager.inspect_package(Path::new(&path)).await
}

/// 安装插件包。sha256 必须是 inspect_plugin_package 返回的值，确保装的就是用户确认过的那份文件。
#[tauri::command]
async fn install_plugin_package(
    state: State<'_, AppState>,
    path: String,
    sha256: String,
) -> Result<InstallResult, String> {
    state
        .manager
        .install_package(Path::new(&path), &sha256)
        .await
}

#[tauri::command]
async fn uninstall_plugin(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.manager.uninstall(&id).await
}

#[tauri::command]
fn plugin_logs(state: State<'_, AppState>, id: String) -> Vec<String> {
    state.manager.logs(&id)
}

#[tauri::command]
async fn reload_plugins(state: State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    state.manager.reload().await;
    Ok(state.manager.list())
}

/// 单个插件的开发模式热重载：停止其后端进程、重新扫描清单并返回最新插件列表。
#[tauri::command]
async fn reload_single_plugin(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<PluginInfo>, String> {
    state.manager.reload_single(&id).await;
    Ok(state.manager.list())
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Value {
    state.settings.get()
}

#[tauri::command]
fn patch_settings(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    patch: Value,
) -> Result<Value, String> {
    let next = state.settings.patch(&patch)?;
    if let Some(on_top) = patch.get("alwaysOnTop").and_then(Value::as_bool) {
        let _ = window.set_always_on_top(on_top);
    }
    Ok(next)
}

/// 数据根目录的展示信息：每个根下的 nutPlugins / nutRuntimes / nutConfig，以及里面有多少插件、运行时。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RootView {
    key: String,
    label: String,
    paths: Vec<RootPathInfo>,
    subdirs: Vec<String>,
    plugin_count: usize,
    runtime_count: usize,
}

fn root_views(state: &AppState) -> Vec<RootView> {
    let plugin_counts: HashMap<String, usize> = state
        .manager
        .dirs_info()
        .into_iter()
        .map(|d| (d.key, d.plugin_count))
        .collect();
    let installed: Vec<InstalledRuntime> = state.runtimes.installed();
    state
        .roots
        .info()
        .into_iter()
        .map(|r| RootView {
            plugin_count: plugin_counts.get(&r.key).copied().unwrap_or(0),
            runtime_count: installed
                .iter()
                .filter(|i| {
                    r.paths
                        .iter()
                        .any(|p| i.location.starts_with(Path::new(&p.path)))
                })
                .count(),
            key: r.key,
            label: r.label,
            paths: r.paths,
            subdirs: r.subdirs,
        })
        .collect()
}

#[tauri::command]
fn plugin_dirs(state: State<'_, AppState>) -> Vec<RootView> {
    root_views(&state)
}

/// 调整数据根目录的优先级（插件、运行时、配置都跟着调整）。
#[tauri::command]
fn set_dir_order(state: State<'_, AppState>, order: Vec<String>) -> Result<Vec<RootView>, String> {
    state.roots.set_order(&order)?;
    let keys = state.roots.keys();
    state.manager.set_order(&keys)?;
    let _ = state
        .settings
        .patch(&serde_json::json!({ "dirOrder": keys }));
    Ok(root_views(&state))
}

/* ---------- 运行时（解释器） ---------- */

#[tauri::command]
fn runtime_status(state: State<'_, AppState>) -> Vec<RuntimeStatus> {
    state.runtimes.status()
}

/// 下载并安装运行时。进度通过 "download-progress" 事件（key = "runtime:<名字>"）推送。
#[tauri::command]
async fn runtime_install(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<InstalledRuntime, String> {
    let key = format!("runtime:{name}");
    let cancel = start_job(&state, &key)?;
    let rt = state.runtimes.clone();
    let mut on_progress = emitter(&app, &key);
    let result =
        tauri::async_runtime::spawn_blocking(move || rt.install(&name, &mut on_progress, &cancel))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
    end_job(&state, &key);
    result
}

#[tauri::command]
fn runtime_remove(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state.runtimes.remove(&name)
}

/// 取消正在进行的下载（运行时或插件包）。
#[tauri::command]
fn download_cancel(state: State<'_, AppState>, key: String) {
    if let Some(flag) = state.jobs.lock().unwrap().get(&key) {
        flag.store(true, Ordering::SeqCst);
    }
}

/* ---------- 插件下载 ---------- */

#[tauri::command]
fn market_sources(state: State<'_, AppState>) -> Vec<Source> {
    state.market.sources()
}

#[tauri::command]
async fn market_index(
    state: State<'_, AppState>,
    source_id: String,
) -> Result<IndexResult, String> {
    let market = state.market.clone();
    let source = market.find_source(&source_id).ok_or("插件源不存在")?;
    tauri::async_runtime::spawn_blocking(move || market.fetch_index(&source))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r)
}

#[tauri::command]
fn source_add(
    state: State<'_, AppState>,
    url: String,
    name: Option<String>,
) -> Result<Source, String> {
    state.market.add_source(&url, name.as_deref())
}

#[tauri::command]
fn source_remove(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.market.remove_source(&id)
}

/// 下载插件包到临时目录并返回路径，之后与"导入"走同一套检查/确认/安装流程。
/// `source_id` 为官方源时限制在 GitHub 域名并要求 sha256；手动粘贴的地址只要求 https。
/// 进度事件的 key 是 "pkg:<地址>"。
#[tauri::command]
async fn market_download(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    sha256: Option<String>,
    source_id: Option<String>,
) -> Result<String, String> {
    let key = format!("pkg:{url}");
    let cancel = start_job(&state, &key)?;
    let market = state.market.clone();
    let dir = state.drop_dir.clone();
    let official = source_id.as_deref() == Some(nuthusk_core::OFFICIAL_ID);
    let mut on_progress = emitter(&app, &key);
    let result = tauri::async_runtime::spawn_blocking(move || {
        market.download_package(
            &url,
            sha256.as_deref(),
            official,
            &dir,
            &mut on_progress,
            &cancel,
        )
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r)
    .map(|p| p.to_string_lossy().into_owned());
    end_job(&state, &key);
    result
}

#[tauri::command]
fn trusted_signers(state: State<'_, AppState>) -> Vec<Signer> {
    state
        .manager
        .trusted_keys()
        .iter()
        .map(|k| Signer {
            name: k.name.clone(),
            key_id: k.key_id.clone(),
        })
        .collect()
}

/* ---------- 配置加载清单（.nutprofile）导入与导出 ---------- */

fn build_current_profile(
    state: &AppState,
    name: Option<String>,
    workspace: Option<ProfileWorkspace>,
) -> NutProfile {
    let sources = state
        .market
        .sources()
        .into_iter()
        .filter(|s| !s.official)
        .map(|s| ProfileSource {
            name: s.name,
            url: s.url,
        })
        .collect();
    let runtimes = state
        .runtimes
        .installed()
        .into_iter()
        .map(|r| r.name)
        .collect();
    let plugins = state
        .manager
        .list()
        .into_iter()
        .map(|p| ProfilePlugin {
            id: p.id,
            name: p.name,
            version: Some(p.version),
            enabled: p.enabled,
            url: None,
            sha256: None,
        })
        .collect();
    NutProfile {
        format: PROFILE_FORMAT.into(),
        version: PROFILE_VERSION,
        name,
        settings: state.settings.get(),
        sources,
        runtimes,
        plugins,
        workspace,
    }
}

/// 导出当前工作台的配置清单文件（`.nutprofile`）。若传入 `path` 则写入磁盘并返回完整对象。
#[tauri::command]
fn export_profile(
    state: State<'_, AppState>,
    path: Option<String>,
    name: Option<String>,
    workspace: Option<ProfileWorkspace>,
) -> Result<NutProfile, String> {
    let profile = build_current_profile(&state, name, workspace);
    if let Some(p) = path {
        profile.write_file(Path::new(&p))?;
    }
    Ok(profile)
}

/// 从文件路径读取并校验 `.nutprofile` 配置清单文件。
#[tauri::command]
fn read_profile_file(path: String) -> Result<NutProfile, String> {
    NutProfile::read_file(Path::new(&path))
}

/// 接收拖进窗口的文件内容（二进制），存成临时文件并返回路径，之后与"选择文件"走同一套检查/安装流程。
#[tauri::command]
fn save_dropped_package(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("需要二进制数据".into());
    };
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .map(|s| {
            percent_encoding::percent_decode_str(s)
                .decode_utf8_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| "package.nut".into());
    package::save_temp_package(&state.drop_dir, &name, bytes)
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
fn discard_dropped_package(state: State<'_, AppState>, path: String) -> Result<(), String> {
    package::discard_temp_package(&state.drop_dir, Path::new(&path))
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 插件页面的内容安全策略：默认禁止一切联网，声明了 network 权限的插件才放开。
fn plugin_csp(network: bool) -> String {
    let connect = if network {
        "connect-src http: https:"
    } else {
        "connect-src 'none'"
    };
    format!(
        "default-src 'none'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; font-src 'self' data:; {connect}; form-action 'none'"
    )
}

fn reply(status: u16, mime: &str, body: Vec<u8>, csp: Option<String>) -> http::Response<Vec<u8>> {
    let mut b = http::Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store");
    if let Some(csp) = csp {
        b = b.header("Content-Security-Policy", csp);
    }
    b.body(body).unwrap()
}

/// plugin://localhost/<插件id>/<文件>  （Windows 上为 http://plugin.localhost/...）
fn serve_plugin(manager: &PluginManager, req: &http::Request<Vec<u8>>) -> http::Response<Vec<u8>> {
    let not_found = || reply(404, "text/plain", b"not found".to_vec(), None);
    let forbidden = || reply(403, "text/plain", b"forbidden".to_vec(), None);

    let decoded = percent_encoding::percent_decode_str(req.uri().path())
        .decode_utf8_lossy()
        .into_owned();
    let Some((id, file)) = decoded.trim_start_matches('/').split_once('/') else {
        return not_found();
    };

    // 宿主提供的前端 SDK。插件 id 必含 "."，不会与 "_sdk" 冲突。
    if id == "_sdk" {
        return match file {
            "ui.js" => reply(
                200,
                "text/javascript; charset=utf-8",
                SDK_JS.as_bytes().to_vec(),
                None,
            ),
            "ui.css" => reply(
                200,
                "text/css; charset=utf-8",
                SDK_CSS.as_bytes().to_vec(),
                None,
            ),
            _ => not_found(),
        };
    }

    let Some(dir) = manager.plugin_dir(id) else {
        return not_found();
    };

    let rel = Path::new(file);
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return forbidden();
    }
    let (Ok(root), Ok(full)) = (dir.canonicalize(), dir.join(rel).canonicalize()) else {
        return not_found();
    };
    if !full.starts_with(&root) {
        return forbidden(); // 符号链接逃逸
    }
    let Ok(bytes) = std::fs::read(&full) else {
        return not_found();
    };

    let mime = mime_for(&full);
    let csp = mime
        .starts_with("text/html")
        .then(|| plugin_csp(manager.has_permission(id, Permission::Network)));
    reply(200, mime, bytes, csp)
}

/// 数据根目录有两个，优先级可调（默认 AppData 在前，新下载的插件、运行时都写到靠前且可写的根）：
///   appdata —— 应用数据目录
///   program —— 程序所在目录（开发模式下是仓库根目录）；程序是免安装的，一般只有程序本身，这里通常只读取
/// 每个根下都有 nutPlugins / nutRuntimes / nutConfig 三个子目录。
fn data_roots(app: &tauri::App) -> tauri::Result<Vec<DataRoot>> {
    let appdata = app.path().app_data_dir()?;
    let program: Vec<PathBuf> = if cfg!(debug_assertions) {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        vec![repo]
    } else {
        let mut v: Vec<PathBuf> = Vec::new();
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        {
            v.push(dir);
        }
        // 安装包的资源目录（macOS / Linux 上与可执行文件所在目录不同）
        if let Ok(res) = app.path().resource_dir() {
            if !v.contains(&res) {
                v.push(res);
            }
        }
        v
    };
    Ok(vec![
        DataRoot {
            key: "appdata".into(),
            label: "AppData".into(),
            bases: vec![appdata],
        },
        DataRoot {
            key: "program".into(),
            label: "程序目录".into(),
            bases: program,
        },
    ])
}

fn toggle_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let visible = win.is_visible().unwrap_or(true);
        let minimized = win.is_minimized().unwrap_or(false);
        if visible && !minimized {
            let _ = win.hide();
        } else {
            let _ = win.show();
            let _ = win.unminimize();
            let _ = win.set_focus();
        }
    }
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "toggle", "显示 / 隐藏主窗口", true, None::<&str>)?;
    let stop_item = MenuItem::with_id(
        app,
        "stop_backends",
        "结束所有后台插件进程",
        true,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(app, "quit", "退出 NutHusk", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &stop_item, &quit_item])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("NutHusk")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => toggle_main_window(app),
            "stop_backends" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let stopped = state.manager.stop_all().await;
                    if stopped > 0 {
                        let _ = app.emit("backends-changed", stopped);
                    }
                });
            }
            "quit" => {
                let state = app.state::<AppState>();
                state.quitting.store(true, Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let _ = builder.build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol("plugin", |ctx, request| {
            let state = ctx.app_handle().state::<AppState>();
            serve_plugin(&state.manager, &request)
        })
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            // 设置和插件启用状态属于"状态"，固定放在 AppData 的 nutConfig 里
            let state_dir = data.join(CONFIG_DIR);
            let settings = Arc::new(Settings::load(state_dir.join("settings.json")));
            let keys = nuthusk_core::load_trusted_keys(TRUSTED_KEYS_JSON).unwrap_or_else(|e| {
                eprintln!("trusted_keys.json 无效，所有插件都将显示为未经安全验证: {e}");
                Vec::new()
            });
            // 优先级单独存放（不放进配置目录，避免配置位置取决于优先级的循环）
            let roots = Arc::new(Roots::new(data_roots(app)?, data.join("root-order.json")));
            let runtimes = Arc::new(RuntimeManager::new(roots.clone()));
            let market = Arc::new(Market::new(roots.clone(), keys.clone()));
            let manager = Arc::new(
                PluginManager::new(roots.plugin_groups(), state_dir.join("plugin-state.json"))
                    .with_trusted_keys(keys)
                    .with_runtimes(runtimes.clone()),
            );
            manager.scan();

            if let Some(true) = settings.get().get("alwaysOnTop").and_then(Value::as_bool) {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_always_on_top(true);
                }
            }

            // 后台定时回收闲置超时的插件后端进程
            {
                let manager = manager.clone();
                let settings = settings.clone();
                let handle = app.handle().clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_secs(15));
                    let mins = settings
                        .get()
                        .get("idleTimeoutMinutes")
                        .and_then(Value::as_u64)
                        .unwrap_or(5);
                    if mins > 0 {
                        let stopped = tauri::async_runtime::block_on(
                            manager.reap_idle(Duration::from_secs(mins * 60)),
                        );
                        if !stopped.is_empty() {
                            let _ = handle.emit("backends-changed", stopped.len());
                        }
                    }
                });
            }

            let _ = setup_tray(app);

            let drop_dir = std::env::temp_dir().join("nuthusk-drop");
            package::clean_temp_dir(&drop_dir); // 清掉上次遗留的临时文件
            app.manage(AppState {
                manager,
                settings,
                roots,
                runtimes,
                market,
                drop_dir,
                jobs: Mutex::new(HashMap::new()),
                quitting: AtomicBool::new(false),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                let close_to_tray = state
                    .settings
                    .get()
                    .get("closeToTray")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if close_to_tray && !state.quitting.load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_plugins,
            set_plugin_enabled,
            plugin_call,
            stop_plugin_backend,
            stop_all_backends,
            set_always_on_top,
            inspect_plugin_package,
            install_plugin_package,
            uninstall_plugin,
            plugin_logs,
            reload_plugins,
            reload_single_plugin,
            get_settings,
            patch_settings,
            plugin_dirs,
            set_dir_order,
            trusted_signers,
            export_profile,
            read_profile_file,
            save_dropped_package,
            discard_dropped_package,
            runtime_status,
            runtime_install,
            runtime_remove,
            download_cancel,
            market_sources,
            market_index,
            source_add,
            source_remove,
            market_download
        ])
        .build(tauri::generate_context!())
        .expect("failed to build app")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                let _ = tauri::async_runtime::block_on(state.manager.stop_all());
                package::clean_temp_dir(&state.drop_dir);
            }
        });
}
