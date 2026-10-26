//! 数据根目录。程序附带的文件按"根"组织，每个根下有固定的三个子目录：
//!
//! ```text
//! <根>/nutPlugins/   插件
//! <根>/nutRuntimes/  运行时（解释器等，由程序下载或手动放入）
//! <根>/nutConfig/    配置（运行时配方 runtimes.d/、插件源 sources.json 等）
//! ```
//!
//! 有两个根（AppData、程序目录），优先级可调：同名插件/运行时取靠前的根里的；
//! 新下载的内容写到靠前且可写的根里。优先级保存在 AppData 下一个很小的 `order` 文件里
//! （不放进配置目录，避免"配置位置取决于优先级、优先级又存在配置里"的循环）。

use crate::manager::PluginDir;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

pub const PLUGINS_DIR: &str = "nutPlugins";
pub const RUNTIMES_DIR: &str = "nutRuntimes";
pub const CONFIG_DIR: &str = "nutConfig";

/// 一个数据根。`bases` 可以有多个物理路径（例如程序所在目录与安装包资源目录），第一个是写入位置。
#[derive(Debug, Clone)]
pub struct DataRoot {
    pub key: String,
    pub label: String,
    pub bases: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootPathInfo {
    pub path: String,
    pub exists: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootInfo {
    pub key: String,
    pub label: String,
    pub paths: Vec<RootPathInfo>,
    /// 根下的固定子目录名。
    pub subdirs: Vec<String>,
}

/// 目录是否可写（目录不存在时，看它最近的已存在的上级目录）。不会创建任何目录。
pub(crate) fn is_writable(dir: &Path) -> bool {
    let mut probe_dir = Some(dir);
    while let Some(d) = probe_dir {
        if d.exists() && !d.is_dir() {
            return false; // 路径上有一个普通文件挡着，建不出目录
        }
        if d.is_dir() {
            let probe = d.join(format!(".nuthusk-write-test-{}", std::process::id()));
            return match std::fs::File::create(&probe) {
                Ok(_) => {
                    let _ = std::fs::remove_file(&probe);
                    true
                }
                Err(_) => false,
            };
        }
        probe_dir = d.parent();
    }
    false
}

pub struct Roots {
    roots: RwLock<Vec<DataRoot>>,
    order_file: PathBuf,
}

impl Roots {
    /// `roots` 是默认顺序；`order_file` 里保存过的顺序会覆盖它（无法识别的键被忽略）。
    pub fn new(roots: Vec<DataRoot>, order_file: PathBuf) -> Self {
        let this = Self { roots: RwLock::new(roots), order_file };
        let saved: Vec<String> = std::fs::read_to_string(&this.order_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.get("order").and_then(Value::as_array).cloned())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let known: Vec<String> = saved.into_iter().filter(|k| this.keys().contains(k)).collect();
        if !known.is_empty() {
            let _ = this.reorder(&known);
        }
        this
    }

    pub fn keys(&self) -> Vec<String> {
        self.roots.read().unwrap().iter().map(|r| r.key.clone()).collect()
    }

    fn reorder(&self, order: &[String]) -> Result<(), String> {
        let mut roots = self.roots.write().unwrap();
        if let Some(bad) = order.iter().find(|k| !roots.iter().any(|r| &r.key == *k)) {
            return Err(format!("未知的目录: {bad}"));
        }
        let mut sorted: Vec<DataRoot> = Vec::new();
        for k in order {
            if !sorted.iter().any(|r| &r.key == k) {
                if let Some(r) = roots.iter().find(|r| &r.key == k) {
                    sorted.push(r.clone());
                }
            }
        }
        for r in roots.iter() {
            if !sorted.iter().any(|s| s.key == r.key) {
                sorted.push(r.clone());
            }
        }
        *roots = sorted;
        Ok(())
    }

    /// 调整优先级并保存。`order` 里没提到的根按原顺序排在最后；出现未知的键则报错。
    pub fn set_order(&self, order: &[String]) -> Result<(), String> {
        self.reorder(order)?;
        if let Some(parent) = self.order_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&self.order_file, json!({ "order": self.keys() }).to_string())
            .map_err(|e| format!("保存目录优先级失败: {e}"))
    }

    fn snapshot(&self) -> Vec<DataRoot> {
        self.roots.read().unwrap().clone()
    }

    /// 按优先级排列的插件目录组，交给 `PluginManager`。
    pub fn plugin_groups(&self) -> Vec<PluginDir> {
        self.snapshot()
            .into_iter()
            .map(|r| PluginDir {
                key: r.key,
                label: r.label,
                paths: r.bases.iter().map(|b| b.join(PLUGINS_DIR)).collect(),
            })
            .collect()
    }

    /// 按优先级排列的运行时目录（查找用）。
    pub fn runtime_search(&self) -> Vec<PathBuf> {
        self.snapshot().iter().flat_map(|r| r.bases.iter().map(|b| b.join(RUNTIMES_DIR))).collect()
    }

    /// 按优先级排列的配置目录（读取用，各根里的配置合并）。
    pub fn config_search(&self) -> Vec<PathBuf> {
        self.snapshot().iter().flat_map(|r| r.bases.iter().map(|b| b.join(CONFIG_DIR))).collect()
    }

    fn first_writable(&self, sub: &str) -> Result<PathBuf, String> {
        for r in self.snapshot() {
            if let Some(base) = r.bases.first() {
                if is_writable(base) {
                    return Ok(base.join(sub));
                }
            }
        }
        Err("没有可写的目录，请检查权限，或在设置里调整目录优先级".into())
    }

    /// 新运行时的安装位置：优先级最高且可写的根。
    pub fn runtime_install_dir(&self) -> Result<PathBuf, String> {
        self.first_writable(RUNTIMES_DIR)
    }

    /// 用户新增配置（配方、插件源）的写入位置。
    pub fn config_write_dir(&self) -> Result<PathBuf, String> {
        self.first_writable(CONFIG_DIR)
    }

    pub fn info(&self) -> Vec<RootInfo> {
        self.snapshot()
            .into_iter()
            .map(|r| RootInfo {
                key: r.key,
                label: r.label,
                paths: r
                    .bases
                    .iter()
                    .map(|p| RootPathInfo { path: p.display().to_string(), exists: p.is_dir(), writable: is_writable(p) })
                    .collect(),
                subdirs: vec![PLUGINS_DIR.into(), RUNTIMES_DIR.into(), CONFIG_DIR.into()],
            })
            .collect()
    }
}
