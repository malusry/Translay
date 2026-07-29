use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
        System::Threading::CreateMutexW,
    },
    core::PCWSTR,
};

const INSTANCE_MUTEX_NAME: &str = "Local\\Translay.SystemOverlay.Singleton";

pub struct SingleInstanceGuard {
    handle: HANDLE,
}

impl SingleInstanceGuard {
    pub fn acquire() -> Result<Option<Self>, String> {
        Self::acquire_named(INSTANCE_MUTEX_NAME)
    }

    fn acquire_named(name: &str) -> Result<Option<Self>, String> {
        let wide_name = name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        // SAFETY: wide_name is null terminated and remains alive for the call.
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(wide_name.as_ptr())) }
            .map_err(|error| format!("创建 Translay 单实例锁失败：{error}"))?;
        // GetLastError must be read immediately after CreateMutexW. The call
        // succeeds for both a newly created and an already existing mutex.
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if already_exists {
            // SAFETY: handle was returned successfully and is closed once.
            let _ = unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self { handle }))
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns the process-local handle.
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_instance_is_rejected_until_the_owner_exits() {
        let name = format!("Local\\Translay.SystemOverlay.Test.{}", std::process::id());
        let first = SingleInstanceGuard::acquire_named(&name)
            .expect("first acquisition should succeed")
            .expect("first acquisition should own the mutex");
        assert!(
            SingleInstanceGuard::acquire_named(&name)
                .expect("duplicate acquisition should be handled")
                .is_none()
        );

        drop(first);
        assert!(
            SingleInstanceGuard::acquire_named(&name)
                .expect("mutex should be reusable after owner exits")
                .is_some()
        );
    }
}
