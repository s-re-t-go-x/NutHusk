//! 配置加载清单文件（`.nutprofile`）的生成、校验与文件系统读写。
//!
//! `.nutprofile` 本质是一份 UTF-8 JSON 清单，记录当前工作台的软件设置、插件源、已安装插件列表、
//! 已安装便携运行时及用户的收藏/标签布局。导出后体积极小（几 KB），分享给他人或在新电脑导入时，
//! 宿主可根据清单自动添加源、下载缺失插件与运行时并恢复设置。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;

pub const PROFILE_FORMAT: &str = "nuthusk-profile";
pub const PROFILE_VERSION: u32 = 1;
const MAX_PROFILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSource {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePlugin {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 可选：直接指定插件包下载地址（若不指定，导入时从已配置的插件源索引中按 id 查找）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileWorkspace {
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub tabs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NutProfile {
    pub format: String,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub settings: Value,
    #[serde(default)]
    pub sources: Vec<ProfileSource>,
    #[serde(default)]
    pub runtimes: Vec<String>,
    #[serde(default)]
    pub plugins: Vec<ProfilePlugin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<ProfileWorkspace>,
}

impl NutProfile {
    pub fn validate(&self) -> Result<(), String> {
        if self.format != PROFILE_FORMAT {
            return Err(format!(
                "不是有效的 NutHusk 配置清单（format 应为 {PROFILE_FORMAT}）"
            ));
        }
        if self.version == 0 || self.version > PROFILE_VERSION {
            return Err(format!(
                "不支持的配置清单版本 v{}（当前支持 v1）",
                self.version
            ));
        }
        if self.sources.len() > 50 || self.plugins.len() > 500 || self.runtimes.len() > 50 {
            return Err("配置清单中的条目数量过多".into());
        }
        for s in &self.sources {
            crate::http::check_url(&s.url, None)
                .map_err(|e| format!("插件源「{}」地址无效: {e}", s.name))?;
        }
        for p in &self.plugins {
            if p.id.trim().is_empty() {
                return Err("配置清单中存在空的插件 id".into());
            }
            if let Some(url) = &p.url {
                crate::http::check_url(url, None)
                    .map_err(|e| format!("插件「{}」的下载地址无效: {e}", p.id))?;
            }
        }
        Ok(())
    }

    pub fn from_json_str(text: &str) -> Result<Self, String> {
        if text.len() as u64 > MAX_PROFILE_BYTES {
            return Err("配置清单文件过大".into());
        }
        let profile: Self =
            serde_json::from_str(text).map_err(|e| format!("无法解析配置清单 JSON: {e}"))?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn read_file(path: &Path) -> Result<Self, String> {
        let meta = fs::metadata(path).map_err(|e| format!("无法读取文件: {e}"))?;
        if meta.len() > MAX_PROFILE_BYTES {
            return Err("配置清单文件过大".into());
        }
        let text = fs::read_to_string(path).map_err(|e| format!("无法读取配置清单文件: {e}"))?;
        Self::from_json_str(&text)
    }

    pub fn write_file(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| format!("无法创建目录: {e}"))?;
            }
        }
        let json =
            serde_json::to_string_pretty(self).map_err(|e| format!("序列化配置清单失败: {e}"))?;
        fs::write(path, json).map_err(|e| format!("写入配置清单文件失败: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn profile_roundtrip_and_validation() {
        let p = NutProfile {
            format: PROFILE_FORMAT.into(),
            version: 1,
            name: Some("前端开发工作台".into()),
            settings: json!({ "theme": "dark", "stopOnClose": true }),
            sources: vec![ProfileSource {
                name: "团队源".into(),
                url: "https://example.com/index.json".into(),
            }],
            runtimes: vec!["node".into(), "python".into()],
            plugins: vec![ProfilePlugin {
                id: "com.example.json-formatter".into(),
                name: "JSON 格式化".into(),
                version: Some("1.0.0".into()),
                enabled: true,
                url: None,
                sha256: None,
            }],
            workspace: Some(ProfileWorkspace {
                favorites: vec!["com.example.json-formatter".into()],
                tabs: vec!["home".into(), "com.example.json-formatter".into()],
                active: Some("com.example.json-formatter".into()),
                split_id: None,
            }),
        };

        let dir = std::env::temp_dir().join(format!("nuthusk-profile-{}", std::process::id()));
        let file = dir.join("test.nutprofile");
        p.write_file(&file).unwrap();

        let loaded = NutProfile::read_file(&file).unwrap();
        assert_eq!(loaded, p);
        let _ = fs::remove_dir_all(&dir);

        // 非法 format 或非 https 源应被拒绝
        assert!(
            NutProfile::from_json_str(r#"{"format":"wrong","version":1}"#)
                .unwrap_err()
                .contains("format")
        );
        assert!(NutProfile::from_json_str(
            r#"{"format":"nuthusk-profile","version":1,"sources":[{"name":"bad","url":"http://insecure.com/i.json"}]}"#
        )
        .unwrap_err()
        .contains("https"));
    }
}
