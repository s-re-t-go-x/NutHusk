mod common;
use common::*;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn plugins_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../nutPlugins")
}

#[test]
fn scans_shipped_example_plugins_with_icons() {
    let root = tmp("shipped");
    let m = manager(&root);
    // 把仓库里的示例插件当作"程序目录"
    let groups = vec![nuthusk_core::PluginDir { key: "program".into(), label: "程序目录".into(), paths: vec![plugins_dir()] }];
    let m2 = nuthusk_core::PluginManager::new(groups, root.join("s.json"));
    m2.scan();
    drop(m);
    let list = m2.list();
    let json = list.iter().find(|p| p.id == "com.example.json-formatter").unwrap();
    assert!(json.error.is_none() && json.icon.as_deref() == Some("icon.svg"));
    let hash = list.iter().find(|p| p.id == "com.example.hash-calc").unwrap();
    assert!(hash.error.is_none() && hash.has_backend);
    assert_eq!(json.trust.level, nuthusk_core::TrustLevel::Unverified);
}

#[tokio::test]
async fn backend_roundtrip_over_stdio_and_disable() {
    let groups = vec![nuthusk_core::PluginDir { key: "program".into(), label: "程序目录".into(), paths: vec![plugins_dir()] }];
    let root = tmp("roundtrip");
    let m = nuthusk_core::PluginManager::new(groups, root.join("s.json"));
    m.scan();

    let out = m.invoke("com.example.hash-calc", "hash", json!({ "text": "abc", "algo": "sha256" })).await.unwrap();
    assert_eq!(out, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert!(m.invoke("com.example.hash-calc", "nope", json!({})).await.unwrap_err().contains("未知方法"));

    m.set_enabled("com.example.hash-calc", false).await.unwrap();
    assert_eq!(m.invoke("com.example.hash-calc", "hash", json!({})).await.unwrap_err(), "插件已禁用");
    assert!(m.plugin_dir("com.example.hash-calc").is_none());
    m.set_enabled("com.example.hash-calc", true).await.unwrap();
    assert!(m.plugin_dir("com.example.hash-calc").is_some());
    assert_eq!(m.invoke("com.example.json-formatter", "x", json!({})).await.unwrap_err(), "该插件没有后端");
    m.stop_all().await;
}

#[test]
fn bad_manifest_and_incompatible_api_are_reported() {
    let root = tmp("bad");
    let g = groups(&root);
    let dir = &g[0].paths[0];
    fs::create_dir_all(dir.join("broken")).unwrap();
    fs::write(dir.join("broken/plugin.json"), r#"{"id":"Bad ID","name":"x","version":"1.0.0","apiVersion":"^1.0.0"}"#).unwrap();
    fs::create_dir_all(dir.join("future")).unwrap();
    fs::write(dir.join("future/plugin.json"), r#"{"id":"com.example.future","name":"Future","version":"2.0.0","apiVersion":"^2.0.0"}"#).unwrap();
    fs::create_dir_all(dir.join("badicon")).unwrap();
    fs::write(dir.join("badicon/plugin.json"), r#"{"id":"com.example.badicon","name":"B","version":"1.0.0","apiVersion":"^1.0.0","icon":"braces"}"#).unwrap();

    let m = manager(&root);
    let list = m.list();
    assert!(list.iter().find(|p| p.id == "invalid:broken").unwrap().error.as_deref().unwrap().contains("manifest 无效"));
    assert!(list.iter().find(|p| p.id == "com.example.future").unwrap().error.as_deref().unwrap().contains("需要宿主 API"));
    let bad = list.iter().find(|p| p.id == "invalid:badicon").unwrap();
    assert!(bad.error.as_deref().unwrap().contains("icon 必须是插件目录内的图片文件"), "{:?}", bad.error);
}

#[test]
fn same_id_in_two_directories_priority_decides_and_is_changeable() {
    let root = tmp("priority");
    let g = groups(&root);
    for (dir, version) in [(&g[0].paths[0], "1.0.0"), (&g[1].paths[0], "2.0.0")] {
        fs::create_dir_all(dir.join("x")).unwrap();
        fs::write(dir.join("x/plugin.json"), manifest("com.test.same", version, "")).unwrap();
    }
    let m = manager(&root);

    let p = m.list().into_iter().find(|p| p.id == "com.test.same").unwrap();
    assert_eq!((p.version.as_str(), p.group.as_str()), ("1.0.0", "appdata"), "默认 AppData 优先");
    assert_eq!(p.shadowed.len(), 1);
    assert_eq!(m.list().len(), 1, "同 id 只显示一份");

    m.set_order(&["program".to_string(), "appdata".to_string()]).unwrap();
    let p = m.list().into_iter().find(|p| p.id == "com.test.same").unwrap();
    assert_eq!((p.version.as_str(), p.group.as_str()), ("2.0.0", "program"));
    assert_eq!(m.dirs_info()[0].key, "program");

    assert!(m.set_order(&["nope".to_string()]).unwrap_err().contains("未知的插件目录"));
    // 没提到的目录组保持原顺序排在后面
    m.set_order(&["appdata".to_string()]).unwrap();
    assert_eq!(m.dirs_info().iter().map(|d| d.key.as_str()).collect::<Vec<_>>(), ["appdata", "program"]);
}

#[test]
fn dirs_info_reports_counts_and_writability() {
    let root = tmp("dirsinfo");
    let g = groups(&root);
    fs::create_dir_all(g[0].paths[0].join("a")).unwrap();
    fs::write(g[0].paths[0].join("a/plugin.json"), manifest("com.test.a", "1.0.0", "")).unwrap();
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
    fs::write(g[0].paths[0].join(".installing-com.test.x/plugin.json"), manifest("com.test.x", "1.0.0", "")).unwrap();
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
    assert_eq!(all["dirOrder"], json!(["appdata", "program"]));

    s.patch(&json!({ "theme": "dark", "showFavorites": false, "future.feature": { "a": 1 } })).unwrap();
    // 重新加载后仍在，且未知键被保留
    let s2 = nuthusk_core::Settings::load(file.clone());
    assert_eq!(s2.get()["theme"], "dark");
    assert_eq!(s2.get()["showFavorites"], false);
    assert_eq!(s2.get()["future.feature"], json!({ "a": 1 }));
    assert_eq!(s2.get()["showRecent"], true, "没改的保持默认");

    // 有一项不合法，整体都不生效
    assert!(s2.patch(&json!({ "theme": "light", "showRecent": "yes" })).is_err());
    assert_eq!(s2.get()["theme"], "dark");
    assert!(s2.patch(&json!({ "theme": "purple" })).is_err());
    assert!(s2.patch(&json!({ "dirOrder": [] })).is_err());
    assert!(s2.patch(&json!([1, 2])).is_err());
    s2.patch(&json!({ "dirOrder": ["program", "appdata"] })).unwrap();
    assert_eq!(s2.dir_order(), ["program", "appdata"]);

    // 损坏的设置文件不会让程序崩溃
    fs::write(&file, "{ not json").unwrap();
    assert_eq!(nuthusk_core::Settings::load(file).get()["theme"], "system");
}
