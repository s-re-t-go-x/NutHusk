//! 应用设置：保存成一个 JSON 文件。已知的键会校验，未知的键原样保留，方便以后扩展设置项。

use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Settings {
    path: PathBuf,
    data: Mutex<Map<String, Value>>,
}

fn defaults() -> Map<String, Value> {
    let v = json!({
        "theme": "system",
        "showRecent": true,
        "showFavorites": true,
        "stopOnClose": true,
        "idleTimeoutMinutes": 5,
        "closeToTray": false,
        "alwaysOnTop": false,
        "dirOrder": ["appdata", "program"]
    });
    v.as_object().cloned().unwrap_or_default()
}

fn validate(key: &str, value: &Value) -> Result<(), String> {
    match key {
        "theme" => match value.as_str() {
            Some("system" | "light" | "dark") => Ok(()),
            _ => Err("theme 只能是 system / light / dark".into()),
        },
        "showRecent" | "showFavorites" | "stopOnClose" | "closeToTray" | "alwaysOnTop" => {
            if value.is_boolean() {
                Ok(())
            } else {
                Err(format!("{key} 必须是 true 或 false"))
            }
        }
        "idleTimeoutMinutes" => match value.as_u64() {
            Some(n) if n <= 1440 => Ok(()),
            _ => Err("idleTimeoutMinutes 必须是 0 到 1440 之间的整数".into()),
        },
        "dirOrder" => match value.as_array() {
            Some(a)
                if !a.is_empty()
                    && a.iter()
                        .all(|x| x.as_str().map(|s| !s.is_empty()).unwrap_or(false)) =>
            {
                Ok(())
            }
            _ => Err("dirOrder 必须是非空的字符串数组".into()),
        },
        _ => {
            let ok_name = key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                && !key.is_empty();
            if ok_name && value.to_string().len() <= 16 * 1024 {
                Ok(())
            } else {
                Err(format!("不支持的设置项: {key}"))
            }
        }
    }
}

impl Settings {
    pub fn load(path: PathBuf) -> Self {
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.as_object().cloned())
            .map(|m| {
                m.into_iter()
                    .filter(|(k, v)| validate(k, v).is_ok())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            path,
            data: Mutex::new(data),
        }
    }

    /// 完整设置（已保存的值覆盖默认值）。
    pub fn get(&self) -> Value {
        let mut all = defaults();
        for (k, v) in self.data.lock().unwrap().iter() {
            all.insert(k.clone(), v.clone());
        }
        Value::Object(all)
    }

    pub fn dir_order(&self) -> Vec<String> {
        self.get()["dirOrder"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 合并一部分设置并保存。任何一项不合法则整体不生效。
    pub fn patch(&self, patch: &Value) -> Result<Value, String> {
        let obj = patch.as_object().ok_or("设置必须是一个对象")?;
        for (k, v) in obj {
            validate(k, v)?;
        }
        {
            let mut data = self.data.lock().unwrap();
            for (k, v) in obj {
                data.insert(k.clone(), v.clone());
            }
            if let Some(parent) = self.path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(
                &self.path,
                serde_json::to_string_pretty(&*data).unwrap_or_default(),
            )
            .map_err(|e| format!("保存设置失败: {e}"))?;
        }
        Ok(self.get())
    }
}
