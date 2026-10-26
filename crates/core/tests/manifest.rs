use nuthusk_core::{BackendSpec, Manifest};

fn parse(json: &str) -> Manifest {
    serde_json::from_str(json).unwrap()
}

const BASE: &str = r#""id":"com.test.m","name":"M","version":"1.0.0","apiVersion":"^1.0.0""#;

#[test]
fn backend_resolves_per_operating_system() {
    let m = parse(&format!(
        r#"{{{BASE},"backend":{{"command":"python3","args":["main.py"],"windows":{{"command":"python","args":["main.py"]}},"macos":{{"command":"./bin/mac"}}}}}}"#
    ));
    m.validate().unwrap();
    let b = m.backend.unwrap();
    assert_eq!(b.for_os("windows").unwrap().command, "python");
    assert_eq!(b.for_os("macos").unwrap().command, "./bin/mac");
    assert_eq!(b.for_os("linux").unwrap().command, "python3", "没有专用配置时回退到通用的 command");
    assert_eq!(b.for_os("linux").unwrap().args, ["main.py"]);
}

#[test]
fn plugin_without_backend_for_this_os_is_reported_incompatible() {
    let other = if std::env::consts::OS == "windows" { "linux" } else { "windows" };
    let m = parse(&format!(r#"{{{BASE},"backend":{{"{other}":{{"command":"./x"}}}}}}"#));
    m.validate().unwrap();
    let err = m.check_compatible().unwrap_err();
    assert!(err.contains("没有适用于当前系统"), "{err}");
}

#[test]
fn backend_validation() {
    let empty = parse(&format!(r#"{{{BASE},"backend":{{}}}}"#));
    assert!(empty.validate().unwrap_err().contains("backend 至少要有"));
    let unsafe_variant = parse(&format!(r#"{{{BASE},"backend":{{"command":"node","windows":{{"command":"./../evil.exe"}}}}}}"#));
    assert!(unsafe_variant.validate().unwrap_err().contains("路径不安全"));
    let blank = parse(&format!(r#"{{{BASE},"backend":{{"command":"  "}}}}"#));
    assert!(blank.validate().is_err());
    let _: BackendSpec = serde_json::from_str(r#"{"command":"x"}"#).unwrap();
}

#[test]
fn icon_validation() {
    for good in ["icon.png", "img/logo.SVG", "a.webp", "i.ico", "x.jpeg", "g.gif"] {
        parse(&format!(r#"{{{BASE},"icon":"{good}"}}"#)).validate().unwrap_or_else(|e| panic!("{good}: {e}"));
    }
    for bad in ["braces", "icon.txt", "../icon.png", "/abs/icon.png", "icon"] {
        assert!(parse(&format!(r#"{{{BASE},"icon":"{bad}"}}"#)).validate().is_err(), "{bad} 应被拒绝");
    }
    assert!(parse(&format!("{{{BASE}}}")).validate().is_ok(), "icon 是可选的");
}
