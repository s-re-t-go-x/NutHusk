//! 运行时（解释器）管理。
//!
//! 一个运行时由一份"配方"描述：下载地址、SHA-256、压缩包格式、可执行文件位置、需要保留的文件。
//! 通用的 下载 → 校验 → 解压（精简）→ 原子替换 流程只有一份，所以不需要为每种语言写代码：
//! 内置配方随程序发布（Python、Node），用户可以在配置目录的 `runtimes.d/*.json` 里追加自己的配方（Lua、AHK、Bun……）。
//!
//! 运行时装在 `<根>/nutRuntimes/<名字>/<版本>/`，同名运行时只保留一个版本。
//! 启动插件后端时：已装的运行时优先 → 系统 PATH → 都没有而又有配方，就返回 `RuntimeMissing`，由界面提示下载。

use crate::http;
use crate::layout::Roots;
use crate::package::command_on_path;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// 旧客户端使用的错误前缀；宿主 IPC 已改用结构化 InvokeError，不再解析此字符串。
pub const MISSING_PREFIX: &str = "RuntimeMissing:";

const BUILTIN_RECIPES: &str = include_str!("runtimes.json");
const META_FILE: &str = "runtime.json";
const MAX_DOWNLOAD_BYTES: u64 = 200 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 600 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 300 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Archive {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.gz")]
    TarGz,
    /// 下载下来的就是可执行文件本身（如 bun、deno、AutoHotkey 的单个 exe）。
    #[serde(rename = "file")]
    File,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRecipe {
    pub url: String,
    pub sha256: String,
    pub archive: Archive,
    /// 解压后可执行文件的相对路径（`file` 格式时是保存成的文件名）。
    pub bin: String,
    /// 去掉压缩包最外层的几级目录。
    #[serde(default)]
    pub strip_components: usize,
    /// 只保留这些文件（精简体积）：精确路径、`目录/*` 前缀，或 `*`。留空表示全部保留。
    #[serde(default)]
    pub keep: Vec<String>,
    /// 额外加入 PATH 的目录（相对路径）。留空时用可执行文件所在目录。
    #[serde(default)]
    pub path_dirs: Vec<String>,
    /// 下载大小（字节），只用于在提示里显示。
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub name: String,
    pub version: String,
    /// 其他可执行文件名，例如 python 的 python3。
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub description: String,
    /// 键是 `系统-架构`，如 `windows-x86_64`、`linux-aarch64`、`macos-aarch64`。
    pub platforms: BTreeMap<String, PlatformRecipe>,
    /// 是否来自用户追加的配方（内置配方为 false）。
    #[serde(skip_deserializing, default)]
    pub user: bool,
}

/// 当前系统的配方键，如 `windows-x86_64`。
pub fn platform_key() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn safe_rel(p: &str) -> bool {
    !p.contains('\\') && crate::manifest::is_safe_relative(p)
}

fn simple_token(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

impl Recipe {
    pub fn validate(&self) -> Result<(), String> {
        let ctx = |m: String| format!("运行时配方「{}」无效: {m}", self.name);
        if !simple_token(&self.name) || self.name.starts_with('.') || !safe_rel(&self.name) {
            return Err(ctx("名字只能包含字母、数字、. - _ +".into()));
        }
        if !simple_token(&self.version) || !safe_rel(&self.version) {
            return Err(ctx("版本号只能包含字母、数字、. - _ +".into()));
        }
        if self.aliases.iter().any(|a| !simple_token(a)) {
            return Err(ctx("别名格式不对".into()));
        }
        if self.platforms.is_empty() {
            return Err(ctx("没有任何平台".into()));
        }
        for (key, p) in &self.platforms {
            let c = |m: &str| ctx(format!("{key}: {m}"));
            http::check_url(&p.url, None).map_err(|e| c(&e))?;
            if p.sha256.len() != 64 || !p.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(c("sha256 应为 64 位十六进制"));
            }
            if !safe_rel(&p.bin) {
                return Err(c("bin 必须是安全的相对路径"));
            }
            if p.keep.iter().any(|k| k != "*" && !safe_rel(k.trim_end_matches("/*"))) {
                return Err(c("keep 含不安全的路径"));
            }
            if p.path_dirs.iter().any(|d| !(d == "." || safe_rel(d))) {
                return Err(c("pathDirs 含不安全的路径"));
            }
            if p.archive == Archive::File && p.bin.contains('/') {
                return Err(c("file 格式的 bin 只能是文件名"));
            }
        }
        Ok(())
    }

    pub fn platform(&self) -> Option<&PlatformRecipe> {
        self.platforms.get(&platform_key())
    }

    fn matches_command(&self, stem: &str) -> bool {
        self.name.eq_ignore_ascii_case(stem) || self.aliases.iter().any(|a| a.eq_ignore_ascii_case(stem))
    }
}

/// 可执行文件名去掉目录和 .exe 后缀，用于和配方的名字/别名比较。
fn command_stem(command: &str) -> String {
    let base = command.rsplit(['/', '\\']).next().unwrap_or(command).to_ascii_lowercase();
    base.strip_suffix(".exe").map(String::from).unwrap_or(base)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledRuntime {
    pub name: String,
    pub version: String,
    /// 可执行文件的完整路径。
    pub bin: PathBuf,
    /// 启动后端时加到 PATH 最前面的目录。
    pub path_dirs: Vec<PathBuf>,
    /// 所在的运行时目录（`.../nutRuntimes`）。
    pub location: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    name: String,
    version: String,
    bin: String,
    path_dirs: Vec<String>,
}

/// 由宿主运行时解析生成的缺失信息，作为结构化错误的 detail 传递。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMissing {
    pub name: String,
    pub version: String,
    pub command: String,
    /// 当前系统有没有可下载的配方。false 时只能请用户自行安装。
    pub downloadable: bool,
    pub size: Option<u64>,
    /// 下载来源域名。
    pub source: Option<String>,
    pub user_recipe: bool,
}

/// 启动后端前对命令的处理结果。
pub enum Prepared {
    /// 使用已安装的运行时。
    Managed { program: PathBuf, path_dirs: Vec<PathBuf> },
    /// 按原样启动（系统 PATH 里有，或不是已知的运行时）。
    System,
    Missing(RuntimeMissing),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub name: String,
    pub version: String,
    pub description: String,
    pub aliases: Vec<String>,
    pub user_recipe: bool,
    pub downloadable: bool,
    pub size: Option<u64>,
    pub source: Option<String>,
    pub installed: Option<InstalledRuntime>,
    /// 系统 PATH 里是否已经有这个程序（没装运行时时才有意义）。
    pub on_system: bool,
}

pub struct RuntimeManager {
    roots: Arc<Roots>,
    builtin: Vec<Recipe>,
}

fn parse_recipes(text: &str) -> Result<Vec<Recipe>, String> {
    #[derive(Deserialize)]
    struct File {
        recipes: Vec<Recipe>,
    }
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let recipes: Vec<Recipe> = if v.get("recipes").is_some() {
        serde_json::from_value::<File>(v).map_err(|e| e.to_string())?.recipes
    } else {
        vec![serde_json::from_value::<Recipe>(v).map_err(|e| e.to_string())?]
    };
    for r in &recipes {
        r.validate()?;
    }
    Ok(recipes)
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(String::from))
}

impl RuntimeManager {
    pub fn new(roots: Arc<Roots>) -> Self {
        let builtin = parse_recipes(BUILTIN_RECIPES).expect("内置运行时配方无效");
        Self { roots, builtin }
    }

    /// 内置配方 + 用户配方（`<配置目录>/runtimes.d/*.json`）。同名时用户配方优先；无效的用户配方被忽略。
    pub fn recipes(&self) -> Vec<Recipe> {
        let mut out: Vec<Recipe> = Vec::new();
        for dir in self.roots.config_search() {
            let Ok(read) = fs::read_dir(dir.join("runtimes.d")) else { continue };
            let mut files: Vec<_> = read.flatten().map(|e| e.path()).collect();
            files.sort();
            for f in files {
                if f.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&f) else { continue };
                if let Ok(rs) = parse_recipes(&text) {
                    for mut r in rs {
                        r.user = true;
                        if !out.iter().any(|o| o.name == r.name) {
                            out.push(r);
                        }
                    }
                }
            }
        }
        for b in &self.builtin {
            if !out.iter().any(|o| o.name == b.name) {
                out.push(b.clone());
            }
        }
        out
    }

    fn recipe_for(&self, command: &str) -> Option<Recipe> {
        let stem = command_stem(command);
        self.recipes().into_iter().find(|r| r.matches_command(&stem))
    }

    /// 扫描所有运行时目录里已安装的运行时。同名的取优先级靠前的目录里的。
    pub fn installed(&self) -> Vec<InstalledRuntime> {
        let mut out: Vec<InstalledRuntime> = Vec::new();
        for base in self.roots.runtime_search() {
            let Ok(names) = fs::read_dir(&base) else { continue };
            let mut names: Vec<_> = names.flatten().collect();
            names.sort_by_key(|e| e.file_name());
            for n in names {
                if n.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                let Ok(versions) = fs::read_dir(n.path()) else { continue };
                let mut versions: Vec<_> = versions.flatten().collect();
                versions.sort_by_key(|e| e.file_name());
                // 同一目录里取版本号排序最后的一个
                for v in versions.iter().rev() {
                    let dir = v.path();
                    let Ok(text) = fs::read_to_string(dir.join(META_FILE)) else { continue };
                    let Ok(meta) = serde_json::from_str::<Meta>(&text) else { continue };
                    if !safe_rel(&meta.bin) || meta.path_dirs.iter().any(|d| !(d == "." || safe_rel(d))) {
                        continue;
                    }
                    let bin = dir.join(&meta.bin);
                    if !bin.is_file() || out.iter().any(|o| o.name == meta.name) {
                        continue;
                    }
                    let path_dirs = meta
                        .path_dirs
                        .iter()
                        .map(|d| if d == "." { dir.clone() } else { dir.join(d) })
                        .collect();
                    out.push(InstalledRuntime {
                        name: meta.name,
                        version: meta.version,
                        bin,
                        path_dirs,
                        location: base.clone(),
                    });
                    break;
                }
            }
        }
        out
    }

    /// 命令对应的已安装运行时（按名字或别名匹配；带路径的命令不匹配）。
    pub fn resolve(&self, command: &str) -> Option<InstalledRuntime> {
        if command.contains('/') || command.contains('\\') {
            return None;
        }
        let stem = command_stem(command);
        let recipes = self.recipes();
        let name = recipes.iter().find(|r| r.matches_command(&stem)).map(|r| r.name.clone()).unwrap_or(stem);
        self.installed().into_iter().find(|i| i.name == name)
    }

    /// 命令是否有可用配方（用于在导入确认框里提示"首次使用时可自动下载"）。
    pub fn has_recipe(&self, command: &str) -> bool {
        !command.contains('/') && !command.contains('\\') && self.recipe_for(command).is_some()
    }

    pub fn prepare(&self, command: &str) -> Prepared {
        if let Some(i) = self.resolve(command) {
            return Prepared::Managed { program: i.bin, path_dirs: i.path_dirs };
        }
        if command_on_path(command) {
            return Prepared::System;
        }
        match self.recipe_for(command) {
            Some(r) => {
                let p = r.platform();
                Prepared::Missing(RuntimeMissing {
                    name: r.name.clone(),
                    version: r.version.clone(),
                    command: command.to_string(),
                    downloadable: p.is_some(),
                    size: p.and_then(|p| p.size),
                    source: p.and_then(|p| host_of(&p.url)),
                    user_recipe: r.user,
                })
            }
            None => Prepared::System,
        }
    }

    pub fn status(&self) -> Vec<RuntimeStatus> {
        let installed = self.installed();
        self.recipes()
            .into_iter()
            .map(|r| {
                let p = r.platform();
                RuntimeStatus {
                    installed: installed.iter().find(|i| i.name == r.name).cloned(),
                    on_system: std::iter::once(r.name.as_str())
                        .chain(r.aliases.iter().map(String::as_str))
                        .any(command_on_path),
                    downloadable: p.is_some(),
                    size: p.and_then(|p| p.size),
                    source: p.and_then(|p| host_of(&p.url)),
                    name: r.name,
                    version: r.version,
                    description: r.description,
                    aliases: r.aliases,
                    user_recipe: r.user,
                }
            })
            .collect()
    }

    /// 下载并安装名为 `name` 的运行时（阻塞，请放进 `spawn_blocking`）。
    pub fn install(
        &self,
        name: &str,
        progress: &mut dyn FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> Result<InstalledRuntime, String> {
        let recipe = self
            .recipes()
            .into_iter()
            .find(|r| r.name == name)
            .ok_or_else(|| format!("没有「{name}」的运行时配方"))?;
        let plat = recipe
            .platform()
            .cloned()
            .ok_or_else(|| format!("「{name}」没有适用于当前系统（{}）的下载，请自行安装并加入 PATH", platform_key()))?;
        let base = self.roots.runtime_install_dir()?;
        fs::create_dir_all(&base).map_err(|e| format!("无法创建运行时目录: {e}"))?;

        let download = base.join(format!(".download-{name}.tmp"));
        let allowed = host_of(&plat.url).map(|h| vec![h]);
        let sha = http::download(&plat.url, &download, MAX_DOWNLOAD_BYTES, allowed.as_deref(), progress, cancel)?;
        let result = install_from_file(&base, &recipe, &plat, &download, &sha);
        let _ = fs::remove_file(&download);
        result
    }

    /// 删除运行时（所有可写目录里同名的都删）。
    pub fn remove(&self, name: &str) -> Result<(), String> {
        if !simple_token(name) || name.starts_with('.') {
            return Err("运行时名字无效".into());
        }
        let mut removed = false;
        for base in self.roots.runtime_search() {
            let dir = base.join(name);
            if dir.is_dir() {
                fs::remove_dir_all(&dir).map_err(|e| format!("删除失败（可能没有权限，或文件被占用）: {e}"))?;
                removed = true;
            }
        }
        if removed {
            Ok(())
        } else {
            Err("没有找到这个运行时".into())
        }
    }
}

/* ---------- 校验、解压、落地（不涉及网络，可单独测试） ---------- */

/// 校验哈希 → 解压到临时目录 → 写元数据 → 替换到位。`base` 是运行时目录。
pub(crate) fn install_from_file(
    base: &Path,
    recipe: &Recipe,
    plat: &PlatformRecipe,
    archive: &Path,
    actual_sha256: &str,
) -> Result<InstalledRuntime, String> {
    if !actual_sha256.eq_ignore_ascii_case(&plat.sha256) {
        return Err("下载内容的 SHA-256 与配方不符，已拒绝安装（文件可能被篡改或配方过期）".into());
    }
    let staging = base.join(format!(".installing-{}", recipe.name));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| format!("无法创建临时目录: {e}"))?;

    let built = (|| {
        match plat.archive {
            Archive::Zip => extract_zip(archive, &staging, plat)?,
            Archive::TarGz => extract_tar_gz(archive, &staging, plat)?,
            Archive::File => {
                let out = staging.join(&plat.bin);
                fs::copy(archive, &out).map_err(|e| format!("无法保存文件: {e}"))?;
                set_executable(&out, Some(0o755));
            }
        }
        let bin = staging.join(&plat.bin);
        if !bin.is_file() {
            return Err(format!("解压后找不到可执行文件: {}", plat.bin));
        }
        let path_dirs: Vec<String> = if plat.path_dirs.is_empty() {
            vec![Path::new(&plat.bin)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| ".".into())]
        } else {
            plat.path_dirs.clone()
        };
        let meta = Meta { name: recipe.name.clone(), version: recipe.version.clone(), bin: plat.bin.clone(), path_dirs };
        fs::write(staging.join(META_FILE), serde_json::to_string_pretty(&meta).unwrap_or_default())
            .map_err(|e| format!("无法写入运行时信息: {e}"))?;
        Ok(())
    })();
    if let Err(e) = built {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }

    // 同名运行时只保留一个版本：先清掉旧的，再把新目录改名到位
    let name_dir = base.join(&recipe.name);
    let backup = base.join(format!(".backup-{}", recipe.name));
    let _ = fs::remove_dir_all(&backup);
    if name_dir.exists() {
        fs::rename(&name_dir, &backup).map_err(|e| {
            let _ = fs::remove_dir_all(&staging);
            format!("无法替换已安装的运行时（可能正被使用）: {e}")
        })?;
    }
    let final_dir = name_dir.join(&recipe.version);
    let moved = fs::create_dir_all(&name_dir).and_then(|_| fs::rename(&staging, &final_dir));
    if let Err(e) = moved {
        let _ = fs::remove_dir_all(&name_dir);
        if backup.exists() {
            let _ = fs::rename(&backup, &name_dir);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("安装失败: {e}"));
    }
    let _ = fs::remove_dir_all(&backup);

    let meta_text = fs::read_to_string(final_dir.join(META_FILE)).map_err(|e| e.to_string())?;
    let meta: Meta = serde_json::from_str(&meta_text).map_err(|e| e.to_string())?;
    Ok(InstalledRuntime {
        name: meta.name,
        version: meta.version,
        bin: final_dir.join(&meta.bin),
        path_dirs: meta
            .path_dirs
            .iter()
            .map(|d| if d == "." { final_dir.clone() } else { final_dir.join(d) })
            .collect(),
        location: base.to_path_buf(),
    })
}

fn kept(keep: &[String], rel: &str) -> bool {
    keep.is_empty()
        || keep.iter().any(|k| {
            k == "*" || k == rel || (k.ends_with("/*") && rel.starts_with(&k[..k.len() - 1]))
        })
}

/// 去掉最外层 `n` 级目录；条目路径含 `..`、绝对路径时返回错误；层数不够时返回 None（跳过）。
fn stripped(raw: &Path, n: usize) -> Result<Option<String>, String> {
    let mut parts: Vec<String> = Vec::new();
    for c in raw.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return Err(format!("压缩包含不安全的路径: {}", raw.display())),
        }
    }
    if parts.len() <= n {
        return Ok(None);
    }
    let rel = parts[n..].join("/");
    if !safe_rel(&rel) { return Err(format!("压缩包含不安全的路径: {}", raw.display())); }
    Ok(Some(rel))
}

fn write_entry(reader: &mut impl Read, out: &Path, total: &mut u64) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut w = File::create(out).map_err(|e| format!("无法写入 {}: {e}", out.display()))?;
    // 按实际读出的字节限流，不信任压缩包里声明的大小
    let n = io::copy(&mut reader.take(MAX_ENTRY_BYTES + 1), &mut w).map_err(|e| e.to_string())?;
    if n > MAX_ENTRY_BYTES {
        return Err("压缩包内文件过大".into());
    }
    *total += n;
    if *total > MAX_UNPACKED_BYTES {
        return Err("解压后体积过大".into());
    }
    Ok(())
}

#[cfg(unix)]
fn set_executable(path: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode.unwrap_or(0o755) & 0o777));
}
#[cfg(not(unix))]
fn set_executable(_path: &Path, _mode: Option<u32>) {}

fn extract_zip(src: &Path, dest: &Path, p: &PlatformRecipe) -> Result<(), String> {
    let file = File::open(src).map_err(|e| e.to_string())?;
    let mut ar = zip::ZipArchive::new(file).map_err(|e| format!("不是有效的 zip 压缩包: {e}"))?;
    let mut total = 0u64;
    for i in 0..ar.len() {
        let mut f = ar.by_index(i).map_err(|e| e.to_string())?;
        let entry_name = f.name().map_err(|e| format!("压缩包文件名无法解析: {e}"))?.into_owned();
        if !safe_rel(entry_name.trim_end_matches('/')) {
            return Err(format!("压缩包含不安全的路径: {entry_name}"));
        }
        let name = f.enclosed_name().ok_or_else(|| format!("压缩包含不安全的路径: {entry_name}"))?;
        let Some(rel) = stripped(&name, p.strip_components)? else { continue };
        if f.is_dir() || rel.is_empty() || !kept(&p.keep, &rel) {
            continue;
        }
        if f.unix_mode().map(|m| m & 0o170000 == 0o120000).unwrap_or(false) {
            return Err(format!("压缩包含符号链接: {rel}"));
        }
        let mode = f.unix_mode();
        let out = dest.join(&rel);
        write_entry(&mut f, &out, &mut total)?;
        set_executable(&out, mode);
    }
    Ok(())
}

fn extract_tar_gz(src: &Path, dest: &Path, p: &PlatformRecipe) -> Result<(), String> {
    let file = File::open(src).map_err(|e| e.to_string())?;
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut total = 0u64;
    for entry in ar.entries().map_err(|e| format!("不是有效的 tar.gz 压缩包: {e}"))? {
        let mut e = entry.map_err(|e| e.to_string())?;
        let path = e.path().map_err(|e| e.to_string())?.into_owned();
        let Some(rel) = stripped(&path, p.strip_components)? else { continue };
        let ty = e.header().entry_type();
        if ty.is_dir() || rel.is_empty() || !kept(&p.keep, &rel) {
            continue; // 不需要的条目（包括 npm 之类的符号链接）直接跳过
        }
        if !ty.is_file() {
            return Err(format!("压缩包含不支持的条目（符号链接等）: {rel}"));
        }
        let mode = e.header().mode().ok();
        let out = dest.join(&rel);
        write_entry(&mut e, &out, &mut total)?;
        set_executable(&out, mode);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nuthusk-rt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn recipe(archive: Archive, bin: &str, strip: usize, keep: &[&str]) -> (Recipe, PlatformRecipe) {
        let plat = PlatformRecipe {
            url: "https://example.com/x".into(),
            sha256: "0".repeat(64),
            archive,
            bin: bin.into(),
            strip_components: strip,
            keep: keep.iter().map(|s| s.to_string()).collect(),
            path_dirs: vec![],
            size: None,
        };
        let r = Recipe {
            name: "demo".into(),
            version: "1.0.0".into(),
            aliases: vec![],
            description: String::new(),
            platforms: BTreeMap::from([(platform_key(), plat.clone())]),
            user: false,
        };
        (r, plat)
    }

    fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut w = zip::ZipWriter::new(File::create(path).unwrap());
        for (n, c) in files {
            w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(c).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn builtin_recipes_are_valid_and_cover_python_windows() {
        let m = RuntimeManager::new(Arc::new(Roots::new(vec![], PathBuf::from("none"))));
        let rs = m.recipes();
        let py = rs.iter().find(|r| r.name == "python").unwrap();
        assert!(py.aliases.contains(&"python3".to_string()));
        assert!(py.platforms.contains_key("windows-x86_64"));
        assert!(!py.platforms.keys().any(|k| k.starts_with("linux") || k.starts_with("macos")), "Python 只在 Windows 自动补全");
        assert!(rs.iter().any(|r| r.name == "node" && r.platforms.contains_key("linux-x86_64")));
    }

    #[test]
    fn recipe_validation_rejects_bad_input() {
        let (mut r, _) = recipe(Archive::Zip, "a.exe", 0, &[]);
        assert!(r.validate().is_ok());
        r.name = "../evil".into();
        assert!(r.validate().is_err());
        let (mut bad_version, _) = recipe(Archive::Zip, "a.exe", 0, &[]);
        bad_version.version = "..".into();
        assert!(bad_version.validate().is_err());
        let (mut r, _) = recipe(Archive::Zip, "../a.exe", 0, &[]);
        assert!(r.validate().is_err());
        r.platforms.values_mut().for_each(|p| p.bin = "a.exe".into());
        r.platforms.values_mut().for_each(|p| p.url = "http://example.com/x".into());
        assert!(r.validate().is_err(), "只允许 https");
        r.platforms.values_mut().for_each(|p| {
            p.url = "https://example.com/x".into();
            p.sha256 = "zz".into();
        });
        assert!(r.validate().is_err());
    }

    #[test]
    fn command_matching_uses_name_and_aliases() {
        assert_eq!(command_stem("python3"), "python3");
        assert_eq!(command_stem("C:\\x\\Node.EXE"), "node");
        let m = RuntimeManager::new(Arc::new(Roots::new(vec![], PathBuf::from("none"))));
        assert!(m.has_recipe("python3") && m.has_recipe("node") && m.has_recipe("python.exe"));
        assert!(!m.has_recipe("./main.exe") && !m.has_recipe("lua"));
    }

    #[test]
    fn install_strips_components_keeps_only_listed_files_and_replaces_old_version() {
        let root = tmp("install");
        let zip_path = root.join("a.zip");
        make_zip(
            &zip_path,
            &[("top/demo.exe", b"BIN"), ("top/LICENSE", b"L"), ("top/npm/big.js", b"NO"), ("top/docs/readme.md", b"NO")],
        );
        let (r, plat) = recipe(Archive::Zip, "demo.exe", 1, &["demo.exe", "LICENSE"]);
        let base = root.join("nutRuntimes");
        let got = install_from_file(&base, &r, &plat, &zip_path, &plat.sha256).unwrap();
        assert_eq!(fs::read(&got.bin).unwrap(), b"BIN");
        assert!(got.bin.ends_with("demo/1.0.0/demo.exe"));
        assert!(base.join("demo/1.0.0/LICENSE").is_file());
        assert!(!base.join("demo/1.0.0/npm").exists() && !base.join("demo/1.0.0/docs").exists(), "精简：不在 keep 里的不解压");

        // 新版本替换旧版本，只保留一个
        let (mut r2, plat2) = recipe(Archive::Zip, "demo.exe", 1, &["demo.exe"]);
        r2.version = "2.0.0".into();
        install_from_file(&base, &r2, &plat2, &zip_path, &plat2.sha256).unwrap();
        assert!(base.join("demo/2.0.0/demo.exe").is_file() && !base.join("demo/1.0.0").exists());
        assert!(fs::read_dir(&base).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with('.')), "不留临时目录");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn install_rejects_hash_mismatch_and_missing_binary_and_traversal() {
        let root = tmp("reject");
        let base = root.join("nutRuntimes");
        let zip_path = root.join("a.zip");
        make_zip(&zip_path, &[("demo.exe", b"BIN")]);
        let (r, plat) = recipe(Archive::Zip, "demo.exe", 0, &[]);
        let e = install_from_file(&base, &r, &plat, &zip_path, &"1".repeat(64)).unwrap_err();
        assert!(e.contains("SHA-256"));
        assert!(!base.join("demo").exists(), "校验失败什么都不落地");

        let (r, plat) = recipe(Archive::Zip, "missing.exe", 0, &[]);
        assert!(install_from_file(&base, &r, &plat, &zip_path, &plat.sha256).unwrap_err().contains("找不到可执行文件"));
        assert!(!base.join("demo").exists() && !base.join(".installing-demo").exists());

        let evil = root.join("evil.zip");
        make_zip(&evil, &[("../escape.exe", b"X")]);
        let (r, plat) = recipe(Archive::Zip, "escape.exe", 0, &[]);
        assert!(install_from_file(&base, &r, &plat, &evil, &plat.sha256).is_err());
        assert!(!root.join("escape.exe").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tar_gz_skips_unneeded_symlinks_but_rejects_kept_ones() {
        let root = tmp("targz");
        let base = root.join("nutRuntimes");
        let tgz = root.join("a.tar.gz");
        {
            let enc = flate2::write::GzEncoder::new(File::create(&tgz).unwrap(), flate2::Compression::fast());
            let mut b = tar::Builder::new(enc);
            let mut h = tar::Header::new_gnu();
            h.set_size(3);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, "node-v1/bin/node", &b"BIN"[..]).unwrap();
            let mut l = tar::Header::new_gnu();
            l.set_entry_type(tar::EntryType::Symlink);
            l.set_size(0);
            b.append_link(&mut l, "node-v1/bin/npm", "../lib/npm").unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let (r, plat) = recipe(Archive::TarGz, "bin/node", 1, &["bin/node"]);
        let got = install_from_file(&base, &r, &plat, &tgz, &plat.sha256).unwrap();
        assert_eq!(fs::read(&got.bin).unwrap(), b"BIN");
        assert!(!base.join("demo/1.0.0/bin/npm").exists());
        assert_eq!(got.path_dirs, vec![base.join("demo/1.0.0/bin")]);

        let (r, plat) = recipe(Archive::TarGz, "bin/node", 1, &[]);
        let e = install_from_file(&base, &r, &plat, &tgz, &plat.sha256).unwrap_err();
        assert!(e.contains("符号链接"), "{e}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn single_file_runtime_and_resolve_prefer_installed_over_path() {
        let root = tmp("file");
        let roots = Arc::new(Roots::new(
            vec![crate::layout::DataRoot { key: "a".into(), label: "A".into(), bases: vec![root.clone()] }],
            root.join("order.json"),
        ));
        let m = RuntimeManager::new(roots);
        // 用户追加一份"file"格式的配方
        let dir = root.join("nutConfig/runtimes.d");
        fs::create_dir_all(&dir).unwrap();
        let src = root.join("tool.bin");
        fs::write(&src, b"EXE").unwrap();
        let sha = {
            let f = File::open(&src).unwrap();
            crate::signature::hash_reader(f).unwrap().iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        let recipe_json = format!(
            r#"{{"name":"mytool","version":"0.1.0","platforms":{{"{}":{{"url":"https://example.com/t","sha256":"{sha}","archive":"file","bin":"mytool.exe"}}}}}}"#,
            platform_key()
        );
        fs::write(dir.join("mytool.json"), recipe_json).unwrap();
        assert!(m.recipes().iter().any(|r| r.name == "mytool" && r.user), "用户配方被读取并标记");
        assert!(matches!(m.prepare("mytool"), Prepared::Missing(ref x) if x.downloadable && x.user_recipe));

        let r = m.recipes().into_iter().find(|r| r.name == "mytool").unwrap();
        let plat = r.platform().unwrap().clone();
        install_from_file(&root.join("nutRuntimes"), &r, &plat, &src, &sha).unwrap();
        assert!(matches!(m.prepare("mytool"), Prepared::Managed { .. }));
        assert!(m.resolve("./mytool").is_none(), "带路径的命令不走运行时解析");
        m.remove("mytool").unwrap();
        assert!(matches!(m.prepare("mytool"), Prepared::Missing(_)));
        let _ = fs::remove_dir_all(&root);
    }
}
