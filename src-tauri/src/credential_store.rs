use std::{ffi::c_void, ptr::null_mut};

use windows::{
    Win32::Security::Credentials::{
        CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW,
        CredDeleteW, CredFree, CredReadW, CredWriteW,
    },
    core::{PCWSTR, PWSTR},
};

const LEGACY_API_KEY_TARGET: &str = "Translay/model-api-key";
const API_KEY_TARGET_PREFIX: &str = "Translay/model-api-key";
const API_KEY_USERNAME: &str = "Translay";

#[derive(Clone, Default)]
pub struct CredentialStore;

impl CredentialStore {
    pub fn write_api_key(&self, base_url: &str, api_key: &str) -> Result<(), String> {
        let target = api_key_target(base_url)?;
        self.write_credential(&target, api_key)
    }

    pub fn read_api_key(&self, base_url: &str) -> Result<Option<String>, String> {
        let target = api_key_target(base_url)?;
        self.read_credential(&target)
    }

    pub fn api_key_hint(&self, base_url: &str) -> Result<Option<String>, String> {
        self.read_api_key(base_url)
            .map(|key| key.map(|value| mask_api_key(&value)))
    }

    pub fn clear_api_key(&self, base_url: &str) -> Result<(), String> {
        let target = api_key_target(base_url)?;
        self.clear_credential(&target)
    }

    pub fn migrate_legacy_api_key(&self, base_url: &str) -> Result<(), String> {
        let target = api_key_target(base_url)?;
        if self.read_credential(&target)?.is_some() {
            return self.clear_credential(LEGACY_API_KEY_TARGET);
        }
        let Some(api_key) = self.read_credential(LEGACY_API_KEY_TARGET)? else {
            return Ok(());
        };
        self.write_credential(&target, &api_key)?;
        self.clear_credential(LEGACY_API_KEY_TARGET)
    }

    fn write_credential(&self, target_name: &str, api_key: &str) -> Result<(), String> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err("API Key 不能为空".to_owned());
        }
        let bytes = api_key.as_bytes();
        if bytes.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            return Err("API Key 超过 Windows 凭据管理器长度限制".to_owned());
        }

        let mut target = wide(target_name);
        let mut username = wide(API_KEY_USERNAME);
        let mut blob = bytes.to_vec();
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_mut_ptr()),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(username.as_mut_ptr()),
            ..Default::default()
        };

        unsafe { CredWriteW(&credential, 0) }
            .map_err(|error| format!("保存 API Key 到 Windows 凭据管理器失败：{error}"))
    }

    fn read_credential(&self, target_name: &str) -> Result<Option<String>, String> {
        let target = wide(target_name);
        let mut raw: *mut CREDENTIALW = null_mut();
        if let Err(error) =
            unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut raw) }
        {
            // 0x80070490 is HRESULT_FROM_WIN32(ERROR_NOT_FOUND).
            if error.code().0 as u32 == 0x8007_0490 {
                return Ok(None);
            }
            return Err(format!("读取 Windows 凭据管理器中的 API Key 失败：{error}"));
        }
        if raw.is_null() {
            return Ok(None);
        }

        let result = unsafe {
            let credential = &*raw;
            let bytes = std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            );
            String::from_utf8(bytes.to_vec())
                .map(Some)
                .map_err(|_| "Windows 凭据管理器中的 API Key 不是有效 UTF-8".to_owned())
        };
        unsafe { CredFree(raw.cast::<c_void>()) };
        result
    }

    fn clear_credential(&self, target_name: &str) -> Result<(), String> {
        if self.read_credential(target_name)?.is_none() {
            return Ok(());
        }
        let target = wide(target_name);
        unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) }
            .map_err(|error| format!("删除 Windows 凭据管理器中的 API Key 失败：{error}"))
    }
}

fn api_key_target(base_url: &str) -> Result<String, String> {
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

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn mask_api_key(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_and_username_are_null_terminated_utf16() {
        assert_eq!(wide(LEGACY_API_KEY_TARGET).last(), Some(&0));
        assert_eq!(wide(API_KEY_USERNAME).last(), Some(&0));
    }

    #[test]
    fn api_key_targets_are_scoped_to_the_provider() {
        assert_eq!(
            api_key_target("https://api.deepseek.com").unwrap(),
            "Translay/model-api-key/deepseek"
        );
        assert_eq!(
            api_key_target("https://api.deepseek.com/chat/completions").unwrap(),
            "Translay/model-api-key/deepseek"
        );
        assert_eq!(
            api_key_target("https://open.bigmodel.cn/api/paas/v4").unwrap(),
            "Translay/model-api-key/glm"
        );
        assert_ne!(
            api_key_target("https://example.com/v1").unwrap(),
            api_key_target("https://example.net/v1").unwrap()
        );
    }

    #[test]
    fn api_key_hint_reveals_only_a_safe_prefix_and_suffix() {
        let openai_style_key = ["sk", "-example-secret-8F3A"].concat();
        assert_eq!(mask_api_key(&openai_style_key), "sk-••••••••8F3A");
        assert_eq!(mask_api_key("generic-secret-29BC"), "••••••••29BC");
        assert_eq!(mask_api_key("tiny"), "••••");
    }
}
