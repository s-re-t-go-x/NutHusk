mod common;
use common::*;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// 宿主测试使用临时最小插件，不依赖插件仓库的具体工具实现。
fn plugins_dir(root: &std::path::Path) -> PathBuf {
    let base = root.join("fixtures");
    for (name, id, extra) in [
        ("frontend", "com.test.frontend", r#","icon":"icon.svg""#),
        (
            "backend",
            "com.test.backend",
            r#","icon":"icon.svg","backend":{"command":"node","args":["main.js"]}"#,
        ),
    ] {
        let dir = base.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("plugin.json"), manifest(id, "1.0.0", extra)).unwrap();
        fs::write(dir.join("ui.html"), "<html></html>").unwrap();
        fs::write(dir.join("icon.svg"), ICON_SVG).unwrap();
    }
    fs::write(
        base.join("backend/main.js"),
        r#"
const rl = require('readline').createInterface({ input: process.stdin });
rl.on('line', line => {
    const m = JSON.parse(line);
    const response = m.method === 'echo'
        ? { result: m.params.text }
        : { error: { code: -32601, message: '未知方法' } };
    console.log(JSON.stringify({ jsonrpc: '2.0', id: m.id, ...response }));
});
"#,
    )
    .unwrap();
    base
}

#[test]
fn scans_plugins_with_icons_and_backend_metadata() {
    let root = tmp("shipped");
    let m = manager(&root);
    // 将测试夹具目录作为程序目录。
    let groups = vec![nuthusk_core::PluginDir {
        key: "program".into(),
        label: "程序目录".into(),
        paths: vec![plugins_dir(&root)],
    }];
    let m2 = nuthusk_core::PluginManager::new(groups, root.join("s.json"));
    m2.scan();
    drop(m);
    let list = m2.list();
    let frontend = list.iter().find(|p| p.id == "com.test.frontend").unwrap();
    assert!(frontend.error.is_none() && frontend.icon.as_deref() == Some("icon.svg"));
    let backend = list.iter().find(|p| p.id == "com.test.backend").unwrap();
    assert!(
        backend.error.is_none()
            && backend.has_backend
            && !backend.backend_running
            && backend.backend_pid.is_none()
    );
    assert_eq!(frontend.trust.level, nuthusk_core::TrustLevel::Unverified);
}

#[tokio::test]
async fn backend_roundtrip_over_stdio_and_disable() {
    let root = tmp("roundtrip");
    let groups = vec![nuthusk_core::PluginDir {
        key: "program".into(),
        label: "程序目录".into(),
        paths: vec![plugins_dir(&root)],
    }];
    let m = nuthusk_core::PluginManager::new(groups, root.join("s.json"));
    m.scan();

    let out = m
        .invoke("com.test.backend", "echo", json!({ "text": "abc" }))
        .await
        .unwrap();
    assert_eq!(out, "abc");

    // 调用后后端进程处于运行状态，且暴露 PID
    let backend_info = m
        .list()
        .into_iter()
        .find(|p| p.id == "com.test.backend")
        .unwrap();
    assert!(backend_info.backend_running);
    assert!(backend_info.backend_pid.is_some());

    // 闲置回收：阈值大于当前闲置时间时不停止，阈值为 0 时立即回收
    assert!(m.reap_idle(Duration::from_secs(60)).await.is_empty());
    let reaped = m.reap_idle(Duration::ZERO).await;
    assert_eq!(reaped, vec!["com.test.backend".to_string()]);
    let backend_after = m
        .list()
        .into_iter()
        .find(|p| p.id == "com.test.backend")
        .unwrap();
    assert!(!backend_after.backend_running);

    // 再次调用会自动重新拉起
    assert!(m
        .invoke("com.test.backend", "nope", json!({}))
        .await
        .unwrap_err()
        .contains("未知方法"));
    assert!(
        m.list()
            .into_iter()
            .find(|p| p.id == "com.test.backend")
            .unwrap()
            .backend_running
    );

    m.set_enabled("com.test.backend", false).await.unwrap();
    assert!(
        !m.list()
            .into_iter()
            .find(|p| p.id == "com.test.backend")
            .unwrap()
            .backend_running
    );
    assert_eq!(
        m.invoke("com.test.backend", "echo", json!({}))
            .await
            .unwrap_err(),
        "插件已禁用"
    );
    assert!(m.plugin_dir("com.test.backend").is_none());
    m.set_enabled("com.test.backend", true).await.unwrap();
    assert!(m.plugin_dir("com.test.backend").is_some());
    assert_eq!(
        m.invoke("com.test.frontend", "x", json!({}))
            .await
            .unwrap_err(),
        "该插件没有后端"
    );
    m.stop_all().await;
}

#[test]
fn bad_manifest_and_incompatible_api_are_reported() {
    let root = tmp("bad");
    let g = groups(&root);
    let dir = &g[0].paths[0];
    fs::create_dir_all(dir.join("broken")).unwrap();
    fs::write(
        dir.join("broken/plugin.json"),
        r#"{"id":"Bad ID","name":"x","version":"1.0.0","apiVersion":"^1.0.0"}"#,
    )
    .unwrap();
    fs::create_dir_all(dir.join("future")).unwrap();
    fs::write(
        dir.join("future/plugin.json"),
        r#"{"id":"com.example.future","name":"Future","version":"2.0.0","apiVersion":"^2.0.0"}"#,
    )
    .unwrap();
    fs::create_dir_all(dir.join("badicon")).unwrap();
    fs::write(dir.join("badicon/plugin.json"), r#"{"id":"com.example.badicon","name":"B","version":"1.0.0","apiVersion":"^1.0.0","icon":"braces"}"#).unwrap();

    let m = manager(&root);
    let list = m.list();
    assert!(list
        .iter()
        .find(|p| p.id == "invalid:broken")
        .unwrap()
        .error
        .as_deref()
        .unwrap()
        .contains("manifest 无效"));
    assert!(list
        .iter()
        .find(|p| p.id == "com.example.future")
        .unwrap()
        .error
        .as_deref()
        .unwrap()
        .contains("需要宿主 API"));
    let bad = list.iter().find(|p| p.id == "invalid:badicon").unwrap();
    assert!(
        bad.error
            .as_deref()
            .unwrap()
            .contains("icon 必须是插件目录内的图片文件"),
        "{:?}",
        bad.error
    );
}

#[test]
fn same_id_in_two_directories_priority_decides_and_is_changeable() {
    let root = tmp("priority");
    let g = groups(&root);
    for (dir, version) in [(&g[0].paths[0], "1.0.0"), (&g[1].paths[0], "2.0.0")] {
        fs::create_dir_all(dir.join("x")).unwrap();
        fs::write(
            dir.join("x/plugin.json"),
            manifest("com.test.same", version, ""),
        )
        .unwrap();
    }
    let m = manager(&root);

    let p = m
        .list()
        .into_iter()
        .find(|p| p.id == "com.test.same")
        .unwrap();
    assert_eq!(
        (p.version.as_str(), p.group.as_str()),
        ("1.0.0", "appdata"),
        "默认 AppData 优先"
    );
    assert_eq!(p.shadowed.len(), 1);
    assert_eq!(m.list().len(), 1, "同 id 只显示一份");

    m.set_order(&["program".to_string(), "appdata".to_string()])
        .unwrap();
    let p = m
        .list()
        .into_iter()
        .find(|p| p.id == "com.test.same")
        .unwrap();
    assert_eq!((p.version.as_str(), p.group.as_str()), ("2.0.0", "program"));
    assert_eq!(m.dirs_info()[0].key, "program");

    assert!(m
        .set_order(&["nope".to_string()])
        .unwrap_err()
        .contains("未知的插件目录"));
    // 没提到的目录组保持原顺序排在后面
    m.set_order(&["appdata".to_string()]).unwrap();
    assert_eq!(
        m.dirs_info()
            .iter()
            .map(|d| d.key.as_str())
            .collect::<Vec<_>>(),
        ["appdata", "program"]
    );
}

#[test]
fn dirs_info_reports_counts_and_writability() {
    let root = tmp("dirsinfo");
    let g = groups(&root);
    fs::create_dir_all(g[0].paths[0].join("a")).unwrap();
    fs::write(
        g[0].paths[0].join("a/plugin.json"),
        manifest("com.test.a", "1.0.0", ""),
    )
    .unwrap();
    let m = manager(&root);
    let info = m.dirs_info();
    assert_eq!(info[0].plugin_count, 1);
    assert_eq!(info[1].plugin_count, 0);
    assert!(info[0].paths[0].exists && info[0].paths[0].writable);
}

#[test]
fn dot_directories_are_ignored() {
    let root = tmp("dot");
    let g = groups(&root);
    fs::create_dir_all(g[0].paths[0].join(".installing-com.test.x")).unwrap();
    fs::write(
        g[0].paths[0].join(".installing-com.test.x/plugin.json"),
        manifest("com.test.x", "1.0.0", ""),
    )
    .unwrap();
    assert!(manager(&root).list().is_empty());
}

#[test]
fn settings_defaults_patch_persist_and_validate() {
    let root = tmp("settings");
    let file = root.join("settings.json");
    let s = nuthusk_core::Settings::load(file.clone());
    let all = s.get();
    assert_eq!(all["theme"], "system");
    assert_eq!(all["showRecent"], true);
    assert_eq!(all["stopOnClose"], true);
    assert_eq!(all["idleTimeoutMinutes"], 5);
    assert_eq!(all["closeToTray"], false);
    assert_eq!(all["alwaysOnTop"], false);
    assert_eq!(all["dirOrder"], json!(["appdata", "program"]));

    s.patch(&json!({
        "theme": "dark",
        "showFavorites": false,
        "stopOnClose": false,
        "idleTimeoutMinutes": 15,
        "closeToTray": true,
        "alwaysOnTop": true,
        "future.feature": { "a": 1 }
    }))
    .unwrap();
    // 重新加载后仍在，且未知键被保留
    let s2 = nuthusk_core::Settings::load(file.clone());
    assert_eq!(s2.get()["theme"], "dark");
    assert_eq!(s2.get()["showFavorites"], false);
    assert_eq!(s2.get()["stopOnClose"], false);
    assert_eq!(s2.get()["idleTimeoutMinutes"], 15);
    assert_eq!(s2.get()["closeToTray"], true);
    assert_eq!(s2.get()["alwaysOnTop"], true);
    assert_eq!(s2.get()["future.feature"], json!({ "a": 1 }));
    assert_eq!(s2.get()["showRecent"], true, "没改的保持默认");

    // 有一项不合法，整体都不生效
    assert!(s2
        .patch(&json!({ "theme": "light", "showRecent": "yes" }))
        .is_err());
    assert!(s2.patch(&json!({ "idleTimeoutMinutes": 99999 })).is_err());
    assert_eq!(s2.get()["theme"], "dark");
    assert!(s2.patch(&json!({ "theme": "purple" })).is_err());
    assert!(s2.patch(&json!({ "dirOrder": [] })).is_err());
    assert!(s2.patch(&json!([1, 2])).is_err());
    s2.patch(&json!({ "dirOrder": ["program", "appdata"] }))
        .unwrap();
    assert_eq!(s2.dir_order(), ["program", "appdata"]);

    // 损坏的设置文件不会让程序崩溃
    fs::write(&file, "{ not json").unwrap();
    assert_eq!(nuthusk_core::Settings::load(file).get()["theme"], "system");
}

#[test]
fn settings_save_failure_preserves_memory_and_file() {
    let root = tmp("settings-save-failure");
    let file = root.join("settings.json");
    let settings = nuthusk_core::Settings::load(file.clone());
    settings.patch(&json!({ "theme": "dark" })).unwrap();
    // A directory at the target forces replacement to fail, including on Windows.
    let backup = root.join("original.json");
    fs::rename(&file, &backup).unwrap();
    fs::create_dir(&file).unwrap();
    assert!(settings.patch(&json!({ "theme": "light" })).is_err());
    assert_eq!(settings.get()["theme"], "dark");
    assert_eq!(nuthusk_core::Settings::load(backup).get()["theme"], "dark");
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        2,
        "temporary file is removed"
    );
}

#[tokio::test]
async fn backend_not_reading_stdin_times_out_and_stops() {
    let root = tmp("blocked-stdin");
    let g = groups(&root);
    let dir = g[0].paths[0].join("blocked");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("plugin.json"),
        manifest(
            "com.test.blocked",
            "1.0.0",
            r#","backend":{"command":"node","args":["main.js"]}"#,
        ),
    )
    .unwrap();
    fs::write(dir.join("ui.html"), "").unwrap();
    fs::write(dir.join("main.js"), "setInterval(() => {}, 1000);").unwrap();
    let m = manager(&root);
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        m.invoke(
            "com.test.blocked",
            "blocked",
            json!({ "text": "x".repeat(8 * 1024 * 1024) }),
        ),
    )
    .await;
    let running = m.list().iter().any(|p| p.backend_running);
    m.stop_all().await;
    assert!(result
        .expect("stdin write must be covered by the request timeout")
        .unwrap_err()
        .contains("超时"));
    assert!(
        !running,
        "a partial request must not leave the backend running"
    );
}


#[tokio::test]
async fn oversized_backend_stdout_is_bounded_and_process_is_stopped() {
    let root = tmp("oversized-stdout");
    let g = groups(&root);
    let dir = g[0].paths[0].join("large");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("plugin.json"), manifest("com.test.large", "1.0.0", r#","backend":{"command":"node","args":["main.js"]}"#)).unwrap();
    fs::write(dir.join("ui.html"), "").unwrap();
    fs::write(dir.join("main.js"), "process.stdout.write('x'.repeat(17 * 1024 * 1024)); setInterval(() => {}, 1000);").unwrap();
    let m = manager(&root);
    let result = tokio::time::timeout(Duration::from_secs(5), m.invoke("com.test.large", "echo", json!({}))).await;
    let running = m.list().iter().any(|p| p.backend_running);
    m.stop_all().await;
    assert!(result.expect("oversized output must fail before timeout").unwrap_err().contains("上限"));
    assert!(!running);
}
