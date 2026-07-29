use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Debug)]
pub struct CancellationToken(AtomicBool);

impl CancellationToken {
    fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct CaptureRequest {
    pub id: u64,
    pub cancellation: Arc<CancellationToken>,
}

#[derive(Debug, Default)]
pub struct CaptureSession {
    latest_id: AtomicU64,
    current: Mutex<Option<Arc<CancellationToken>>>,
}

impl CaptureSession {
    pub fn begin(&self) -> CaptureRequest {
        let id = self.latest_id.fetch_add(1, Ordering::AcqRel) + 1;
        let token = Arc::new(CancellationToken::new());
        let mut current = self.current.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = current.replace(token.clone()) {
            previous.cancel();
        }
        CaptureRequest {
            id,
            cancellation: token,
        }
    }

    pub fn cancel_current(&self) {
        self.latest_id.fetch_add(1, Ordering::AcqRel);
        if let Some(token) = self
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            token.cancel();
        }
    }

    pub fn is_current(&self, request: &CaptureRequest) -> bool {
        !request.cancellation.is_cancelled() && self.latest_id.load(Ordering::Acquire) == request.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_request_cancels_and_supersedes_older_request() {
        let session = CaptureSession::default();
        let old = session.begin();
        let new = session.begin();

        assert!(old.cancellation.is_cancelled());
        assert!(!session.is_current(&old));
        assert!(session.is_current(&new));
    }

    #[test]
    fn explicit_cancel_invalidates_current_request() {
        let session = CaptureSession::default();
        let request = session.begin();
        session.cancel_current();
        assert!(!session.is_current(&request));
    }
}
