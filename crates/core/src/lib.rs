//! NutHusk 核心：插件 manifest 校验、插件发现与目录优先级、进程外后端（stdio JSON-RPC）、
//! 插件包安装、签名与可信来源、应用设置、数据根目录、运行时（解释器）管理、插件下载。
//! 不依赖 Tauri，可以单独测试，将来换壳也能复用。

mod http;
mod layout;
mod manager;
mod manifest;
mod market;
pub mod package;
mod runtime;
mod settings;
pub mod signature;

pub use layout::{DataRoot, RootInfo, RootPathInfo, Roots, CONFIG_DIR, PLUGINS_DIR, RUNTIMES_DIR};
pub use manager::{DirInfo, DirPathInfo, InstallResult, PluginDir, PluginInfo, PluginManager};
pub use manifest::{
    has_icon_extension, BackendSpec, CommandSpec, Manifest, Permission, HOST_API_VERSION, ICON_EXTENSIONS,
};
pub use market::{IndexEntry, IndexResult, Market, Source, OFFICIAL_ID, OFFICIAL_INDEX_URL};
pub use package::{command_on_path, PackageInfo};
pub use runtime::{
    platform_key, InstalledRuntime, Recipe, RuntimeManager, RuntimeMissing, RuntimeStatus, MISSING_PREFIX,
};
pub use settings::Settings;
pub use signature::{load_trusted_keys, Trust, TrustLevel, TrustedKey};
