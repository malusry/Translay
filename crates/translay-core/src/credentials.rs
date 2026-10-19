/// Implemented by each operating system; secrets never enter configuration JSON.
pub trait Credentials: Clone + Send + Sync + 'static {
    fn write_api_key(&self, base_url: &str, api_key: &str) -> Result<(), String>;
    fn read_api_key(&self, base_url: &str) -> Result<Option<String>, String>;
    fn clear_api_key(&self, base_url: &str) -> Result<(), String>;
    fn api_key_hint(&self, base_url: &str) -> Result<Option<String>, String> {
        self.read_api_key(base_url)
            .map(|key| key.map(|value| mask_api_key(&value)))
    }
}

const API_KEY_TARGET_PREFIX: &str = "Translay/model-api-key";
pub fn api_key_target(base_url: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(base_url.trim())
        .map_err(|_| "服务地址不是有效 URL，无法定位对应的 API Key".to_owned())?;
    let host = url
        .host_str()
        .ok_or_else(|| "服务地址缺少主机名，无法定位对应的 API Key".to_owned())?
        .to_ascii_lowercase();
    let scope = if host == "api.deepseek.com" {
        "deepseek".to_owned()
    } else if host == "open.bigmodel.cn" {
        "glm".to_owned()
    } else {
        let port = url
            .port_or_known_default()
            .map(|value| format!(":{value}"))
            .unwrap_or_default();
        let origin = format!("{}://{host}{port}", url.scheme().to_ascii_lowercase());
        format!("custom-{:016x}", stable_hash(&origin))
    };
    Ok(format!("{API_KEY_TARGET_PREFIX}/{scope}"))
}

fn stable_hash(value: &str) -> u64 {
    value
        .as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

pub fn mask_api_key(value: &str) -> String {
    let value = value.trim();
    let characters = value.chars().collect::<Vec<_>>();
    if characters.len() <= 4 {
        return "••••".to_owned();
    }
    let prefix = if value.starts_with("sk-") { "sk-" } else { "" };
    let suffix = characters[characters.len() - 4..]
        .iter()
        .collect::<String>();
    format!("{prefix}••••••••{suffix}")
}
