//! 插件下载（插件市场）。
//!
//! - **官方源**：固定的 GitHub 仓库里的 `index.json`（旁边有 `index.json.sig`），必须由受信任的密钥签名，
//!   条目的下载地址限制在 GitHub 域名内，并且必须带 SHA-256。
//! - **私有源 / 直接地址**：用户手动粘贴。只允许 https；私有源的索引签名是可选的（没有签名就标注"未经安全验证"），
//!   签名存在但对不上的一律拒绝。
//!
//! 这里只负责取索引、下载插件包到临时位置；之后走和"导入"完全相同的检查、确认、安装流程
//! （`inspect_package` / `install_package`），签名校验、防路径穿越、回滚都不用重做。
//! 函数是阻塞的，在异步环境里请放进 `spawn_blocking`。

use crate::http;
use crate::layout::Roots;
use crate::package::MAX_PACKAGE_BYTES;
use crate::signature::{self, hash_reader, Trust, TrustLevel, TrustedKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

pub const OFFICIAL_ID: &str = "official";
// 与 runtimes.json 一样随程序编译，用户源不能修改官方信任策略。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OfficialSourceConfig {
    name: String,
    url: String,
    allowed_hosts: Vec<String>,
}

fn official_config() -> &'static OfficialSourceConfig {
    static CONFIG: OnceLock<OfficialSourceConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        serde_json::from_str(include_str!("market.json")).expect("内置 market.json 格式无效")
    })
}

const MAX_INDEX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 500;
const SOURCES_FILE: &str = "sources.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default, skip_deserializing)]
    pub official: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    /// 插件包（.nut）的下载地址。
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexResult {
    pub source: Source,
    pub name: String,
    pub entries: Vec<IndexEntry>,
    pub trust: Trust,
    /// 网络不可用时显示的是上次缓存的内容。
    pub from_cache: bool,
    /// 使用缓存时，这里写着网络错误的原因。
    pub error: Option<String>,
}

fn official_hosts() -> &'static [String] {
    &official_config().allowed_hosts
}

fn simple_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn short_hash(text: &str) -> String {
    hash_reader(text.as_bytes())
        .map(|h| h.iter().take(4).map(|b| format!("{b:02x}")).collect())
        .unwrap_or_default()
}

/// 校验并解析索引内容，不涉及网络。同时兼容 `plugins` 与 `entries` 两种列表字段名。
pub(crate) fn process_index(
    source: &Source,
    bytes: &[u8],
    sig: Option<&str>,
    trusted: &[TrustedKey],
) -> Result<(String, Vec<IndexEntry>, Trust), String> {
    let trust = signature::verify_index(sig, bytes, trusted);
    if trust.level == TrustLevel::Invalid {
        return Err(format!(
            "插件源的签名校验失败，已拒绝使用：{}",
            trust.reason
        ));
    }
    if source.official && trust.level != TrustLevel::Trusted {
        return Err(format!(
            "官方插件源没有通过签名校验，已拒绝使用：{}",
            trust.reason
        ));
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("索引无法解析: {e}"))?;
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let raw = v
        .get("plugins")
        .or_else(|| v.get("entries"))
        .and_then(Value::as_array)
        .ok_or("索引里缺少 plugins 列表")?;
    if raw.len() > MAX_ENTRIES {
        return Err(format!("索引条目过多（超过 {MAX_ENTRIES} 个）"));
    }
    let hosts = official_hosts();
    let mut entries = Vec::new();
    for item in raw {
        let e: IndexEntry = serde_json::from_value(item.clone())
            .map_err(|e| format!("索引里有无法识别的条目: {e}"))?;
        if !simple_id(&e.id) {
            return Err(format!("索引里的插件 id 不合法: {}", e.id));
        }
        http::check_url(&e.url, if source.official { Some(hosts) } else { None })
            .map_err(|err| format!("「{}」的下载地址不可用: {err}", e.id))?;
        match &e.sha256 {
            Some(h) if h.len() != 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) => {
                return Err(format!("「{}」的 sha256 格式不对", e.id));
            }
            None if source.official => return Err(format!("官方源的「{}」缺少 sha256", e.id)),
            _ => {}
        }
        entries.push(e);
    }
    Ok((name, entries, trust))
}

#[derive(Default, Deserialize)]
struct SourcesFile {
    sources: Vec<Source>,
}

fn read_sources(dir: &Path) -> Vec<Source> {
    fs::read(dir.join(SOURCES_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SourcesFile>(&bytes).ok())
        .unwrap_or_default()
        .sources
}

fn write_sources(dir: &Path, sources: &[Source]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("无法创建配置目录: {e}"))?;
    fs::write(
        dir.join(SOURCES_FILE),
        serde_json::to_string_pretty(&json!({ "sources": sources })).unwrap_or_default(),
    )
    .map_err(|e| format!("保存插件源失败: {e}"))
}

pub struct Market {
    roots: Arc<Roots>,
    trusted: Vec<TrustedKey>,
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl Market {
    pub fn new(roots: Arc<Roots>, trusted: Vec<TrustedKey>) -> Self {
        Self { roots, trusted }
    }

    pub fn official() -> Source {
        Source {
            id: OFFICIAL_ID.into(),
            name: official_config().name.clone(),
            url: official_config().url.clone(),
            official: true,
        }
    }

    /// 官方源 + 用户添加的源（各个根的配置目录里的 sources.json 合并）。
    pub fn sources(&self) -> Vec<Source> {
        let mut out = vec![Self::official()];
        for dir in self.roots.config_search() {
            for s in read_sources(&dir) {
                if s.id != OFFICIAL_ID && !out.iter().any(|o| o.id == s.id) {
                    out.push(s);
                }
            }
        }
        out
    }

    pub fn find_source(&self, id: &str) -> Option<Source> {
        self.sources().into_iter().find(|s| s.id == id)
    }

    /// 添加私有源（索引地址）。同一个地址不会重复添加。
    pub fn add_source(&self, url: &str, name: Option<&str>) -> Result<Source, String> {
        let url = url.trim();
        let host = http::check_url(url, None)?;
        let id = format!("u-{}", short_hash(url));
        if let Some(existing) = self.find_source(&id) {
            return Ok(existing);
        }
        let name = name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(&host)
            .to_string();
        let source = Source {
            id,
            name,
            url: url.to_string(),
            official: false,
        };
        let dir = self.roots.config_write_dir()?;
        let mut list = read_sources(&dir);
        list.push(source.clone());
        write_sources(&dir, &list)?;
        Ok(source)
    }

    pub fn remove_source(&self, id: &str) -> Result<(), String> {
        if id == OFFICIAL_ID {
            return Err("官方插件源不能删除".into());
        }
        let mut removed = false;
        for dir in self.roots.config_search() {
            let list = read_sources(&dir);
            if list.iter().any(|s| s.id == id) {
                let rest: Vec<Source> = list.into_iter().filter(|s| s.id != id).collect();
                write_sources(&dir, &rest)?;
                removed = true;
            }
        }
        if removed {
            Ok(())
        } else {
            Err("没有找到这个插件源".into())
        }
    }

    fn cache_paths(&self, source: &Source) -> Option<(PathBuf, PathBuf)> {
        let dir = self.roots.config_write_dir().ok()?.join("cache");
        Some((
            dir.join(format!("{}.index.json", source.id)),
            dir.join(format!("{}.index.sig", source.id)),
        ))
    }

    /// 取索引。网络失败时退回上次的缓存（会重新校验），并在结果里说明。
    pub fn fetch_index(&self, source: &Source) -> Result<IndexResult, String> {
        let allowed = source.official.then(official_hosts);
        let sig_url = format!("{}.sig", source.url);
        let fetched = http::fetch_bytes(&source.url, MAX_INDEX_BYTES, allowed).map(|bytes| {
            let sig = http::fetch_bytes(&sig_url, 16 * 1024, allowed)
                .ok()
                .map(|b| String::from_utf8_lossy(&b).into_owned());
            (bytes, sig)
        });
        match fetched {
            Ok((bytes, sig)) => {
                let (name, entries, trust) =
                    process_index(source, &bytes, sig.as_deref(), &self.trusted)?;
                if let Some((index_path, sig_path)) = self.cache_paths(source) {
                    if let Some(parent) = index_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    let _ = fs::write(&index_path, &bytes);
                    match &sig {
                        Some(s) => {
                            let _ = fs::write(&sig_path, s);
                        }
                        None => {
                            let _ = fs::remove_file(&sig_path);
                        }
                    }
                }
                Ok(IndexResult {
                    source: source.clone(),
                    name,
                    entries,
                    trust,
                    from_cache: false,
                    error: None,
                })
            }
            Err(net_err) => {
                let (index_path, sig_path) =
                    self.cache_paths(source).ok_or_else(|| net_err.clone())?;
                let bytes = fs::read(&index_path).map_err(|_| net_err.clone())?;
                let sig = fs::read_to_string(&sig_path).ok();
                let (name, entries, trust) =
                    process_index(source, &bytes, sig.as_deref(), &self.trusted)?;
                Ok(IndexResult {
                    source: source.clone(),
                    name,
                    entries,
                    trust,
                    from_cache: true,
                    error: Some(net_err),
                })
            }
        }
    }

    /// 下载插件包到 `dest_dir`（临时目录）。`official_only` 为真时限制在 GitHub 域名，并且必须提供 `expected_sha256`。
    /// 返回下载到的文件路径；之后用和"导入"相同的流程检查并安装。
    pub fn download_package(
        &self,
        url: &str,
        expected_sha256: Option<&str>,
        official_only: bool,
        dest_dir: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> Result<PathBuf, String> {
        if official_only && expected_sha256.is_none() {
            return Err("官方源的下载必须提供 sha256".into());
        }
        let hosts = official_hosts();
        let allowed = official_only.then_some(hosts);
        http::check_url(url, allowed)?;

        let file_name = url::Url::parse(url)
            .ok()
            .and_then(|u| u.path_segments().and_then(|s| s.last().map(String::from)))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "package.nut".into());
        let clean: String = file_name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dest = dest_dir.join(format!("{}-dl{n}-{clean}", std::process::id()));

        let actual = http::download(
            url,
            &dest,
            MAX_PACKAGE_BYTES as u64,
            allowed,
            progress,
            cancel,
        )?;
        if let Some(expected) = expected_sha256 {
            if !actual.eq_ignore_ascii_case(expected) {
                let _ = fs::remove_file(&dest);
                return Err(
                    "下载内容的 SHA-256 与索引不符，已丢弃（文件可能被篡改或索引过期）".into(),
                );
            }
        }
        Ok(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::DataRoot;

    #[test]
    fn builtin_source_config_is_valid() {
        let config = official_config();
        assert!(!config.name.trim().is_empty());
        assert!(!config.allowed_hosts.is_empty());
        for host in &config.allowed_hosts {
            assert_eq!(
                http::check_url(&format!("https://{host}"), None).unwrap(),
                *host
            );
        }
        http::check_url(&config.url, Some(official_hosts())).unwrap();
        let source = Market::official();
        assert!(source.official);
        assert_eq!(source.url, config.url);
        assert_eq!(source.name, config.name);
    }

    fn keypair() -> ([u8; 32], Vec<TrustedKey>) {
        let seed = [7u8; 32];
        let public = signature::public_key_of_seed(&seed);
        let json = format!(
            r#"{{"keys":[{{"name":"测试","public":"{}"}}]}}"#,
            signature::format_public_key(&public)
        );
        (seed, signature::load_trusted_keys(&json).unwrap())
    }

    fn index(url: &str, sha: Option<&str>) -> Vec<u8> {
        let mut p = json!({ "id": "com.example.a", "name": "A", "version": "1.0.0", "url": url });
        if let Some(s) = sha {
            p["sha256"] = json!(s);
        }
        serde_json::to_vec(&json!({ "name": "测试源", "plugins": [p] })).unwrap()
    }

    fn sig_of(seed: &[u8; 32], bytes: &[u8]) -> String {
        serde_json::to_string(&signature::sign_index(seed, bytes)).unwrap()
    }

    #[test]
    fn official_index_requires_trusted_signature_github_urls_and_sha() {
        let (seed, trusted) = keypair();
        let official = Market::official();
        let sha = "a".repeat(64);
        let good = index(
            "https://github.com/x/y/releases/download/v1/a.nut",
            Some(&sha),
        );
        let (name, entries, trust) =
            process_index(&official, &good, Some(&sig_of(&seed, &good)), &trusted).unwrap();
        assert_eq!(
            (name.as_str(), entries.len(), trust.level),
            ("测试源", 1, TrustLevel::Trusted)
        );

        // 兼容 entries 字段名
        let with_entries = serde_json::to_vec(&json!({
            "name": "测试源",
            "entries": [{ "id": "com.example.a", "name": "A", "version": "1.0.0", "url": "https://github.com/x/a.nut", "sha256": sha }]
        })).unwrap();
        let (_, entries2, _) = process_index(
            &official,
            &with_entries,
            Some(&sig_of(&seed, &with_entries)),
            &trusted,
        )
        .unwrap();
        assert_eq!(entries2.len(), 1);

        assert!(
            process_index(&official, &good, None, &trusted).is_err(),
            "官方源必须有签名"
        );
        let mut tampered = good.clone();
        tampered.extend_from_slice(b" ");
        assert!(
            process_index(&official, &tampered, Some(&sig_of(&seed, &good)), &trusted).is_err(),
            "内容被改动"
        );
        let other = index("https://evil.example.com/a.nut", Some(&sha));
        assert!(
            process_index(&official, &other, Some(&sig_of(&seed, &other)), &trusted).is_err(),
            "官方源只允许 GitHub 域名"
        );
        let no_sha = index("https://github.com/x/a.nut", None);
        assert!(
            process_index(&official, &no_sha, Some(&sig_of(&seed, &no_sha)), &trusted).is_err(),
            "官方源必须带 sha256"
        );
    }

    #[test]
    fn private_source_may_be_unsigned_but_never_wrongly_signed() {
        let (seed, trusted) = keypair();
        let src = Source {
            id: "u-1".into(),
            name: "内部".into(),
            url: "https://intra.example.com/index.json".into(),
            official: false,
        };
        let idx = index("https://intra.example.com/a.nut", None);
        let (_, entries, trust) = process_index(&src, &idx, None, &trusted).unwrap();
        assert_eq!((entries.len(), trust.level), (1, TrustLevel::Unverified));
        let (_, _, signed) =
            process_index(&src, &idx, Some(&sig_of(&seed, &idx)), &trusted).unwrap();
        assert_eq!(signed.level, TrustLevel::Trusted);
        let other = index("https://intra.example.com/b.nut", None);
        assert!(
            process_index(&src, &other, Some(&sig_of(&seed, &idx)), &trusted).is_err(),
            "签名对不上要拒绝"
        );
        let http_url = index("http://intra.example.com/a.nut", None);
        assert!(
            process_index(&src, &http_url, None, &trusted).is_err(),
            "私有源也只允许 https"
        );
        assert!(process_index(&src, b"not json", None, &trusted).is_err());
    }

    #[test]
    fn user_sources_are_added_deduplicated_and_removed() {
        let root = std::env::temp_dir().join(format!("nuthusk-market-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let roots = Arc::new(Roots::new(
            vec![DataRoot {
                key: "a".into(),
                label: "A".into(),
                bases: vec![root.clone()],
            }],
            root.join("order.json"),
        ));
        let m = Market::new(roots, vec![]);
        assert_eq!(m.sources().len(), 1);
        assert!(m.add_source("http://x.com/index.json", None).is_err());
        let s = m
            .add_source("https://intra.example.com/index.json", Some("内部库"))
            .unwrap();
        assert_eq!(
            m.add_source("https://intra.example.com/index.json", None)
                .unwrap()
                .id,
            s.id,
            "重复添加返回同一个"
        );
        assert_eq!(m.sources().len(), 2);
        assert_eq!(m.find_source(&s.id).unwrap().name, "内部库");
        assert!(m.remove_source(OFFICIAL_ID).is_err());
        m.remove_source(&s.id).unwrap();
        assert_eq!(m.sources().len(), 1);
        assert!(m.remove_source(&s.id).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn official_download_requires_sha_and_github_host() {
        let m = Market::new(Arc::new(Roots::new(vec![], PathBuf::from("none"))), vec![]);
        let cancel = AtomicBool::new(false);
        let dir = std::env::temp_dir();
        let r = m.download_package(
            "https://github.com/x/a.nut",
            None,
            true,
            &dir,
            &mut |_, _| {},
            &cancel,
        );
        assert!(r.unwrap_err().contains("sha256"));
        let r = m.download_package(
            "https://evil.example.com/a.nut",
            Some(&"a".repeat(64)),
            true,
            &dir,
            &mut |_, _| {},
            &cancel,
        );
        assert!(r.unwrap_err().contains("域名"));
        let r = m.download_package(
            "http://example.com/a.nut",
            None,
            false,
            &dir,
            &mut |_, _| {},
            &cancel,
        );
        assert!(r.unwrap_err().contains("https"));
    }
}
