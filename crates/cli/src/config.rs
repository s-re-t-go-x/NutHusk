//! 从本地发布文件生成插件源索引和运行时配方，不下载或执行发布文件。
use nuthusk_core::{package, platform_key, Recipe, TrustLevel};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use url::Url;

use super::flag;

fn required(args: &[String], name: &str) -> Result<String, String> {
    flag(args, name)
        .filter(|s| !s.starts_with("--") && !s.trim().is_empty())
        .ok_or_else(|| format!("缺少 {name} <值>"))
}

fn https_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw).map_err(|e| format!("地址无效: {e}"))?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("只允许带域名的 https 地址".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("地址里不能包含用户名或密码".into());
    }
    Ok(url)
}

fn write_json(path: &Path, value: &Value, force: bool) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建输出目录: {e}"))?;
    }
    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options.open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            format!("{} 已存在；确认重新生成后使用 --force", path.display())
        } else {
            format!("无法写入 {}: {e}", path.display())
        }
    })?;
    file.write_all(&bytes)
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|e| format!("写入失败: {e}"))
}

pub fn index(args: &[String]) -> Result<(), String> {
    let dir = PathBuf::from(args.get(1).filter(|s| !s.starts_with("--"))
        .ok_or("用法: nuthusk-cli index <插件包目录> --base-url <下载目录URL> [--name <源名称>] [--out index.json] [--force]")?);
    let mut base = https_url(&required(args, "--base-url")?)?;
    if base.query().is_some() || base.fragment().is_some() {
        return Err("--base-url 应为下载目录地址，不能带查询参数或片段".into());
    }
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| format!("无法读取插件包目录: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            continue;
        }
        let path = entry.path();
        if path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| ["nut", "zip", "mytool"].contains(&s.to_ascii_lowercase().as_str()))
            .unwrap_or(false)
        {
            files.push(path);
        }
    }
    files.sort();
    if files.is_empty() {
        return Err("目录里没有 .nut / .zip / .mytool 插件包".into());
    }
    if files.len() > 500 {
        return Err("索引最多支持 500 个插件".into());
    }
    let mut ids = HashSet::new();
    let mut entries = Vec::new();
    for path in files {
        let info = package::inspect(&path, &[]).map_err(|e| format!("{}: {e}", path.display()))?;
        if info.trust.level == TrustLevel::Invalid {
            return Err(format!(
                "{} 的签名无效: {}",
                path.display(),
                info.trust.reason
            ));
        }
        let m = info.manifest;
        if !ids.insert(m.id.clone()) {
            return Err(format!(
                "重复的插件 id: {}；每个插件只保留一个待发布包（签名后请移走原包）",
                m.id
            ));
        }
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("插件包文件名不是 UTF-8")?;
        let mut url = base.clone();
        url.path_segments_mut()
            .map_err(|_| "下载地址不能作为目录")?
            .pop_if_empty()
            .push(filename);
        let mut entry = json!({
            "id": m.id, "name": m.name, "version": m.version,
            "description": m.description, "category": m.category,
            "url": url.as_str(), "sha256": info.sha256, "size": info.package_size,
        });
        if let Some(author) = flag(args, "--author") {
            entry["author"] = json!(author);
        }
        entries.push(entry);
    }
    let out = PathBuf::from(flag(args, "--out").unwrap_or_else(|| "index.json".into()));
    write_json(
        &out,
        &json!({
            "schemaVersion": 1,
            "name": flag(args, "--name").unwrap_or_else(|| "NutHusk 插件源".into()),
            "plugins": entries,
        }),
        args.iter().any(|s| s == "--force"),
    )?;
    println!(
        "已生成：{}（{} 个插件）。发布前如需签名，请运行 sign-index。",
        out.display(),
        entries.len()
    );
    Ok(())
}

fn list(args: &[String], name: &str) -> Vec<String> {
    flag(args, name)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

pub fn runtime_recipe(args: &[String]) -> Result<(), String> {
    let name = args.get(1).filter(|s| !s.starts_with("--"))
        .ok_or("用法: nuthusk-cli runtime-recipe <名字> --version <版本> --file <本地安装包> --url <下载URL> --bin <可执行文件相对路径> [--platform <系统-架构>] [--out <配方.json>]")?;
    let file = PathBuf::from(required(args, "--file")?);
    let url = required(args, "--url")?;
    https_url(&url)?;
    let version = required(args, "--version")?;
    let bin = required(args, "--bin")?;
    let platform = flag(args, "--platform").unwrap_or_else(platform_key);
    let (os, arch) = platform
        .split_once('-')
        .ok_or("平台应形如 windows-x86_64")?;
    if !["windows", "linux", "macos"].contains(&os) || !["x86_64", "aarch64"].contains(&arch) {
        return Err("平台应为 windows/linux/macos 与 x86_64/aarch64 的组合".into());
    }
    let filename = file.to_string_lossy().to_ascii_lowercase();
    let archive = flag(args, "--archive").unwrap_or_else(|| {
        if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
            "tar.gz"
        } else if filename.ends_with(".zip") {
            "zip"
        } else {
            "file"
        }
        .into()
    });
    let strip: usize = flag(args, "--strip-components")
        .unwrap_or_else(|| "0".into())
        .parse()
        .map_err(|_| "--strip-components 必须为非负整数")?;
    let value = json!({
        "name": name, "version": version,
        "description": flag(args, "--description").unwrap_or_default(),
        "aliases": list(args, "--aliases"),
        "platforms": { (platform): {
            "url": url,
            "sha256": package::sha256_file(&file)?,
            "size": fs::metadata(&file).map_err(|e| e.to_string())?.len(),
            "archive": archive, "bin": bin, "stripComponents": strip,
            "keep": list(args, "--keep"),
        }},
    });
    let recipe: Recipe =
        serde_json::from_value(value.clone()).map_err(|e| format!("配方无效: {e}"))?;
    recipe.validate()?;
    let out = PathBuf::from(flag(args, "--out").unwrap_or_else(|| format!("{name}.json")));
    write_json(&out, &value, args.iter().any(|s| s == "--force"))?;
    println!(
        "已生成：{}。请放入 nutConfig/runtimes.d/；下载URL必须提供与本地文件完全相同的内容。",
        out.display()
    );
    Ok(())
}
