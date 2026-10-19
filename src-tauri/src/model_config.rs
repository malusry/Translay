// Windows application paths and legacy migration stay in this adapter.
use std::{fs, path::Path};
use tauri::{AppHandle, Manager};
pub use translay_core::model_config::*;
const LEGACY_APP_IDENTIFIER: &str = "com.translay.techspike";

pub fn load(app: &AppHandle) -> Result<ModelConfigStore, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("无法取得配置目录：{error}"))?;
    let path = directory.join(MODEL_CONFIG_FILENAME);
    migrate_legacy_model_config(&path)?;
    fs::create_dir_all(&directory).map_err(|error| format!("无法创建配置目录：{error}"))?;
    ModelConfigStore::load_from_path(path)
}

fn migrate_legacy_model_config(current_path: &Path) -> Result<bool, String> {
    if current_path.exists() {
        return Ok(false);
    }
    let current_directory = current_path
        .parent()
        .ok_or_else(|| "正式配置路径缺少父目录".to_owned())?;
    let config_root = current_directory
        .parent()
        .ok_or_else(|| "正式配置路径缺少配置根目录".to_owned())?;
    let legacy_path = config_root
        .join(LEGACY_APP_IDENTIFIER)
        .join(MODEL_CONFIG_FILENAME);
    if !legacy_path.exists() {
        return Ok(false);
    }

    let bytes = fs::read(&legacy_path).map_err(|error| format!("无法读取旧版模型配置：{error}"))?;
    serde_json::from_slice::<ModelConfig>(&bytes)
        .map_err(|error| format!("旧版模型配置格式无效：{error}"))?;
    fs::create_dir_all(current_directory)
        .map_err(|error| format!("无法创建正式配置目录：{error}"))?;
    fs::write(current_path, bytes).map_err(|error| format!("无法迁移旧版模型配置：{error}"))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn valid() -> ModelConfig {
        ModelConfig {
            local: EndpointConfig {
                base_url: "http://localhost:11434/v1".to_owned(),
                model: "qwen".to_owned(),
            },
            api: EndpointConfig {
                base_url: "https://example.com/v1".to_owned(),
                model: "model".to_owned(),
            },
            ..ModelConfig::default()
        }
    }

    #[test]
    fn migrates_legacy_config_without_removing_the_source() {
        let root = temporary_test_directory("legacy-migration");
        let legacy_path = root.join(LEGACY_APP_IDENTIFIER).join(MODEL_CONFIG_FILENAME);
        let current_path = root
            .join("com.malusry.translay")
            .join(MODEL_CONFIG_FILENAME);
        fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        let expected = serde_json::to_vec_pretty(&valid()).unwrap();
        fs::write(&legacy_path, &expected).unwrap();

        assert!(migrate_legacy_model_config(&current_path).unwrap());
        assert_eq!(fs::read(&current_path).unwrap(), expected);
        assert!(legacy_path.exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_formal_config_is_never_overwritten() {
        let root = temporary_test_directory("current-config-wins");
        let legacy_path = root.join(LEGACY_APP_IDENTIFIER).join(MODEL_CONFIG_FILENAME);
        let current_path = root
            .join("com.malusry.translay")
            .join(MODEL_CONFIG_FILENAME);
        fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        fs::create_dir_all(current_path.parent().unwrap()).unwrap();
        fs::write(&legacy_path, serde_json::to_vec(&valid()).unwrap()).unwrap();
        fs::write(&current_path, b"current").unwrap();

        assert!(!migrate_legacy_model_config(&current_path).unwrap());
        assert_eq!(fs::read(&current_path).unwrap(), b"current");

        fs::remove_dir_all(root).unwrap();
    }

    fn temporary_test_directory(name: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "translay-{name}-{}-{timestamp}-{sequence}",
            std::process::id()
        ))
    }
}
