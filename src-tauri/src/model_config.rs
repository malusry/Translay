use std::{
    fs,
    path::PathBuf,
    sync::{Arc, RwLock},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::translation::TranslationMode;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelBackend {
    #[default]
    Local,
    Api,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointConfig {
    pub base_url: String,
    pub model: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    pub backend: ModelBackend,
    pub mode: TranslationMode,
    #[serde(default)]
    pub reasoning_enabled: bool,
    pub local: EndpointConfig,
    pub api: EndpointConfig,
    pub timeout_seconds: u64,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            backend: ModelBackend::Local,
            mode: TranslationMode::Conversational,
            reasoning_enabled: false,
            local: EndpointConfig {
                base_url: "http://127.0.0.1:11434/v1".to_owned(),
                model: String::new(),
            },
            api: EndpointConfig {
                base_url: "https://api.openai.com/v1".to_owned(),
                model: String::new(),
            },
            timeout_seconds: 60,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveModelConfig {
    pub backend: ModelBackend,
    pub mode: TranslationMode,
    pub reasoning_enabled: bool,
    pub local: EndpointConfig,
    pub api: EndpointConfig,
    pub timeout_seconds: u64,
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyStatus {
    pub has_api_key: bool,
    pub api_key_hint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfigView {
    #[serde(flatten)]
    pub config: ModelConfig,
    pub has_api_key: bool,
    pub api_key_hint: Option<String>,
}

#[derive(Clone)]
pub struct ModelConfigStore {
    path: PathBuf,
    inner: Arc<RwLock<ModelConfig>>,
}

impl ModelConfigStore {
    pub fn load(app: &AppHandle) -> Result<Self, String> {
        let directory = app
            .path()
            .app_config_dir()
            .map_err(|error| format!("无法取得配置目录：{error}"))?;
        fs::create_dir_all(&directory).map_err(|error| format!("无法创建配置目录：{error}"))?;
        let path = directory.join("model-config.json");
        let config = if path.exists() {
            let bytes = fs::read(&path).map_err(|error| format!("无法读取模型配置：{error}"))?;
            serde_json::from_slice(&bytes).map_err(|error| format!("模型配置格式无效：{error}"))?
        } else {
            ModelConfig::default()
        };
        Ok(Self {
            path,
            inner: Arc::new(RwLock::new(config)),
        })
    }

    pub fn get(&self) -> ModelConfig {
        self.inner
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn save(&self, config: ModelConfig) -> Result<(), String> {
        validate_config(&config)?;
        let data = serde_json::to_vec_pretty(&config)
            .map_err(|error| format!("无法序列化模型配置：{error}"))?;
        fs::write(&self.path, data).map_err(|error| format!("无法保存模型配置：{error}"))?;
        *self
            .inner
            .write()
            .unwrap_or_else(|error| error.into_inner()) = config;
        Ok(())
    }
}

pub fn validate_config(config: &ModelConfig) -> Result<(), String> {
    if !(5..=180).contains(&config.timeout_seconds) {
        return Err("请求超时必须在 5 到 180 秒之间".to_owned());
    }
    validate_endpoint(&config.local, false, config.backend == ModelBackend::Local)?;
    validate_endpoint(&config.api, true, config.backend == ModelBackend::Api)?;
    Ok(())
}

fn validate_endpoint(
    endpoint: &EndpointConfig,
    require_https: bool,
    require_model: bool,
) -> Result<(), String> {
    if require_model && endpoint.model.trim().is_empty() {
        return Err("请先右键托盘打开“配置”，在模型页面填写模型名称".to_owned());
    }
    let url = reqwest::Url::parse(endpoint.base_url.trim())
        .map_err(|_| "服务地址不是有效 URL".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("服务地址只支持 http 或 https".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("服务地址中不能包含用户名或密码".to_owned());
    }
    let is_loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if require_https && url.scheme() != "https" && !is_loopback {
        return Err("API 服务必须使用 HTTPS；仅本机地址可以使用 HTTP".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn default_uses_confirmed_product_terms() {
        assert_eq!(ModelConfig::default().mode, TranslationMode::Conversational);
    }

    #[test]
    fn validates_local_and_secure_api_endpoints() {
        assert!(validate_config(&valid()).is_ok());
    }

    #[test]
    fn rejects_plain_http_remote_api_with_credentials() {
        let mut config = valid();
        config.api.base_url = "http://user:pass@example.com/v1".to_owned();
        assert!(validate_config(&config).is_err());
    }
}
