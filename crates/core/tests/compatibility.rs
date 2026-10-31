mod common;
use common::*;
use nuthusk_core::{command_on_path, TrustLevel};
use serde_json::json;

#[tokio::test]
async fn packages_with_icons_and_future_api_are_handled() {
    let root = tmp("compat-packages");
    let m = manager(&root);
    let ui = root.join("ui.nut");
    make_zip(
        &ui,
        &[
            (
                "plugin.json",
                &manifest("com.test.ui", "1.0.0", r#","icon":"icon.svg""#),
            ),
            ("ui.html", "<html></html>"),
            ("icon.svg", ICON_SVG),
        ],
    );
    let info = m.inspect_package(&ui).await.unwrap();
    assert_eq!(info.manifest.id, "com.test.ui");
    assert!(info.compat_error.is_none() && info.backend_command.is_none());
    assert!(info
        .icon_data
        .as_deref()
        .unwrap()
        .starts_with("data:image/svg+xml;base64,"));
    assert_eq!(info.trust.level, TrustLevel::Unverified);

    let backend = root.join("backend.nut");
    backend_pkg(&backend, "com.test.backend", "1.1.0");
    let info = m.inspect_package(&backend).await.unwrap();
    assert_eq!(
        (info.manifest.id.as_str(), info.manifest.version.as_str()),
        ("com.test.backend", "1.1.0")
    );
    assert_eq!(info.backend_command.as_deref(), Some("node main.js"));

    let future = root.join("future.nut");
    let future_manifest = manifest("com.test.future", "1.0.0", "").replace("^1.0.0", "^2.0.0");
    make_zip(
        &future,
        &[
            ("plugin.json", &future_manifest),
            ("ui.html", "<html></html>"),
        ],
    );
    let future_info = m.inspect_package(&future).await.unwrap();
    assert!(future_info.compat_error.is_some());
    assert!(m
        .install_package(&future, &future_info.sha256)
        .await
        .is_err());

    m.install_package(&backend, &info.sha256).await.unwrap();
    let out = m
        .invoke("com.test.backend", "echo", json!({ "text": "abc" }))
        .await
        .unwrap();
    assert_eq!(out, "1.1.0");
    m.stop_all().await;
}

#[tokio::test]
async fn python_backend_runs_with_per_os_command() {
    let cmd = if cfg!(windows) { "python" } else { "python3" };
    if !command_on_path(cmd) {
        eprintln!("跳过：本机没有 {cmd}");
        return;
    }
    let root = tmp("compat-python");
    let m = manager(&root);
    let pkg = root.join("python.nut");
    make_zip(
        &pkg,
        &[
            (
                "plugin.json",
                &manifest(
                    "com.test.python",
                    "1.0.0",
                    r#","backend":{"command":"python3","args":["main.py"],"windows":{"command":"python","args":["main.py"]}}"#,
                ),
            ),
            ("ui.html", "<html></html>"),
            (
                "main.py",
                r#"import json, sys
for line in sys.stdin:
    m = json.loads(line)
    result = {"result": m["params"]["text"]} if m["method"] == "echo" else {"error": {"code": -32601, "message": "unknown method"}}
    print(json.dumps({"jsonrpc": "2.0", "id": m["id"], **result}), flush=True)
"#,
            ),
        ],
    );
    let info = m.inspect_package(&pkg).await.unwrap();
    assert_eq!(
        info.backend_command.as_deref(),
        Some(format!("{cmd} main.py").as_str())
    );
    assert_eq!(info.backend_found, Some(true));
    m.install_package(&pkg, &info.sha256).await.unwrap();
    let result = m
        .invoke("com.test.python", "echo", json!({ "text": "hello" }))
        .await
        .unwrap();
    assert_eq!(result, "hello");
    let err = m
        .invoke("com.test.python", "nope", json!({}))
        .await
        .unwrap_err();
    assert!(err.contains("unknown method"), "{err}");
    m.stop_all().await;
}
