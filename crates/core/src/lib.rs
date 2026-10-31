//! NutHusk 核心：插件 manifest 校验、插件发现与目录优先级、进程外后端（stdio JSON-RPC）、
//! 插件包安装、签名与可信来源、应用设置、数据根目录、运行时（解释器）管理、插件下载、配置加载清单、开发者工具链。
//! 不依赖 Tauri，可以单独测试，将来换壳也能复用。

pub mod devkit;
mod http;
mod layout;
mod manager;
mod manifest;
mod market;
pub mod package;
mod profile;
mod runtime;
mod settings;
pub mod signature;

pub use devkit::{
    check_plugin_dir, pack_plugin_dir, run_headless_action, scaffold_plugin, CheckReport,
};
pub use layout::{DataRoot, RootInfo, RootPathInfo, Roots, CONFIG_DIR, PLUGINS_DIR, RUNTIMES_DIR};
pub use manager::{InvokeError, DirInfo, DirPathInfo, InstallResult, PluginDir, PluginInfo, PluginManager};
pub use manifest::{
    has_icon_extension, is_valid_id, ActionSpec, BackendSpec, CommandSpec, Manifest, MatchSpec,
    Permission, HOST_API_VERSION, ICON_EXTENSIONS,
};
pub use market::{IndexEntry, IndexResult, Market, Source, OFFICIAL_ID};
pub use package::{command_on_path, PackageInfo};
pub use profile::{
    NutProfile, ProfilePlugin, ProfileSource, ProfileWorkspace, PROFILE_FORMAT, PROFILE_VERSION,
};
pub use runtime::{
    platform_key, InstalledRuntime, Recipe, RuntimeManager, RuntimeMissing, RuntimeStatus,
    MISSING_PREFIX,
};
pub use settings::Settings;
pub use signature::{load_trusted_keys, Trust, TrustLevel, TrustedKey};
