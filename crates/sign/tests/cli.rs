use nuthusk_core::signature::load_trusted_keys;
use nuthusk_core::TrustLevel;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nuthusk-sign"))
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nuthusk-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn make_zip(path: &Path, ui: &str) {
    use zip::write::SimpleFileOptions;
    let mut w = zip::ZipWriter::new(File::create(path).unwrap());
    w.start_file("plugin.json", SimpleFileOptions::default()).unwrap();
    w.write_all(r#"{"id":"com.test.cli","name":"CLI 测试","version":"1.0.0","apiVersion":"^1.0.0","ui":"ui.html"}"#.as_bytes()).unwrap();
    w.start_file("ui.html", SimpleFileOptions::default()).unwrap();
    w.write_all(ui.as_bytes()).unwrap();
    w.finish().unwrap();
}

fn trusted_keys_json(dir: &Path, pub_file: &Path) -> PathBuf {
    let public = fs::read_to_string(pub_file).unwrap();
    let path = dir.join("trusted_keys.json");
    fs::write(&path, format!(r#"{{"keys":[{{"name":"测试开发者","public":"{}"}}]}}"#, public.trim())).unwrap();
    path
}

#[test]
fn keygen_sign_verify_full_flow() {
    let dir = tmp("flow");
    let out = bin().current_dir(&dir).args(["keygen", "mykey"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("trusted_keys.json") && stdout.contains("nuthusk-ed25519-public:"));
    assert!(dir.join("mykey.key").is_file() && dir.join("mykey.pub").is_file());

    // 不覆盖已有密钥
    let again = bin().current_dir(&dir).args(["keygen", "mykey"]).output().unwrap();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("已存在"));

    let pkg = dir.join("plugin.nut");
    make_zip(&pkg, "<p>ok</p>");
    let keys = trusted_keys_json(&dir, &dir.join("mykey.pub"));

    // 未签名：未经安全验证
    let v = bin().args(["verify"]).arg(&pkg).arg("--keys").arg(&keys).output().unwrap();
    assert!(v.status.success());
    assert!(String::from_utf8_lossy(&v.stdout).contains("未经安全验证"));

    // 签名
    let signed = dir.join("plugin.signed.nut");
    let s = bin().arg("sign").arg(&pkg).arg("--key").arg(dir.join("mykey.key")).arg("--yes").output().unwrap();
    assert!(s.status.success(), "{}", String::from_utf8_lossy(&s.stderr));
    assert!(signed.is_file(), "默认输出 <名字>.signed.<扩展名>，且不覆盖原文件");
    let v = bin().arg("verify").arg(&signed).arg("--keys").arg(&keys).output().unwrap();
    assert!(v.status.success());
    assert!(String::from_utf8_lossy(&v.stdout).contains("可信来源"), "{}", String::from_utf8_lossy(&v.stdout));

    // 与核心库的判断一致
    let keyset = load_trusted_keys(&fs::read_to_string(&keys).unwrap()).unwrap();
    assert_eq!(nuthusk_core::package::inspect(&signed, &keyset).unwrap().trust.level, TrustLevel::Trusted);

    // 篡改内容后验证失败（退出码非 0）
    let tampered = dir.join("tampered.nut");
    {
        let sig = {
            let mut ar = zip::ZipArchive::new(File::open(&signed).unwrap()).unwrap();
            let mut t = String::new();
            std::io::Read::read_to_string(&mut ar.by_name("signature.json").unwrap(), &mut t).unwrap();
            t
        };
        make_zip(&tampered, "<script>evil()</script>");
        // 往篡改后的包里补上原来的签名
        let tmp2 = dir.join("tampered2.nut");
        nuthusk_core::package::write_signed(&tampered, &tmp2, &sig).unwrap();
        let v = bin().arg("verify").arg(&tmp2).arg("--keys").arg(&keys).output().unwrap();
        assert!(!v.status.success());
        assert!(String::from_utf8_lossy(&v.stdout).contains("签名无效"));
    }
}

#[test]
fn sign_requires_explicit_confirmation() {
    let dir = tmp("confirm");
    assert!(bin().current_dir(&dir).args(["keygen", "k"]).status().unwrap().success());
    let pkg = dir.join("p.nut");
    make_zip(&pkg, "x");

    // 回答 no → 取消，不产生任何输出文件
    let mut child = bin()
        .arg("sign").arg(&pkg).arg("--key").arg(dir.join("k.key"))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"no\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("没有签名"));
    assert!(!dir.join("p.signed.nut").exists());
    // 确认前会列出插件信息
    assert!(String::from_utf8_lossy(&out.stdout).contains("CLI 测试"));

    // 回答 yes → 签名
    let mut child = bin()
        .arg("sign").arg(&pkg).arg("--key").arg(dir.join("k.key"))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"yes\n").unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    assert!(dir.join("p.signed.nut").is_file());
}

#[test]
fn bad_arguments_fail_with_a_helpful_message() {
    let out = bin().arg("sign").output().unwrap();
    assert!(!out.status.success());
    let out = bin().arg("whatever").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("未知命令"));
    let help = bin().arg("--help").output().unwrap();
    assert!(help.status.success() && String::from_utf8_lossy(&help.stdout).contains("keygen"));
}
