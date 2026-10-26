use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

/// 宿主 API 版本。发布后要长期兼容，只做加法。
pub const HOST_API_VERSION: &str = "1.0.0";

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
            v.push(CommandSpec { command: c.clone(), args: self.args.clone() });
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
            self.command
                .as_ref()
                .map(|c| CommandSpec { command: c.clone(), args: self.args.clone() })
        })
    }

    pub fn for_current_os(&self) -> Option<CommandSpec> {
        self.for_os(std::env::consts::OS)
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
}

fn default_category() -> String {
    "其他".into()
}

fn valid_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// 相对路径必须只含普通路径段，不允许 `..`、绝对路径等。
pub fn is_safe_relative(p: &str) -> bool {
    let path = Path::new(p);
    path.components().next().is_some() && path.components().all(|c| matches!(c, Component::Normal(_)))
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
        if !valid_id(&self.id) {
            return Err(format!("id 不合法: {}（应形如 com.example.my-tool）", self.id));
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
        Ok(())
    }

    /// 宿主 API 是否满足插件要求。
    pub fn check_compatible(&self) -> Result<(), String> {
        let req = VersionReq::parse(&self.api_version).map_err(|e| e.to_string())?;
        let host = Version::parse(HOST_API_VERSION).map_err(|e| e.to_string())?;
        if !req.matches(&host) {
            return Err(format!("需要宿主 API {}，当前为 {}", self.api_version, HOST_API_VERSION));
        }
        if let Some(b) = &self.backend {
            if b.for_current_os().is_none() {
                return Err(format!("该插件没有适用于当前系统（{}）的后端", std::env::consts::OS));
            }
        }
        Ok(())
    }
}
