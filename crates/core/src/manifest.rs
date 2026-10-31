use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Component, Path};

/// 宿主 API 版本。发布后要长期兼容，只做加法。
pub const HOST_API_VERSION: &str = "1.1.0";

/// 插件图标允许的图片格式。
pub const ICON_EXTENSIONS: [&str; 7] = ["png", "svg", "jpg", "jpeg", "webp", "gif", "ico"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Permission {
    #[serde(rename = "clipboard")]
    Clipboard,
    #[serde(rename = "fs.read")]
    FsRead,
    #[serde(rename = "fs.write")]
    FsWrite,
    #[serde(rename = "network")]
    Network,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct CommandSpec {
    /// 可执行文件。以 "./" 开头表示插件目录内的文件，否则从 PATH 查找（如 node、python3）。
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// 后端启动方式。可以只写通用的 command/args，也可以按系统（windows / macos / linux）分别覆盖，
/// 方便同一个插件携带各平台的可执行文件，或在不同系统上用不同的解释器名。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackendSpec {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub windows: Option<CommandSpec>,
    #[serde(default)]
    pub macos: Option<CommandSpec>,
    #[serde(default)]
    pub linux: Option<CommandSpec>,
}

impl BackendSpec {
    /// 所有声明过的启动方式（用于整体校验）。
    pub fn variants(&self) -> Vec<CommandSpec> {
        let mut v: Vec<CommandSpec> = [&self.windows, &self.macos, &self.linux]
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        if let Some(c) = &self.command {
            v.push(CommandSpec {
                command: c.clone(),
                args: self.args.clone(),
            });
        }
        v
    }

    /// 指定系统（std::env::consts::OS 的取值）对应的启动方式；没有专门覆盖时回退到通用的 command。
    pub fn for_os(&self, os: &str) -> Option<CommandSpec> {
        let specific = match os {
            "windows" => self.windows.clone(),
            "macos" => self.macos.clone(),
            "linux" => self.linux.clone(),
            _ => None,
        };
        specific.or_else(|| {
            self.command.as_ref().map(|c| CommandSpec {
                command: c.clone(),
                args: self.args.clone(),
            })
        })
    }

    pub fn for_current_os(&self) -> Option<CommandSpec> {
        self.for_os(std::env::consts::OS)
    }
}

/// 智能内容匹配规则：声明插件可自动识别并处理的文本特征或文件扩展名。
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSpec {
    /// 正则表达式列表（用于匹配粘贴或输入的文本）。
    #[serde(default)]
    pub regex: Vec<String>,
    /// 文本前缀列表（不区分大小写，如 ["eyJ", "https://"]）。
    #[serde(default)]
    pub prefixes: Vec<String>,
    /// 可处理的文件扩展名列表（不含点，小写，如 ["json", "png"]）。
    #[serde(default)]
    pub extensions: Vec<String>,
    /// 匹配文本的最小字符长度。
    #[serde(default)]
    pub min_length: Option<usize>,
    /// 匹配文本的最大字符长度。
    #[serde(default)]
    pub max_length: Option<usize>,
    /// 命中规则时的推荐提示语（如 "识别为 JSON 文本"）。
    #[serde(default)]
    pub label: Option<String>,
}

/// 插件声明的无头/流水线动作（可选，宿主 API 1.1.0 新增）。
/// 可被宿主流水线（Workflow）或 CLI `run` 命令直接调用，输入与输出均为文本或 JSON。
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionSpec {
    /// 动作唯一标识（插件内唯一，如 "format", "sha256"）。
    pub id: String,
    /// 动作显示名称（如 "JSON 格式化", "计算 SHA-256"）。
    pub name: String,
    /// 调用的方法名（后端 JSON-RPC method，或前端 onAction 处理的方法名；省略时默认等于 id）。
    #[serde(default)]
    pub method: Option<String>,
    /// 动作说明（可选）。
    #[serde(default)]
    pub description: String,
    /// 调用时附加的默认参数对象（可选，会与 `{ text }` 合并后传给插件）。
    #[serde(default)]
    pub default_params: Option<Value>,
}

impl ActionSpec {
    pub fn effective_method(&self) -> &str {
        self.method.as_deref().unwrap_or(&self.id)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    #[serde(default)]
    pub description: String,
    /// 插件目录内的图片文件（png / svg / jpg / webp / gif / ico），用于在软件里显示。
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub ui: Option<String>,
    pub backend: Option<BackendSpec>,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// 智能内容匹配规则（可选，宿主 API 1.1.0 新增）。
    #[serde(default, rename = "match")]
    pub r#match: Option<MatchSpec>,
    /// 可供流水线或命令行直接调用的动作列表（可选，宿主 API 1.1.0 新增）。
    #[serde(default)]
    pub actions: Vec<ActionSpec>,
}

fn default_category() -> String {
    "其他".into()
}

pub fn is_valid_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// 相对路径必须只含普通路径段，不允许 `..`、绝对路径等。
pub fn is_safe_relative(p: &str) -> bool {
    let path = Path::new(p);
    !p.contains('\0') && path.components().next().is_some()
        && path.components().all(|c| match c {
            Component::Normal(part) => {
                let text = part.to_string_lossy();
                let stem = text.split('.').next().unwrap_or("").to_ascii_uppercase();
                !text.contains(':') && !text.ends_with(['.', ' '])
                    && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                    && !(stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT"))
                        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
            }
            _ => false,
        })
}

pub fn is_plugin_local_command(cmd: &str) -> bool {
    cmd.starts_with("./") || cmd.starts_with(".\\")
}

pub fn has_icon_extension(p: &str) -> bool {
    Path::new(p)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| ICON_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        if !is_valid_id(&self.id) {
            return Err(format!(
                "id 不合法: {}（应形如 com.example.my-tool）",
                self.id
            ));
        }
        if self.name.trim().is_empty() {
            return Err("name 不能为空".into());
        }
        Version::parse(&self.version).map_err(|e| format!("version 不合法: {e}"))?;
        VersionReq::parse(&self.api_version).map_err(|e| format!("apiVersion 不合法: {e}"))?;
        if let Some(ui) = &self.ui {
            if !is_safe_relative(ui) {
                return Err(format!("ui 路径不安全: {ui}"));
            }
        }
        if let Some(icon) = &self.icon {
            if !is_safe_relative(icon) || !has_icon_extension(icon) {
                return Err(format!(
                    "icon 必须是插件目录内的图片文件（{}），当前: {icon}",
                    ICON_EXTENSIONS.join(" / ")
                ));
            }
        }
        if let Some(b) = &self.backend {
            let variants = b.variants();
            if variants.is_empty() {
                return Err("backend 至少要有 command，或某个系统的专用配置".into());
            }
            for v in variants {
                if v.command.trim().is_empty() {
                    return Err("backend.command 不能为空".into());
                }
                if is_plugin_local_command(&v.command) && !is_safe_relative(&v.command[2..]) {
                    return Err(format!("backend.command 路径不安全: {}", v.command));
                }
            }
        }
        if let Some(m) = &self.r#match {
            if let (Some(min), Some(max)) = (m.min_length, m.max_length) {
                if min > max {
                    return Err("match.minLength 不能大于 match.maxLength".into());
                }
            }
            for pat in &m.regex {
                if pat.trim().is_empty() || pat.len() > 512 {
                    return Err("match.regex 不能为空或超过 512 字符".into());
                }
            }
            for pfx in &m.prefixes {
                if pfx.is_empty() {
                    return Err("match.prefixes 不能为空字符串".into());
                }
            }
            for ext in &m.extensions {
                if ext.is_empty()
                    || !ext
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return Err(format!(
                        "match.extensions 不合法（只需写扩展名本身，不含点）: {ext}"
                    ));
                }
            }
        }
        let mut seen_actions = HashSet::new();
        for act in &self.actions {
            let aid = act.id.trim();
            if aid.is_empty() {
                return Err("actions.id 不能为空".into());
            }
            if act.name.trim().is_empty() {
                return Err(format!("动作 {aid} 的 name 不能为空"));
            }
            if let Some(m) = &act.method {
                if m.trim().is_empty() {
                    return Err(format!("动作 {aid} 的 method 不能为空"));
                }
            }
            if !seen_actions.insert(aid) {
                return Err(format!("actions.id 重复: {aid}"));
            }
        }
        Ok(())
    }

    /// 宿主 API 是否满足插件要求。
    pub fn check_compatible(&self) -> Result<(), String> {
        let req = VersionReq::parse(&self.api_version).map_err(|e| e.to_string())?;
        let host = Version::parse(HOST_API_VERSION).map_err(|e| e.to_string())?;
        if !req.matches(&host) {
            return Err(format!(
                "需要宿主 API {}，当前为 {}",
                self.api_version, HOST_API_VERSION
            ));
        }
        if let Some(b) = &self.backend {
            if b.for_current_os().is_none() {
                return Err(format!(
                    "该插件没有适用于当前系统（{}）的后端",
                    std::env::consts::OS
                ));
            }
        }
        Ok(())
    }
}
