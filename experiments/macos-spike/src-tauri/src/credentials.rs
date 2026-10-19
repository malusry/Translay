use translay_core::credentials::Credentials;

#[derive(Clone, Default)]
pub struct CredentialStore;

#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "com.malusry.translay.macos-spike.model-api-key";

#[cfg(target_os = "macos")]
impl Credentials for CredentialStore {
    fn write_api_key(&self, base_url: &str, api_key: &str) -> Result<(), String> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err("API Key 不能为空".into());
        }
        let account = translay_core::credentials::api_key_target(base_url)?;
        security_framework::passwords::set_generic_password(
            KEYCHAIN_SERVICE,
            &account,
            api_key.as_bytes(),
        )
        .map_err(|error| format!("保存 API Key 到钥匙串失败（{}）", error.code()))
    }

    fn read_api_key(&self, base_url: &str) -> Result<Option<String>, String> {
        use security_framework::passwords::{PasswordOptions, generic_password};
        let account = translay_core::credentials::api_key_target(base_url)?;
        match generic_password(PasswordOptions::new_generic_password(
            KEYCHAIN_SERVICE,
            &account,
        )) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "钥匙串中的 API Key 不是有效 UTF-8".into()),
            Err(error) if error.code() == -25300 => Ok(None), // errSecItemNotFound
            Err(error) => Err(format!("读取钥匙串中的 API Key 失败（{}）", error.code())),
        }
    }

    fn clear_api_key(&self, base_url: &str) -> Result<(), String> {
        let account = translay_core::credentials::api_key_target(base_url)?;
        match security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, &account) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => Ok(()),
            Err(error) => Err(format!("删除钥匙串中的 API Key 失败（{}）", error.code())),
        }
    }
}

// Portable CI can compile the app surface without touching this host's secrets.
#[cfg(not(target_os = "macos"))]
impl Credentials for CredentialStore {
    fn write_api_key(&self, _: &str, _: &str) -> Result<(), String> {
        Err("钥匙串需要 macOS".into())
    }
    fn read_api_key(&self, _: &str) -> Result<Option<String>, String> {
        Err("钥匙串需要 macOS".into())
    }
    fn clear_api_key(&self, _: &str) -> Result<(), String> {
        Err("钥匙串需要 macOS".into())
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use security_framework::passwords::{
        PasswordOptions, delete_generic_password, generic_password, set_generic_password,
    };
    #[test]
    fn disposable_keychain_roundtrip() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let service = format!("com.malusry.translay.tests.{}.{stamp}", std::process::id());
        let account = "disposable-fixture";
        struct Cleanup<'a>(&'a str, &'a str);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                let _ = delete_generic_password(self.0, self.1);
            }
        }
        let _cleanup = Cleanup(&service, account);
        assert_eq!(
            generic_password(PasswordOptions::new_generic_password(&service, account))
                .unwrap_err()
                .code(),
            -25300
        );
        set_generic_password(&service, account, b"fake-fixture-first").unwrap();
        assert_eq!(
            generic_password(PasswordOptions::new_generic_password(&service, account)).unwrap(),
            b"fake-fixture-first"
        );
        set_generic_password(&service, account, b"fake-fixture-updated").unwrap();
        assert_eq!(
            generic_password(PasswordOptions::new_generic_password(&service, account)).unwrap(),
            b"fake-fixture-updated"
        );
        delete_generic_password(&service, account).unwrap();
        assert_eq!(
            generic_password(PasswordOptions::new_generic_password(&service, account))
                .unwrap_err()
                .code(),
            -25300
        );
    }
}
