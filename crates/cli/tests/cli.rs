use nuthusk_core::signature::load_trusted_keys;
use nuthusk_core::TrustLevel;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nuthusk-cli"))
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
    w.start_file("plugin.json", SimpleFileOptions::default())
        .unwrap();
    w.write_all(r#"{"id":"com.test.cli","name":"CLI 测试","version":"1.0.0","apiVersion":"^1.0.0","ui":"ui.html"}"#.as_bytes()).unwrap();
    w.start_file("ui.html", SimpleFileOptions::default())
        .unwrap();
    w.write_all(ui.as_bytes()).unwrap();
    w.finish().unwrap();
}

fn trusted_keys_json(dir: &Path, pub_file: &Path) -> PathBuf {
    let public = fs::read_to_string(pub_file).unwrap();
    let path = dir.join("trusted_keys.json");
    fs::write(
        &path,
        format!(
            r#"{{"keys":[{{"name":"测试开发者","public":"{}"}}]}}"#,
            public.trim()
        ),
    )
    .unwrap();
    path
}

#[test]
fn keygen_sign_verify_full_flow() {
    let dir = tmp("flow");
    let out = bin()
        .current_dir(&dir)
        .args(["keygen", "mykey"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("trusted_keys.json") && stdout.contains("nuthusk-ed25519-public:"));
    assert!(dir.join("mykey.key").is_file() && dir.join("mykey.pub").is_file());

    // 不覆盖已有密钥
    let again = bin()
        .current_dir(&dir)
        .args(["keygen", "mykey"])
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("已存在"));

    let pkg = dir.join("plugin.nut");
    make_zip(&pkg, "<p>ok</p>");
    let keys = trusted_keys_json(&dir, &dir.join("mykey.pub"));

    // 未签名：未经安全验证
    let v = bin()
        .args(["verify"])
        .arg(&pkg)
        .arg("--keys")
        .arg(&keys)
        .output()
        .unwrap();
    assert!(v.status.success());
    assert!(String::from_utf8_lossy(&v.stdout).contains("未经安全验证"));

    // 签名
    let signed = dir.join("plugin.signed.nut");
    let s = bin()
        .arg("sign")
        .arg(&pkg)
        .arg("--key")
        .arg(dir.join("mykey.key"))
        .arg("--yes")
        .output()
        .unwrap();
    assert!(s.status.success(), "{}", String::from_utf8_lossy(&s.stderr));
    assert!(
        signed.is_file(),
        "默认输出 <名字>.signed.<扩展名>，且不覆盖原文件"
    );
    let v = bin()
        .arg("verify")
        .arg(&signed)
        .arg("--keys")
        .arg(&keys)
        .output()
        .unwrap();
    assert!(v.status.success());
    assert!(
        String::from_utf8_lossy(&v.stdout).contains("可信来源"),
        "{}",
        String::from_utf8_lossy(&v.stdout)
    );

    // 与核心库的判断一致
    let keyset = load_trusted_keys(&fs::read_to_string(&keys).unwrap()).unwrap();
    assert_eq!(
        nuthusk_core::package::inspect(&signed, &keyset)
            .unwrap()
            .trust
            .level,
        TrustLevel::Trusted
    );

    // 篡改内容后验证失败（退出码非 0）
    let tampered = dir.join("tampered.nut");
    {
        let sig = {
            let mut ar = zip::ZipArchive::new(File::open(&signed).unwrap()).unwrap();
            let mut t = String::new();
            std::io::Read::read_to_string(&mut ar.by_name("signature.json").unwrap(), &mut t)
                .unwrap();
            t
        };
        make_zip(&tampered, "<script>evil()</script>");
        // 往篡改后的包里补上原来的签名
        let tmp2 = dir.join("tampered2.nut");
        nuthusk_core::package::write_signed(&tampered, &tmp2, &sig).unwrap();
        let v = bin()
            .arg("verify")
            .arg(&tmp2)
            .arg("--keys")
            .arg(&keys)
            .output()
            .unwrap();
        assert!(!v.status.success());
        assert!(String::from_utf8_lossy(&v.stdout).contains("签名无效"));
    }
}

#[test]
fn sign_requires_explicit_confirmation() {
    let dir = tmp("confirm");
    assert!(bin()
        .current_dir(&dir)
        .args(["keygen", "k"])
        .status()
        .unwrap()
        .success());
    let pkg = dir.join("p.nut");
    make_zip(&pkg, "x");

    // 回答 no → 取消，不产生任何输出文件
    let mut child = bin()
        .arg("sign")
        .arg(&pkg)
        .arg("--key")
        .arg(dir.join("k.key"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"no\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("没有签名"));
    assert!(!dir.join("p.signed.nut").exists());
    // 确认前会列出插件信息
    assert!(String::from_utf8_lossy(&out.stdout).contains("CLI 测试"));

    // 回答 yes → 签名
    let mut child = bin()
        .arg("sign")
        .arg(&pkg)
        .arg("--key")
        .arg(dir.join("k.key"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"yes\n").unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    assert!(dir.join("p.signed.nut").is_file());
}

#[test]
fn devkit_new_check_run_and_pack_cli_flow() {
    let dir = tmp("devkit");
    let created = dir.join("upper-tool");

    // 1. new
    let out = bin()
        .current_dir(&dir)
        .args([
            "new",
            "com.example.upper-tool",
            "--lang",
            "js",
            "--name",
            "大写转换器",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(created.join("plugin.json").is_file());
    assert!(created.join("main.js").is_file());
    assert!(created.join("ui.html").is_file());

    // 2. check & run
    if nuthusk_core::command_on_path("node") {
        let chk = bin().arg("check").arg(&created).output().unwrap();
        assert!(
            chk.status.success(),
            "{}",
            String::from_utf8_lossy(&chk.stderr)
        );
        assert!(String::from_utf8_lossy(&chk.stdout).contains("JSON-RPC 协议探测通过"));

        let run_out = bin()
            .arg("run")
            .arg(&created)
            .arg("transform")
            .args(["--input", "nuthusk cli"])
            .output()
            .unwrap();
        assert!(
            run_out.status.success(),
            "{}",
            String::from_utf8_lossy(&run_out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run_out.stdout).trim(),
            "NUTHUSK CLI"
        );
    } else {
        let chk = bin()
            .arg("check")
            .arg(&created)
            .arg("--no-backend")
            .output()
            .unwrap();
        assert!(
            chk.status.success(),
            "{}",
            String::from_utf8_lossy(&chk.stderr)
        );
    }

    // 3. pack
    let packed_file = dir.join("upper-tool-0.1.0.nut");
    let p = bin()
        .current_dir(&dir)
        .arg("pack")
        .arg(&created)
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    assert!(packed_file.is_file());
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

#[test]
fn generated_index_uses_package_manifest_hash_size_and_encoded_url() {
    let dir = tmp("index");
    let packages = dir.join("packages");
    fs::create_dir(&packages).unwrap();
    let pkg = packages.join("different name.nut");
    make_zip(&pkg, "<p>ok</p>");
    let out = dir.join("index.json");
    let generate = || {
        bin()
            .arg("index")
            .arg(&packages)
            .args([
                "--base-url",
                "https://example.com/releases",
                "--name",
                "测试源",
            ])
            .arg("--out")
            .arg(&out)
            .output()
            .unwrap()
    };
    let result = generate();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(&out).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let entry = &value["plugins"][0];
    assert_eq!(value["name"], "测试源");
    assert_eq!(entry["id"], "com.test.cli");
    assert_eq!(entry["version"], "1.0.0");
    assert_eq!(
        entry["url"],
        "https://example.com/releases/different%20name.nut"
    );
    assert_eq!(
        entry["sha256"],
        nuthusk_core::package::sha256_file(&pkg).unwrap()
    );
    assert_eq!(entry["size"], fs::metadata(&pkg).unwrap().len());
    assert!(!generate().status.success(), "默认不覆盖输出");
    assert_eq!(fs::read(&out).unwrap(), bytes);
    fs::copy(&pkg, packages.join("duplicate.nut")).unwrap();
    let result = bin()
        .arg("index")
        .arg(&packages)
        .args(["--base-url", "https://example.com/releases", "--force"])
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("重复的插件 id"));
    assert_eq!(fs::read(&out).unwrap(), bytes, "失败不能覆盖已有索引");
}

#[test]
fn generated_runtime_recipe_is_loadable_and_uses_local_file_digest() {
    let dir = tmp("recipe");
    let archive = dir.join("runtime.zip");
    make_zip(&archive, "demo");
    let out = dir.join("nutConfig/runtimes.d/demo.json");
    let generate = |bin_path: &str, force: bool| {
        let mut cmd = bin();
        cmd.args(["runtime-recipe", "demo", "--version", "1.2.3", "--file"])
            .arg(&archive)
            .args([
                "--url",
                "https://example.com/runtime.zip",
                "--bin",
                bin_path,
                "--platform",
                "windows-x86_64",
                "--strip-components",
                "1",
                "--keep",
                "bin/demo.exe,lib/*",
                "--aliases",
                "demo-cli",
            ])
            .arg("--out")
            .arg(&out);
        if force {
            cmd.arg("--force");
        }
        cmd.output().unwrap()
    };
    let result = generate("bin/demo.exe", false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(&out).unwrap();
    let recipe: nuthusk_core::Recipe = serde_json::from_slice(&bytes).unwrap();
    recipe.validate().unwrap();
    let plat = &recipe.platforms["windows-x86_64"];
    assert_eq!(
        plat.sha256,
        nuthusk_core::package::sha256_file(&archive).unwrap()
    );
    assert_eq!(plat.size, Some(fs::metadata(&archive).unwrap().len()));
    assert_eq!(plat.strip_components, 1);
    assert_eq!(plat.keep, ["bin/demo.exe", "lib/*"]);
    assert_eq!(recipe.aliases, ["demo-cli"]);
    assert!(!generate("bin/demo.exe", false).status.success());
    assert!(generate("bin/demo.exe", true).status.success());
    assert!(!generate("../escape.exe", true).status.success());
    assert_eq!(fs::read(&out).unwrap(), bytes);
    let roots = std::sync::Arc::new(nuthusk_core::Roots::new(
        vec![nuthusk_core::DataRoot {
            key: "test".into(),
            label: "Test".into(),
            bases: vec![dir.clone()],
        }],
        dir.join("order.json"),
    ));
    assert!(nuthusk_core::RuntimeManager::new(roots)
        .recipes()
        .iter()
        .any(|r| r.name == "demo" && r.user));
}

#[test]
fn config_generation_rejects_bad_urls_and_empty_package_directory() {
    let dir = tmp("config-errors");
    for url in [
        "http://example.com/p/",
        "https://user:pass@example.com/p/",
        "https://example.com/p/?token=x",
    ] {
        let result = bin()
            .arg("index")
            .arg(&dir)
            .args(["--base-url", url])
            .output()
            .unwrap();
        assert!(!result.status.success());
    }
    let result = bin()
        .arg("index")
        .arg(&dir)
        .args(["--base-url", "https://example.com/p/"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("没有"));
    assert!(!dir.join("index.json").exists());
}


#[test]
fn headless_backend_write_and_large_output_have_limits() {
    let dir = tmp("probe-limits");
    let plugin = dir.join("plugin");
    fs::create_dir(&plugin).unwrap();
    fs::write(plugin.join("plugin.json"), r#"{"id":"com.test.probe","name":"Probe","version":"1.0.0","apiVersion":"^1.0.0","backend":{"command":"node","args":["main.js"]}}"#).unwrap();
    fs::write(plugin.join("main.js"), "setInterval(() => {}, 1000)").unwrap();
    let start = std::time::Instant::now();
    let mut child = bin().arg("run").arg(&plugin).arg("echo")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&vec![b'x'; 1024 * 1024]).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("超时"));
    assert!(start.elapsed() < std::time::Duration::from_secs(10));
    fs::write(plugin.join("main.js"), "process.stdout.write('x'.repeat(17*1024*1024)); setInterval(() => {}, 1000)").unwrap();
    let out = bin().arg("run").arg(&plugin).args(["echo", "--input", "hello"]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("上限"));
    fs::write(plugin.join("main.js"), "console.log('sensitive-user-content')").unwrap();
    let out = bin().arg("run").arg(&plugin).args(["echo", "--input", "hello"]).output().unwrap();
    assert!(!String::from_utf8_lossy(&out.stderr).contains("sensitive-user-content"));
}
