mod common;
use common::*;
use nuthusk_core::{command_on_path, TrustLevel};
use serde_json::json;
use std::path::PathBuf;

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name)
}

#[tokio::test]
async fn shipped_example_packages_are_valid() {
    let root = tmp("examples");
    let m = manager(&root);

    let uuid = m.inspect_package(&example("uuid-gen-1.0.0.nut")).await.unwrap();
    assert_eq!(uuid.manifest.id, "com.example.uuid-gen");
    assert!(uuid.compat_error.is_none() && uuid.backend_command.is_none());
    assert!(uuid.icon_data.as_deref().unwrap().starts_with("data:image/svg+xml;base64,"));
    assert_eq!(uuid.trust.level, TrustLevel::Unverified, "示例包没有签名");

    let hash = m.inspect_package(&example("hash-calc-1.1.0.nut")).await.unwrap();
    assert_eq!((hash.manifest.id.as_str(), hash.manifest.version.as_str()), ("com.example.hash-calc", "1.1.0"));
    assert_eq!(hash.backend_command.as_deref(), Some("node main.js"));

    let future = m.inspect_package(&example("future-api-needs-api-2.nut")).await.unwrap();
    assert!(future.compat_error.is_some());
    assert!(m.install_package(&example("future-api-needs-api-2.nut"), &future.sha256).await.is_err());

    // 示例包能真正装上并运行（需要本机有 node）
    m.install_package(&example("hash-calc-1.1.0.nut"), &hash.sha256).await.unwrap();
    let out = m
        .invoke("com.example.hash-calc", "hash", json!({ "text": "abc", "algo": "sha256" }))
        .await
        .unwrap();
    assert_eq!(out, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    m.stop_all().await;
}

#[tokio::test]
async fn python_example_runs_with_per_os_command() {
    let (cmd, args) = if cfg!(windows) { ("python", "main.py") } else { ("python3", "main.py") };
    if !command_on_path(cmd) {
        eprintln!("跳过：本机没有 {cmd}");
        return;
    }
    let root = tmp("py");
    let m = manager(&root);
    let info = m.inspect_package(&example("py-wordcount-1.0.0.nut")).await.unwrap();
    assert_eq!(info.backend_command.as_deref(), Some(format!("{cmd} {args}").as_str()), "按当前系统选择命令");
    assert_eq!(info.backend_found, Some(true));
    m.install_package(&example("py-wordcount-1.0.0.nut"), &info.sha256).await.unwrap();

    let r = m.invoke("com.example.py-wordcount", "count", json!({ "text": "你好 world\nline2" })).await.unwrap();
    assert_eq!(r, json!({ "chars": 14, "words": 3, "lines": 2, "bytes": 18 }));
    let err = m.invoke("com.example.py-wordcount", "nope", json!({})).await.unwrap_err();
    assert!(err.contains("未知方法"), "{err}");
    m.stop_all().await;
}
