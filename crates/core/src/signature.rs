//! 插件签名与可信来源。
//!
//! 流程：开发者审查第三方提交的插件 → 用自己的私钥给插件包签名 → 软件里内置开发者的公钥，
//! 验证通过的插件标注"可信来源"（绿色）；没有签名或签名者不在受信任列表里的标注"未经安全验证"（黄色）；
//! 签名存在但与内容对不上（被篡改）的标注"签名无效"（红色），并且禁止安装。
//!
//! 签名的是"内容摘要"而不是 zip 文件本身，所以重新压缩、增减顶层文件夹都不影响签名。
//! 摘要 = SHA-256( 每个文件按路径排序后的 "路径\0文件哈希\n" )，不含 signature.json 自身。
//! 被签名的消息还包含插件 id 和版本，防止把一份签名套用到别的插件上。

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::Path;

pub const SIGNATURE_FILE: &str = "signature.json";
pub const PUBLIC_KEY_PREFIX: &str = "nuthusk-ed25519-public:";
pub const PRIVATE_KEY_PREFIX: &str = "nuthusk-ed25519-private:";
const MESSAGE_TAG: &str = "NutHusk-Plugin-Signature-v1";
const MAX_SIGNATURE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureFile {
    pub version: u32,
    pub algorithm: String,
    pub key_id: String,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct TrustedKey {
    pub name: String,
    pub key_id: String,
    key: VerifyingKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustLevel {
    /// 由受信任的密钥签名，且内容未被改动。
    Trusted,
    /// 没有签名，或签名者不在受信任列表里。
    Unverified,
    /// 签名存在但无法通过校验（内容被改动、格式损坏等）。
    Invalid,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trust {
    pub level: TrustLevel,
    pub reason: String,
    pub signer: Option<String>,
    pub key_id: Option<String>,
}

impl Trust {
    pub fn unverified(reason: impl Into<String>) -> Self {
        Self { level: TrustLevel::Unverified, reason: reason.into(), signer: None, key_id: None }
    }
    pub fn invalid(reason: impl Into<String>) -> Self {
        Self { level: TrustLevel::Invalid, reason: reason.into(), signer: None, key_id: None }
    }
}

pub fn key_id_of(public: &[u8; 32]) -> String {
    Sha256::digest(public).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

pub fn format_public_key(public: &[u8; 32]) -> String {
    format!("{PUBLIC_KEY_PREFIX}{}", B64.encode(public))
}

pub fn format_private_key(seed: &[u8; 32]) -> String {
    format!("{PRIVATE_KEY_PREFIX}{}", B64.encode(seed))
}

fn parse_key_bytes(text: &str, prefix: &str) -> Result<[u8; 32], String> {
    let body = text
        .trim()
        .strip_prefix(prefix)
        .ok_or_else(|| format!("密钥格式不对，应以 {prefix} 开头"))?;
    let bytes = B64.decode(body.trim()).map_err(|e| format!("密钥不是有效的 base64: {e}"))?;
    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| "密钥长度应为 32 字节".to_string())
}

pub fn parse_public_key(text: &str) -> Result<[u8; 32], String> {
    parse_key_bytes(text, PUBLIC_KEY_PREFIX)
}

pub fn parse_private_key(text: &str) -> Result<[u8; 32], String> {
    parse_key_bytes(text, PRIVATE_KEY_PREFIX)
}

pub fn public_key_of_seed(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

/// 解析受信任公钥列表：`{"keys":[{"name":"开发者","public":"nuthusk-ed25519-public:..."}]}`
pub fn load_trusted_keys(json: &str) -> Result<Vec<TrustedKey>, String> {
    #[derive(Deserialize)]
    struct Item {
        name: String,
        public: String,
    }
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        keys: Vec<Item>,
    }
    let file: File = serde_json::from_str(json).map_err(|e| format!("trusted_keys.json 无法解析: {e}"))?;
    file.keys
        .into_iter()
        .map(|i| {
            let bytes = parse_public_key(&i.public).map_err(|e| format!("「{}」的公钥无效: {e}", i.name))?;
            let key = VerifyingKey::from_bytes(&bytes).map_err(|e| format!("「{}」的公钥无效: {e}", i.name))?;
            Ok(TrustedKey { name: i.name, key_id: key_id_of(&bytes), key })
        })
        .collect()
}

pub fn hash_reader(mut r: impl Read) -> io::Result<[u8; 32]> {
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

/// 把"(相对路径, 文件哈希)"列表合成内容摘要（十六进制）。
pub fn digest_from_hashes(mut entries: Vec<(String, [u8; 32])>) -> String {
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = Sha256::new();
    for (path, hash) in entries {
        h.update(path.as_bytes());
        h.update([0u8]);
        h.update(hash.iter().map(|b| format!("{b:02x}")).collect::<String>().as_bytes());
        h.update(b"\n");
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 已解压目录的内容摘要。含符号链接时返回错误（符号链接可以把校验引到别处）。
pub fn digest_dir(dir: &Path) -> Result<String, String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, [u8; 32])>) -> Result<(), String> {
        let mut items: Vec<_> = fs::read_dir(dir)
            .map_err(|e| format!("读取目录失败: {e}"))?
            .flatten()
            .collect();
        items.sort_by_key(|e| e.file_name());
        for e in items {
            let path = e.path();
            let ft = e.file_type().map_err(|e| e.to_string())?;
            if ft.is_symlink() {
                return Err(format!("含符号链接: {}", path.display()));
            }
            if ft.is_dir() {
                walk(base, &path, out)?;
            } else if ft.is_file() {
                let rel = path
                    .strip_prefix(base)
                    .map_err(|e| e.to_string())?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                if rel == SIGNATURE_FILE {
                    continue;
                }
                let f = fs::File::open(&path).map_err(|e| format!("读取 {rel} 失败: {e}"))?;
                let hash = hash_reader(f).map_err(|e| format!("读取 {rel} 失败: {e}"))?;
                out.push((rel, hash));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    Ok(digest_from_hashes(out))
}

pub fn signing_message(id: &str, version: &str, digest: &str) -> Vec<u8> {
    format!("{MESSAGE_TAG}\nid={id}\nversion={version}\ndigest={digest}\n").into_bytes()
}

/// 用私钥种子给（插件 id、版本、内容摘要）签名，返回应写入 signature.json 的内容。
pub fn sign(seed: &[u8; 32], id: &str, version: &str, digest: &str) -> SignatureFile {
    let key = SigningKey::from_bytes(seed);
    let sig = key.sign(&signing_message(id, version, digest));
    SignatureFile {
        version: 1,
        algorithm: "ed25519".into(),
        key_id: key_id_of(&key.verifying_key().to_bytes()),
        signature: B64.encode(sig.to_bytes()),
    }
}

/* ---------- 插件源索引（index.json）的签名 ---------- */

const INDEX_TAG: &str = "NutHusk-Index-Signature-v1";

fn index_message(index_bytes: &[u8]) -> Vec<u8> {
    let hex: String = Sha256::digest(index_bytes).iter().map(|b| format!("{b:02x}")).collect();
    format!("{INDEX_TAG}\nsha256={hex}\n").into_bytes()
}

/// 给索引文件的原始字节签名（签名文件通常叫 `index.json.sig`）。
pub fn sign_index(seed: &[u8; 32], index_bytes: &[u8]) -> SignatureFile {
    let key = SigningKey::from_bytes(seed);
    let sig = key.sign(&index_message(index_bytes));
    SignatureFile {
        version: 1,
        algorithm: "ed25519".into(),
        key_id: key_id_of(&key.verifying_key().to_bytes()),
        signature: B64.encode(sig.to_bytes()),
    }
}

/// 校验索引。没有签名 → 未经安全验证；由受信任的密钥签名且对得上 → 可信；其余 → 签名无效。
pub fn verify_index(signature_text: Option<&str>, index_bytes: &[u8], trusted: &[TrustedKey]) -> Trust {
    let Some(text) = signature_text else {
        return Trust::unverified("没有签名");
    };
    if text.len() > MAX_SIGNATURE_BYTES {
        return Trust::invalid("签名文件过大");
    }
    let Ok(sig_file) = serde_json::from_str::<SignatureFile>(text) else {
        return Trust::invalid("签名文件无法解析");
    };
    if sig_file.version != 1 || sig_file.algorithm != "ed25519" {
        return Trust::invalid("不支持的签名格式");
    }
    let Some(key) = trusted.iter().find(|k| k.key_id == sig_file.key_id) else {
        return Trust {
            level: TrustLevel::Unverified,
            reason: format!("由不在受信任列表里的密钥签名（{}）", sig_file.key_id),
            signer: None,
            key_id: Some(sig_file.key_id),
        };
    };
    let ok = B64
        .decode(sig_file.signature.trim())
        .ok()
        .and_then(|b| Signature::from_slice(&b).ok())
        .map(|s| key.key.verify(&index_message(index_bytes), &s).is_ok())
        .unwrap_or(false);
    if ok {
        Trust {
            level: TrustLevel::Trusted,
            reason: format!("由「{}」签名，内容未被改动", key.name),
            signer: Some(key.name.clone()),
            key_id: Some(key.key_id.clone()),
        }
    } else {
        Trust {
            level: TrustLevel::Invalid,
            reason: "签名与内容对不上，索引可能被篡改".into(),
            signer: None,
            key_id: Some(sig_file.key_id),
        }
    }
}

/// 校验。`digest` 是惰性计算的：没有签名的插件不需要读取全部文件，扫描时开销很小。
pub fn verify(
    signature_text: Option<&str>,
    id: &str,
    version: &str,
    digest: impl FnOnce() -> Result<String, String>,
    trusted: &[TrustedKey],
) -> Trust {
    let Some(text) = signature_text else {
        return Trust::unverified("没有签名");
    };
    if text.len() > MAX_SIGNATURE_BYTES {
        return Trust::invalid("signature.json 过大");
    }
    let Ok(sig_file) = serde_json::from_str::<SignatureFile>(text) else {
        return Trust::invalid("signature.json 无法解析");
    };
    if sig_file.version != 1 || sig_file.algorithm != "ed25519" {
        return Trust::invalid("不支持的签名格式");
    }
    let Some(key) = trusted.iter().find(|k| k.key_id == sig_file.key_id) else {
        return Trust {
            level: TrustLevel::Unverified,
            reason: format!("由不在受信任列表里的密钥签名（{}）", sig_file.key_id),
            signer: None,
            key_id: Some(sig_file.key_id),
        };
    };
    let digest = match digest() {
        Ok(d) => d,
        Err(e) => return Trust::invalid(format!("无法计算内容摘要: {e}")),
    };
    let ok = B64
        .decode(sig_file.signature.trim())
        .ok()
        .and_then(|b| Signature::from_slice(&b).ok())
        .map(|s| key.key.verify(&signing_message(id, version, &digest), &s).is_ok())
        .unwrap_or(false);
    if ok {
        Trust {
            level: TrustLevel::Trusted,
            reason: format!("由「{}」签名，内容未被改动", key.name),
            signer: Some(key.name.clone()),
            key_id: Some(key.key_id.clone()),
        }
    } else {
        Trust {
            level: TrustLevel::Invalid,
            reason: "签名与内容对不上，文件可能被篡改，或签名无效".into(),
            signer: None,
            key_id: Some(sig_file.key_id),
        }
    }
}
