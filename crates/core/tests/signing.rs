mod common;
use common::*;
use nuthusk_core::signature;
use nuthusk_core::TrustLevel;
use std::fs;
use std::path::Path;

fn plain_pkg(path: &Path, version: &str, ui: &str) {
    make_zip(path, &[("plugin.json", &manifest("com.test.sig", version, "")), ("ui.html", ui), ("data/readme.txt", "hi")]);
}

#[tokio::test]
async fn signed_by_trusted_key_is_trusted() {
    let root = tmp("sig-ok");
    let m = manager_with_keys(&root, trusted(&test_seed(1), "NutHusk 官方"));
    let (a, b) = (root.join("a.nut"), root.join("b.nut"));
    plain_pkg(&a, "1.0.0", "<p>hi</p>");
    sign_package(&a, &b, &test_seed(1));

    let info = m.inspect_package(&b).await.unwrap();
    assert_eq!(info.trust.level, TrustLevel::Trusted, "{:?}", info.trust);
    assert_eq!(info.trust.signer.as_deref(), Some("NutHusk 官方"));
    assert_eq!(info.trust.key_id.as_deref().unwrap().len(), 16);
    // 未签名的同一份内容是"未经安全验证"
    assert_eq!(m.inspect_package(&a).await.unwrap().trust.level, TrustLevel::Unverified);
}

#[tokio::test]
async fn unknown_signer_is_unverified_not_trusted() {
    let root = tmp("sig-unknown");
    let m = manager_with_keys(&root, trusted(&test_seed(1), "官方"));
    let (a, b) = (root.join("a.nut"), root.join("b.nut"));
    plain_pkg(&a, "1.0.0", "x");
    sign_package(&a, &b, &test_seed(2)); // 另一个人的密钥
    let t = m.inspect_package(&b).await.unwrap().trust;
    assert_eq!(t.level, TrustLevel::Unverified);
    assert!(t.reason.contains("不在受信任列表"), "{}", t.reason);
    // 没有配置任何受信任密钥时，同样不会显示可信
    let m2 = manager(&root);
    assert_eq!(m2.inspect_package(&b).await.unwrap().trust.level, TrustLevel::Unverified);
}

#[tokio::test]
async fn tampered_content_makes_signature_invalid_and_blocks_install() {
    let root = tmp("sig-tamper");
    let m = manager_with_keys(&root, trusted(&test_seed(1), "官方"));
    let (a, signed, evil) = (root.join("a.nut"), root.join("signed.nut"), root.join("evil.nut"));
    plain_pkg(&a, "1.0.0", "<p>good</p>");
    sign_package(&a, &signed, &test_seed(1));

    // 取出合法的签名，贴到被改动过内容的包上
    let sig = {
        let mut ar = zip::ZipArchive::new(fs::File::open(&signed).unwrap()).unwrap();
        let mut s = String::new();
        std::io::Read::read_to_string(&mut ar.by_name("signature.json").unwrap(), &mut s).unwrap();
        s
    };
    make_zip(&evil, &[
        ("plugin.json", &manifest("com.test.sig", "1.0.0", "")),
        ("ui.html", "<script>steal()</script>"),
        ("data/readme.txt", "hi"),
        ("signature.json", &sig),
    ]);
    let info = m.inspect_package(&evil).await.unwrap();
    assert_eq!(info.trust.level, TrustLevel::Invalid);
    let err = m.install_package(&evil, &info.sha256).await.unwrap_err();
    assert!(err.contains("签名校验失败"), "{err}");
    assert!(m.list().is_empty() && fs::read_dir(root.join("appdata")).unwrap().next().is_none());
}

#[test]
fn signature_is_bound_to_plugin_id_and_version() {
    // 摘要完全相同（同一份内容），只换 id 或版本号，也必须校验失败：防止把别的插件的签名套过来
    let keys = trusted(&test_seed(1), "官方");
    let sig = signature::sign(&test_seed(1), "com.test.a", "1.0.0", "digest-1");
    let text = serde_json::to_string(&sig).unwrap();
    let ok = |id: &str, ver: &str, digest: &str| {
        let d = digest.to_string();
        signature::verify(Some(&text), id, ver, move || Ok(d), &keys).level
    };
    assert_eq!(ok("com.test.a", "1.0.0", "digest-1"), TrustLevel::Trusted);
    assert_eq!(ok("com.test.b", "1.0.0", "digest-1"), TrustLevel::Invalid, "换 id");
    assert_eq!(ok("com.test.a", "1.0.1", "digest-1"), TrustLevel::Invalid, "换版本");
    assert_eq!(ok("com.test.a", "1.0.0", "digest-2"), TrustLevel::Invalid, "换内容");
}

#[tokio::test]
async fn digest_ignores_zip_layout_and_resigning_replaces_old_signature() {
    let root = tmp("sig-layout");
    let (flat, folder) = (root.join("flat.nut"), root.join("folder.nut"));
    plain_pkg(&flat, "1.0.0", "x");
    make_zip(&folder, &[
        ("pkg/plugin.json", &manifest("com.test.sig", "1.0.0", "")),
        ("pkg/ui.html", "x"),
        ("pkg/data/readme.txt", "hi"),
    ]);
    let d1 = nuthusk_core::package::digest_of(&flat).unwrap().1;
    let (meta, d2) = nuthusk_core::package::digest_of(&folder).unwrap();
    assert_eq!(d1, d2, "顶层文件夹不影响摘要");
    assert_eq!(meta.prefix, "pkg");

    // 带顶层文件夹的包签名后，signature.json 放在文件夹里，且签两次只留一份
    let (s1, s2) = (root.join("s1.nut"), root.join("s2.nut"));
    sign_package(&folder, &s1, &test_seed(1));
    sign_package(&s1, &s2, &test_seed(1));
    let ar = zip::ZipArchive::new(fs::File::open(&s2).unwrap()).unwrap();
    let sigs: Vec<_> = ar.file_names().filter(|n| n.ends_with("signature.json")).collect();
    assert_eq!(sigs, ["pkg/signature.json"]);
    let m = manager_with_keys(&root, trusted(&test_seed(1), "官方"));
    assert_eq!(m.inspect_package(&s2).await.unwrap().trust.level, TrustLevel::Trusted);
}

#[tokio::test]
async fn installed_plugin_is_trusted_and_tampering_after_install_is_detected() {
    let root = tmp("sig-installed");
    let m = manager_with_keys(&root, trusted(&test_seed(1), "官方"));
    let (a, s) = (root.join("a.nut"), root.join("s.nut"));
    plain_pkg(&a, "1.0.0", "<p>good</p>");
    sign_package(&a, &s, &test_seed(1));
    let sha = m.inspect_package(&s).await.unwrap().sha256;
    m.install_package(&s, &sha).await.unwrap();

    let p = m.list().into_iter().find(|p| p.id == "com.test.sig").unwrap();
    assert_eq!(p.trust.level, TrustLevel::Trusted, "{:?}", p.trust);
    assert_eq!(p.trust.signer.as_deref(), Some("官方"));

    // 安装之后有人改了文件 → 重新扫描时变为"签名无效"
    fs::write(root.join("appdata/com.test.sig/ui.html"), "<script>evil()</script>").unwrap();
    m.scan();
    assert_eq!(m.list()[0].trust.level, TrustLevel::Invalid);

    // 恢复后重新变回可信；往目录里多放一个文件，或放进符号链接，都会被发现
    fs::write(root.join("appdata/com.test.sig/ui.html"), "<p>good</p>").unwrap();
    m.scan();
    assert_eq!(m.list()[0].trust.level, TrustLevel::Trusted);
    fs::write(root.join("appdata/com.test.sig/extra.js"), "x").unwrap();
    m.scan();
    assert_eq!(m.list()[0].trust.level, TrustLevel::Invalid);
    fs::remove_file(root.join("appdata/com.test.sig/extra.js")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/passwd", root.join("appdata/com.test.sig/link")).unwrap();
        m.scan();
        let t = &m.list()[0].trust;
        assert_eq!(t.level, TrustLevel::Invalid, "{t:?}");
    }
}

#[test]
fn key_formats_and_trusted_key_list_parsing() {
    let seed = test_seed(5);
    let public = signature::public_key_of_seed(&seed);
    assert_eq!(signature::parse_private_key(&signature::format_private_key(&seed)).unwrap(), seed);
    assert_eq!(signature::parse_public_key(&signature::format_public_key(&public)).unwrap(), public);
    assert!(signature::parse_public_key("bogus").is_err());
    assert!(signature::parse_private_key(&signature::format_public_key(&public)).is_err(), "公钥不能当私钥用");

    assert!(signature::load_trusted_keys(r#"{"keys":[]}"#).unwrap().is_empty());
    assert!(signature::load_trusted_keys("{}").unwrap().is_empty(), "空列表是允许的：所有插件都显示未经验证");
    assert!(signature::load_trusted_keys(r#"{"keys":[{"name":"x","public":"nuthusk-ed25519-public:AAAA"}]}"#).is_err());
    assert!(signature::load_trusted_keys("not json").is_err());
}

#[test]
fn garbage_signature_files_are_invalid_not_a_crash() {
    let digest = || Ok("d".to_string());
    let keys = trusted(&test_seed(1), "k");
    for bad in ["not json", r#"{"version":2,"algorithm":"ed25519","keyId":"x","signature":"y"}"#, r#"{"version":1,"algorithm":"rsa","keyId":"x","signature":"y"}"#] {
        let t = signature::verify(Some(bad), "a.b", "1.0.0", digest, &keys);
        assert_eq!(t.level, TrustLevel::Invalid, "{bad}");
    }
    let id = signature::key_id_of(&signature::public_key_of_seed(&test_seed(1)));
    let wrong_sig = format!(r#"{{"version":1,"algorithm":"ed25519","keyId":"{id}","signature":"AAAA"}}"#);
    assert_eq!(signature::verify(Some(&wrong_sig), "a.b", "1.0.0", digest, &keys).level, TrustLevel::Invalid);
}
