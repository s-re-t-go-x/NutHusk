//! 下载。只允许 https；手动跟随重定向，每一跳都重新检查协议和域名；限制大小；边下边算 SHA-256。
//! 这些函数是阻塞的，在异步环境里请放进 `spawn_blocking`。

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const MAX_REDIRECTS: usize = 5;
const PROGRESS_STEP: u64 = 256 * 1024;

/// 域名是否在允许列表里（完全相同，或是其子域名）。
pub fn host_matches(host: &str, allowed: &[String]) -> bool {
    let host = host.to_ascii_lowercase();
    allowed.iter().any(|a| {
        let a = a.to_ascii_lowercase();
        host == a || host.ends_with(&format!(".{a}"))
    })
}

/// 检查地址：必须是 https，不带用户名密码，域名（若有限制）在允许列表里。返回域名。
pub fn check_url(url: &str, allowed: Option<&[String]>) -> Result<String, String> {
    let u = url::Url::parse(url).map_err(|e| format!("地址无效: {e}"))?;
    if u.scheme() != "https" {
        return Err("只允许 https 地址".into());
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("地址里不能包含用户名或密码".into());
    }
    let host = u.host_str().ok_or("地址里没有域名")?.to_string();
    if let Some(list) = allowed {
        if !host_matches(&host, list) {
            return Err(format!("不允许从这个域名下载: {host}"));
        }
    }
    Ok(host)
}

fn open(url: &str, allowed: Option<&[String]>) -> Result<ureq::Response, String> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(30))
        .user_agent("NutHusk")
        .build();
    let mut current = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        check_url(&current, allowed)?;
        let resp = match agent.get(&current).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) => return Err(format!("服务器返回 {code}")),
            Err(e) => return Err(format!("网络错误: {e}")),
        };
        if matches!(resp.status(), 301 | 302 | 303 | 307 | 308) {
            let loc = resp.header("Location").ok_or("重定向缺少 Location")?;
            let base = url::Url::parse(&current).map_err(|e| e.to_string())?;
            current = base.join(loc).map_err(|e| format!("重定向地址无效: {e}"))?.to_string();
            continue;
        }
        return Ok(resp);
    }
    Err("重定向次数过多".into())
}

/// 下载到内存（用于索引等小文件）。
pub fn fetch_bytes(url: &str, max_bytes: u64, allowed: Option<&[String]>) -> Result<Vec<u8>, String> {
    let resp = open(url, allowed)?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(max_bytes + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("读取失败: {e}"))?;
    if buf.len() as u64 > max_bytes {
        return Err("文件过大".into());
    }
    Ok(buf)
}

/// 下载到文件，返回内容的 SHA-256（十六进制）。出错或取消时会删除不完整的文件。
pub fn download(
    url: &str,
    dest: &Path,
    max_bytes: u64,
    allowed: Option<&[String]>,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<String, String> {
    let resp = open(url, allowed)?;
    let total = resp.header("Content-Length").and_then(|v| v.trim().parse::<u64>().ok());
    if total.map(|t| t > max_bytes).unwrap_or(false) {
        return Err("文件过大，已拒绝下载".into());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("无法创建目录: {e}"))?;
    }
    let result = (|| {
        let mut reader = resp.into_reader().take(max_bytes + 1);
        let mut out = File::create(dest).map_err(|e| format!("无法写入文件: {e}"))?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        let (mut done, mut reported) = (0u64, 0u64);
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err("已取消".to_string());
            }
            let n = reader.read(&mut buf).map_err(|e| format!("下载中断: {e}"))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| format!("写入失败: {e}"))?;
            done += n as u64;
            if done > max_bytes {
                return Err("文件过大，已中止下载".into());
            }
            if done - reported >= PROGRESS_STEP {
                progress(done, total);
                reported = done;
            }
        }
        progress(done, total.or(Some(done)));
        Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_without_credentials() {
        assert!(check_url("https://nodejs.org/dist/x.zip", None).is_ok());
        assert!(check_url("http://nodejs.org/x", None).is_err());
        assert!(check_url("ftp://nodejs.org/x", None).is_err());
        assert!(check_url("https://user:pw@nodejs.org/x", None).is_err());
        assert!(check_url("not a url", None).is_err());
    }

    #[test]
    fn host_allow_list_matches_exact_and_subdomains_only() {
        let allowed = vec!["github.com".to_string(), "githubusercontent.com".to_string()];
        assert!(check_url("https://github.com/a/b", Some(&allowed)).is_ok());
        assert!(check_url("https://raw.githubusercontent.com/a/b", Some(&allowed)).is_ok());
        assert!(check_url("https://evilgithub.com/a", Some(&allowed)).is_err());
        assert!(check_url("https://github.com.evil.io/a", Some(&allowed)).is_err());
    }
}
