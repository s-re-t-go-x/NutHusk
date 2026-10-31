mod common;
use common::*;
use nuthusk_core::PluginDir;
use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use zip::write::SimpleFileOptions;

#[tokio::test]
async fn inspect_reads_manifest_from_root_or_single_top_folder() {
    let root = tmp("inspect");
    let m = manager(&root);

    let a = root.join("a.nut");
    backend_pkg(&a, "com.test.a", "1.0.0");
    let info = m.inspect_package(&a).await.unwrap();
    assert_eq!(info.manifest.id, "com.test.a");
    assert_eq!(info.sha256.len(), 64);
    assert_eq!(info.backend_command.as_deref(), Some("node main.js"));
    assert!(info.backend_found.is_some());
    assert!(info.compat_error.is_none() && info.installed_version.is_none());
    assert_eq!(info.file_count, 3);
    assert_eq!(info.trust.level, nuthusk_core::TrustLevel::Unverified);
    assert_eq!(info.trust.reason, "没有签名");
    assert!(info.install_target.as_deref().unwrap().ends_with("appdata"), "默认装到优先级最高的 AppData");
    assert_eq!(info.install_target_label.as_deref(), Some("AppData"));

    // 带一层顶层文件夹，且混有 macOS 的 __MACOSX
    let b = root.join("b.nut");
    make_zip(&b, &[
        ("my-plugin/plugin.json", &manifest("com.test.b", "1.0.0", "")),
        ("my-plugin/ui.html", "x"),
        ("__MACOSX/my-plugin/._ui.html", "junk"),
    ]);
    assert_eq!(m.inspect_package(&b).await.unwrap().manifest.id, "com.test.b");
}

#[tokio::test]
async fn inspect_rejects_malicious_or_broken_packages() {
    let root = tmp("reject");
    let m = manager(&root);
    let check = |name: &'static str, files: Vec<(&'static str, String)>, expect: &'static str| {
        let m = &m;
        let path = root.join(name);
        async move {
            let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (*a, b.as_str())).collect();
            make_zip(&path, &refs);
            let err = m.inspect_package(&path).await.unwrap_err();
            assert!(err.contains(expect), "{name}: 期望含「{expect}」，实际: {err}");
        }
    };

    check("slip.nut", vec![("plugin.json", manifest("com.test.x", "1.0.0", "")), ("../evil.txt", "x".into())], "不安全的路径").await;
    check("nomanifest.nut", vec![("ui.html", "x".into())], "找不到 plugin.json").await;
    check("badid.nut", vec![("plugin.json", manifest("Bad ID", "1.0.0", "")), ("ui.html", "x".into())], "id 不合法").await;
    check("noui.nut", vec![("plugin.json", manifest("com.test.x", "1.0.0", ""))], "界面文件不在包内").await;
    check(
        "noicon.nut",
        vec![("plugin.json", manifest("com.test.x", "1.0.0", r#","icon":"icon.png""#)), ("ui.html", "x".into())],
        "图标文件不在包内",
    ).await;
    check(
        "badicon.nut",
        vec![("plugin.json", manifest("com.test.x", "1.0.0", r#","icon":"icon.exe""#)), ("ui.html", "x".into())],
        "icon 必须是",
    ).await;
    check(
        "nobackend.nut",
        vec![("plugin.json", manifest("com.test.x", "1.0.0", r#","backend":{"command":"./run"}"#)), ("ui.html", "x".into())],
        "后端程序不在包内",
    ).await;
    check(
        "nobackend-win.nut",
        vec![
            ("plugin.json", manifest("com.test.x", "1.0.0", r#","backend":{"command":"node","windows":{"command":"./bin/x.exe"}}"#)),
            ("ui.html", "x".into()),
        ],
        "后端程序不在包内",
    ).await;
    check("badjson.nut", vec![("plugin.json", "{ not json".into())], "无法解析").await;

    let notzip = root.join("notzip.nut");
    fs::write(&notzip, "hello").unwrap();
    assert!(m.inspect_package(&notzip).await.unwrap_err().contains("zip"));
}

#[tokio::test]
async fn inspect_rejects_symlink_entries() {
    let root = tmp("symlink");
    let m = manager(&root);
    let path = root.join("link.nut");
    let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
    w.start_file("plugin.json", SimpleFileOptions::default()).unwrap();
    w.write_all(manifest("com.test.x", "1.0.0", "").as_bytes()).unwrap();
    w.add_symlink("ui.html", "/etc/passwd", SimpleFileOptions::default()).unwrap();
    w.finish().unwrap();
    assert!(m.inspect_package(&path).await.unwrap_err().contains("符号链接"));
}

#[tokio::test]
async fn icon_is_previewed_in_package_and_exposed_after_install() {
    let root = tmp("icon");
    let m = manager(&root);
    let p = root.join("p.nut");
    make_zip(&p, &[
        ("plugin.json", &manifest("com.test.icon", "1.0.0", r#","icon":"img/icon.svg""#)),
        ("ui.html", "x"),
        ("img/icon.svg", ICON_SVG),
    ]);
    let info = m.inspect_package(&p).await.unwrap();
    assert!(info.icon_data.as_deref().unwrap().starts_with("data:image/svg+xml;base64,"));
    m.install_package(&p, &info.sha256).await.unwrap();
    let listed = m.list().into_iter().find(|x| x.id == "com.test.icon").unwrap();
    assert_eq!(listed.icon.as_deref(), Some("img/icon.svg"));
    assert!(std::path::Path::new(&listed.path).join("img/icon.svg").is_file());
}

#[tokio::test]
async fn install_update_and_run_backend_end_to_end() {
    let root = tmp("e2e");
    let m = manager(&root);

    let v1 = root.join("v1.nut");
    backend_pkg(&v1, "com.test.echo", "1.0.0");
    let info = m.inspect_package(&v1).await.unwrap();
    let r = m.install_package(&v1, &info.sha256).await.unwrap();
    assert_eq!(r.id, "com.test.echo");
    assert!(r.active && r.group_label == "AppData");

    let listed = m.list().into_iter().find(|p| p.id == r.id).unwrap();
    assert!(listed.has_backend && listed.error.is_none());
    assert_eq!((listed.group.as_str(), listed.backend_command.as_deref()), ("appdata", Some("node main.js")));
    assert_eq!(m.invoke(&r.id, "ping", json!({})).await.unwrap(), "1.0.0");
    assert!(m.logs(&r.id).iter().any(|l| l.contains("got ping")), "stderr 应出现在日志里: {:?}", m.logs(&r.id));

    // 更新到 v2：旧后端必须被停掉，再次调用拿到新版本；界面能看到"已安装版本"
    let v2 = root.join("v2.nut");
    backend_pkg(&v2, "com.test.echo", "2.0.0");
    let info2 = m.inspect_package(&v2).await.unwrap();
    assert_eq!(info2.installed_version.as_deref(), Some("1.0.0"));
    m.install_package(&v2, &info2.sha256).await.unwrap();
    assert_eq!(m.invoke(&r.id, "ping", json!({})).await.unwrap(), "2.0.0");

    // 没有遗留的临时文件
    let leftovers: Vec<String> = fs::read_dir(root.join("appdata"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, vec!["com.test.echo".to_string()], "{leftovers:?}");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(root.join("appdata/com.test.echo/main.js")).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "应保留包内的可执行位");
    }
    m.stop_all().await;
}

#[tokio::test]
async fn install_refuses_changed_file_and_incompatible_api() {
    let root = tmp("refuse");
    let m = manager(&root);

    let p = root.join("p.nut");
    backend_pkg(&p, "com.test.p", "1.0.0");
    let sha = m.inspect_package(&p).await.unwrap().sha256;
    backend_pkg(&p, "com.test.p", "999.0.0"); // 用户确认之后文件被换掉
    let err = m.install_package(&p, &sha).await.unwrap_err();
    assert!(err.contains("发生了变化"), "{err}");
    assert!(m.list().is_empty());

    let f = root.join("future.nut");
    make_zip(&f, &[("plugin.json", r#"{"id":"com.test.future","name":"F","version":"1.0.0","apiVersion":"^2.0.0"}"#)]);
    let info = m.inspect_package(&f).await.unwrap();
    assert!(info.compat_error.as_deref().unwrap().contains("需要宿主 API"));
    assert!(m.install_package(&f, &info.sha256).await.is_err());
    assert!(!root.join("appdata/com.test.future").exists());
    assert!(fs::read_dir(root.join("appdata")).unwrap().next().is_none(), "失败后不留临时文件");
}

#[tokio::test]
async fn install_goes_to_highest_priority_writable_directory() {
    let root = tmp("target");
    let mut g = groups(&root);
    g.reverse(); // program 排第一
    let m = nuthusk_core::PluginManager::new(g, root.join("s.json"));
    m.scan();

    let p = root.join("p.nut");
    make_zip(&p, &[("plugin.json", &manifest("com.test.t", "1.0.0", "")), ("ui.html", "x")]);
    let info = m.inspect_package(&p).await.unwrap();
    assert_eq!(info.install_target_label.as_deref(), Some("程序目录"));
    let r = m.install_package(&p, &info.sha256).await.unwrap();
    assert_eq!(r.group_label, "程序目录");
    assert!(root.join("program/com.test.t/plugin.json").is_file());
    assert!(!root.join("appdata/com.test.t").exists());
}

#[tokio::test]
async fn install_skips_unwritable_directory_and_reports_when_shadowed() {
    let root = tmp("unwritable");
    let appdata = root.join("appdata");
    fs::create_dir_all(&appdata).unwrap();
    // program 组的主路径建在一个普通文件下面 → 不可写；第二个路径里已有同 id 的旧版本
    let blocker = root.join("blocker");
    fs::write(&blocker, "x").unwrap();
    let readable = root.join("program-readonly");
    fs::create_dir_all(readable.join("old")).unwrap();
    fs::write(readable.join("old/plugin.json"), manifest("com.test.s", "1.0.0", "")).unwrap();
    let g = vec![
        PluginDir { key: "program".into(), label: "程序目录".into(), paths: vec![blocker.join("plugins"), readable] },
        PluginDir { key: "appdata".into(), label: "AppData".into(), paths: vec![appdata.clone()] },
    ];
    let m = nuthusk_core::PluginManager::new(g, root.join("s.json"));
    m.scan();
    assert!(!m.dirs_info()[0].paths[0].writable);

    let p = root.join("p.nut");
    make_zip(&p, &[("plugin.json", &manifest("com.test.s", "2.0.0", "")), ("ui.html", "x")]);
    let info = m.inspect_package(&p).await.unwrap();
    assert_eq!(info.installed_version.as_deref(), Some("1.0.0"));
    assert_eq!(info.install_target_label.as_deref(), Some("AppData"), "跳过不可写的目录");
    let r = m.install_package(&p, &info.sha256).await.unwrap();
    assert!(appdata.join("com.test.s/plugin.json").is_file());
    assert!(!r.active, "优先级更高的程序目录里的旧版本仍在生效，要如实告诉用户");
    assert_eq!(m.list()[0].version, "1.0.0");
}

#[tokio::test]
async fn update_replaces_in_place_when_folder_name_differs_from_id() {
    let root = tmp("inplace");
    let g = groups(&root);
    fs::create_dir_all(g[0].paths[0].join("my-folder")).unwrap();
    fs::write(g[0].paths[0].join("my-folder/plugin.json"), manifest("com.test.ip", "1.0.0", "")).unwrap();
    fs::write(g[0].paths[0].join("my-folder/ui.html"), "old").unwrap();
    let m = manager(&root);

    let p = root.join("p.nut");
    make_zip(&p, &[("plugin.json", &manifest("com.test.ip", "2.0.0", "")), ("ui.html", "new")]);
    let sha = m.inspect_package(&p).await.unwrap().sha256;
    let r = m.install_package(&p, &sha).await.unwrap();
    assert!(r.active);
    assert_eq!(fs::read_to_string(g[0].paths[0].join("my-folder/ui.html")).unwrap(), "new");
    assert!(!g[0].paths[0].join("com.test.ip").exists(), "不会产生第二份同 id 的目录");
    assert_eq!(m.list().len(), 1);
}

#[tokio::test]
async fn uninstall_works_in_either_directory() {
    let root = tmp("uninstall");
    let g = groups(&root);
    fs::create_dir_all(g[1].paths[0].join("core-tool")).unwrap();
    fs::write(g[1].paths[0].join("core-tool/plugin.json"), manifest("com.test.prog", "1.0.0", "")).unwrap();
    fs::write(g[1].paths[0].join("core-tool/ui.html"), "x").unwrap();
    let m = manager(&root);

    let p = root.join("p.nut");
    backend_pkg(&p, "com.test.p", "1.0.0");
    let sha = m.inspect_package(&p).await.unwrap().sha256;
    m.install_package(&p, &sha).await.unwrap();
    m.set_enabled("com.test.p", false).await.unwrap();

    // 不区分内置和用户安装：两个目录里的插件都能卸载
    m.uninstall("com.test.p").await.unwrap();
    assert!(!root.join("appdata/com.test.p").exists());
    assert!(!fs::read_to_string(root.join("state.json")).unwrap().contains("com.test.p"), "禁用记录一并清除");
    m.uninstall("com.test.prog").await.unwrap();
    assert!(!g[1].paths[0].join("core-tool").exists());
    assert!(m.list().is_empty());
    assert!(m.uninstall("com.test.p").await.is_err());
}

#[tokio::test]
async fn uninstalling_the_winner_reveals_the_shadowed_copy() {
    let root = tmp("reveal");
    let g = groups(&root);
    for (dir, version) in [(&g[0].paths[0], "2.0.0"), (&g[1].paths[0], "1.0.0")] {
        fs::create_dir_all(dir.join("x")).unwrap();
        fs::write(dir.join("x/plugin.json"), manifest("com.test.r", version, "")).unwrap();
    }
    let m = manager(&root);
    assert_eq!(m.list()[0].version, "2.0.0");
    m.uninstall("com.test.r").await.unwrap();
    assert_eq!(m.list()[0].version, "1.0.0");
}

#[tokio::test]
async fn broken_plugin_directory_can_be_uninstalled() {
    let root = tmp("broken");
    let g = groups(&root);
    fs::create_dir_all(g[0].paths[0].join("oops")).unwrap();
    fs::write(g[0].paths[0].join("oops/plugin.json"), "{ bad").unwrap();
    let m = manager(&root);
    assert_eq!(m.list()[0].id, "invalid:oops");
    m.uninstall("invalid:oops").await.unwrap();
    assert!(!g[0].paths[0].join("oops").exists());
}

#[tokio::test]
async fn spawn_failure_is_recorded_in_logs() {
    let root = tmp("spawnfail");
    let m = manager(&root);
    let p = root.join("p.nut");
    make_zip(&p, &[
        ("plugin.json", &manifest("com.test.nocmd", "1.0.0", r#","backend":{"command":"definitely-not-a-real-command-xyz"}"#)),
        ("ui.html", "x"),
    ]);
    let info = m.inspect_package(&p).await.unwrap();
    assert_eq!(info.backend_found, Some(false), "应提示本机找不到该程序");
    m.install_package(&p, &info.sha256).await.unwrap();

    let err = m.invoke("com.test.nocmd", "x", json!({})).await.unwrap_err();
    assert!(err.contains("无法启动插件后端"), "{err}");
    assert!(m.logs("com.test.nocmd").iter().any(|l| l.contains("无法启动插件后端")));
}

#[tokio::test]
async fn reload_stops_backends_of_removed_plugins() {
    let root = tmp("reload");
    let m = manager(&root);
    let p = root.join("p.nut");
    backend_pkg(&p, "com.test.r", "1.0.0");
    let sha = m.inspect_package(&p).await.unwrap().sha256;
    m.install_package(&p, &sha).await.unwrap();
    assert_eq!(m.invoke("com.test.r", "ping", json!({})).await.unwrap(), "1.0.0");

    // Windows 上子进程运行中时工作目录被系统锁定，无法直接删除整个文件夹；
    // 删除 plugin.json 同样会让 scan 找不到插件，触发 reload 识别并停止后端
    if cfg!(windows) {
        fs::remove_file(root.join("appdata/com.test.r/plugin.json")).unwrap();
    } else {
        fs::remove_dir_all(root.join("appdata/com.test.r")).unwrap();
    }
    m.reload().await;
    assert!(m.list().is_empty());
    assert!(m.invoke("com.test.r", "ping", json!({})).await.is_err());
    let _ = fs::remove_dir_all(root.join("appdata/com.test.r"));
}

#[tokio::test]
async fn backend_command_follows_operating_system_overrides() {
    let root = tmp("peros");
    let m = manager(&root);
    // 只有当前系统的专用配置，没有通用 command；换成别的系统就会被判为不兼容
    let os = std::env::consts::OS;
    let p = root.join("p.nut");
    make_zip(&p, &[
        ("plugin.json", &manifest("com.test.os", "1.0.0", &format!(r#","backend":{{"{os}":{{"command":"node","args":["main.js"]}}}}"#))),
        ("ui.html", "x"),
        ("main.js", &backend_js("os-ok")),
    ]);
    let info = m.inspect_package(&p).await.unwrap();
    assert!(info.compat_error.is_none());
    assert_eq!(info.backend_command.as_deref(), Some("node main.js"));
    m.install_package(&p, &info.sha256).await.unwrap();
    assert_eq!(m.invoke("com.test.os", "x", json!({})).await.unwrap(), "os-ok");
    m.stop_all().await;
}

#[test]
fn dropped_packages_are_staged_safely_in_temp_dir() {
    let root = tmp("drop");
    let dir = root.join("drop");
    let path = nuthusk_core::package::save_temp_package(&dir, "../../evil/我的 插件.nut", b"data").unwrap();
    assert!(path.starts_with(&dir), "文件名里的路径部分被丢弃，不会写到临时目录之外: {path:?}");
    assert_eq!(fs::read(&path).unwrap(), b"data");
    let weird = nuthusk_core::package::save_temp_package(&dir, "a<b>:c|d?.nut", b"x").unwrap();
    assert!(weird.file_name().unwrap().to_string_lossy().ends_with("a_b__c_d_.nut"));
    let _: PathBuf = weird.clone();

    // 只能删除临时目录里的文件
    let outside = root.join("outside.txt");
    fs::write(&outside, "keep").unwrap();
    assert!(nuthusk_core::package::discard_temp_package(&dir, &outside).is_err());
    assert!(outside.exists());
    nuthusk_core::package::discard_temp_package(&dir, &path).unwrap();
    assert!(!path.exists());
    nuthusk_core::package::clean_temp_dir(&dir);
    assert!(!dir.exists());
}


#[tokio::test]
async fn rejects_portable_path_aliases_and_duplicate_zip_entries() {
    let root = tmp("path-aliases");
    let m = manager(&root);
    for unsafe_name in ["ui.html:payload", "CON.txt", "file.", "folder/file ", "a/../hidden.txt", "dir\\escape.txt"] {
        let path = root.join("unsafe.nut");
        make_zip(&path, &[("plugin.json", &manifest("com.test.alias", "1.0.0", "")), ("ui.html", "ok"), (unsafe_name, "bad")]);
        assert!(m.inspect_package(&path).await.is_err(), "accepted {unsafe_name}");
    }
    // Duplicate logical file paths are rejected even when the archive uses ./ aliases.
    let path = root.join("duplicate.nut");
    make_zip(&path, &[("plugin.json", &manifest("com.test.alias", "1.0.0", "")), ("ui.html", "ok"), ("folder/data.txt", "first"), ("folder/./data.txt", "second")]);
    assert!(m.inspect_package(&path).await.unwrap_err().contains("重复路径"));
}

#[tokio::test]
async fn concurrent_package_installs_do_not_share_staging_files() {
    let root = tmp("parallel-installs");
    let m = manager(&root);
    let (a, b) = (root.join("a.nut"), root.join("b.nut"));
    backend_pkg(&a, "com.test.parallel-a", "1.0.0");
    backend_pkg(&b, "com.test.parallel-b", "2.0.0");
    let sha_a = m.inspect_package(&a).await.unwrap().sha256;
    let sha_b = m.inspect_package(&b).await.unwrap().sha256;
    let (a_result, b_result) = tokio::join!(m.install_package(&a, &sha_a), m.install_package(&b, &sha_b));
    assert!(a_result.is_ok(), "{a_result:?}");
    assert!(b_result.is_ok(), "{b_result:?}");
    assert_eq!(m.list().len(), 2);
}
