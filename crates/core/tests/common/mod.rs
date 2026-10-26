#![allow(dead_code)]
use nuthusk_core::signature::{self, TrustedKey};
use nuthusk_core::{PluginDir, PluginManager};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

pub fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nuthusk-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

pub fn make_zip(path: &Path, files: &[(&str, &str)]) {
    let mut w = zip::ZipWriter::new(File::create(path).unwrap());
    for (name, content) in files {
        w.start_file(*name, SimpleFileOptions::default().unix_permissions(0o755)).unwrap();
        w.write_all(content.as_bytes()).unwrap();
    }
    w.finish().unwrap();
}

pub fn manifest(id: &str, version: &str, extra: &str) -> String {
    format!(r#"{{"id":"{id}","name":"测试插件","version":"{version}","apiVersion":"^1.0.0","ui":"ui.html"{extra}}}"#)
}

/// 后端：每次调用返回固定的标记，并向 stderr 写一行日志。
pub fn backend_js(marker: &str) -> String {
    format!(
        "const rl=require('readline').createInterface({{input:process.stdin}});\
         rl.on('line',l=>{{const m=JSON.parse(l);console.error('got '+m.method);\
         process.stdout.write(JSON.stringify({{jsonrpc:'2.0',id:m.id,result:'{marker}'}})+'\\n')}});"
    )
}

pub fn backend_pkg(path: &Path, id: &str, version: &str) {
    make_zip(
        path,
        &[
            ("plugin.json", &manifest(id, version, r#","backend":{"command":"node","args":["main.js"]}"#)),
            ("ui.html", "<html></html>"),
            ("main.js", &backend_js(version)),
        ],
    );
}

pub const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8" fill="#f00"/></svg>"##;

/// 两个目录组：appdata（默认优先）和 program。
pub fn groups(root: &Path) -> Vec<PluginDir> {
    let (a, p) = (root.join("appdata"), root.join("program"));
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&p).unwrap();
    vec![
        PluginDir { key: "appdata".into(), label: "AppData".into(), paths: vec![a] },
        PluginDir { key: "program".into(), label: "程序目录".into(), paths: vec![p] },
    ]
}

pub fn manager(root: &Path) -> PluginManager {
    let m = PluginManager::new(groups(root), root.join("state.json"));
    m.scan();
    m
}

pub fn manager_with_keys(root: &Path, keys: Vec<TrustedKey>) -> PluginManager {
    let m = PluginManager::new(groups(root), root.join("state.json")).with_trusted_keys(keys);
    m.scan();
    m
}

/// 固定种子的测试密钥（仅测试用）。
pub fn test_seed(n: u8) -> [u8; 32] {
    [n; 32]
}

pub fn trusted(seed: &[u8; 32], name: &str) -> Vec<TrustedKey> {
    let public = signature::public_key_of_seed(seed);
    let json = format!(
        r#"{{"keys":[{{"name":"{name}","public":"{}"}}]}}"#,
        signature::format_public_key(&public)
    );
    signature::load_trusted_keys(&json).unwrap()
}

/// 给包签名并输出到 dst。
pub fn sign_package(src: &Path, dst: &Path, seed: &[u8; 32]) {
    let (meta, digest) = nuthusk_core::package::digest_of(src).unwrap();
    let sig = signature::sign(seed, &meta.manifest.id, &meta.manifest.version, &digest);
    nuthusk_core::package::write_signed(src, dst, &serde_json::to_string_pretty(&sig).unwrap()).unwrap();
}
