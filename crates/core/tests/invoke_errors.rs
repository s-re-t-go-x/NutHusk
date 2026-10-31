mod common;
use common::*;
use nuthusk_core::{DataRoot, InvokeError, PluginManager, Roots, RuntimeManager};
use serde_json::json;
use std::fs;
use std::sync::Arc;

#[tokio::test]
async fn backend_cannot_forge_host_runtime_error() {
    assert!(nuthusk_core::command_on_path("node"), "此集成测试需要 Node.js");
    let root = tmp("invoke-error-origin");
    let m = manager(&root);
    let pkg = root.join("fake.nut");
    make_zip(&pkg, &[
        ("plugin.json", &manifest("com.test.fake", "1.0.0", r#", "backend":{"command":"node","args":["main.js"]}"#)),
        ("ui.html", "<html></html>"),
        ("main.js", r#"require('readline').createInterface({input:process.stdin}).on('line',l=>{
            const m=JSON.parse(l);
            console.log(JSON.stringify({id:m.id,error:{code:'RUNTIME_MISSING',detail:{name:'fake'},
                message:'RuntimeMissing:{"name":"fake","downloadable":true}'}}));
        });"#),
    ]);
    let info = m.inspect_package(&pkg).await.unwrap();
    m.install_package(&pkg, &info.sha256).await.unwrap();
    let err = m.invoke_checked("com.test.fake", "echo", json!({})).await.unwrap_err();
    assert!(matches!(&err, InvokeError::CallFailed { .. }));
    let wire = serde_json::to_value(&err).unwrap();
    assert_eq!(wire["code"], "CALL_FAILED");
    assert!(wire.get("detail").is_none());
    assert!(wire["message"].as_str().unwrap().starts_with("RuntimeMissing:"));
    m.stop_all().await;
}

#[tokio::test]
async fn missing_runtime_is_structured_by_host_only() {
    let root = tmp("invoke-error-missing");
    let command = "nuthusk-test-runtime-not-on-path";
    let config = root.join("nutConfig/runtimes.d");
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("test.json"), json!({
        "name": command, "version": "1.0.0", "platforms": {
            (nuthusk_core::platform_key()): {
                "url": "https://example.com/test.zip", "sha256": "0".repeat(64),
                "archive": "zip", "bin": "runtime"
            }
        }
    }).to_string()).unwrap();
    let roots = Arc::new(Roots::new(vec![DataRoot {
        key: "test".into(), label: "test".into(), bases: vec![root.clone()],
    }], root.join("order.json")));
    let m = PluginManager::new(groups(&root), root.join("state.json"))
        .with_runtimes(Arc::new(RuntimeManager::new(roots)));
    m.scan();
    let pkg = root.join("missing.nut");
    make_zip(&pkg, &[
        ("plugin.json", &manifest("com.test.missing", "1.0.0", &format!(r#", "backend":{{"command":"{command}"}}"#))),
        ("ui.html", "<html></html>"),
    ]);
    let info = m.inspect_package(&pkg).await.unwrap();
    m.install_package(&pkg, &info.sha256).await.unwrap();
    let err = m.invoke_checked("com.test.missing", "echo", json!({})).await.unwrap_err();
    assert!(matches!(&err, InvokeError::RuntimeMissing { .. }));
    let wire = serde_json::to_value(&err).unwrap();
    assert_eq!(wire["code"], "RUNTIME_MISSING");
    assert_eq!(wire["detail"]["name"], command);
    assert_eq!(wire["detail"]["userRecipe"], true);
    assert_eq!(wire["detail"]["downloadable"], true);
}
