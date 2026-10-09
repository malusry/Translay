use crate::tone_translation::TranslationOutput;
use std::{
    collections::VecDeque,
    future::Future,
    sync::Mutex,
    time::{Duration, Instant},
};

const TTL: Duration = Duration::from_secs(10 * 60);
const MAX_ENTRIES: usize = 24;
const MAX_BYTES: usize = 1024 * 1024;

#[derive(PartialEq, Eq)]
pub(crate) struct CacheKey {
    pub input: String,
    pub credential: [u64; 2],
}

impl CacheKey {
    pub(crate) fn new(
        request: &crate::translation::TranslationRequest,
        config: &crate::model_config::ModelConfig,
        credential: [u64; 2],
    ) -> Result<Self, String> {
        Ok(Self {
            input: serde_json::to_string(&(request, config))
                .map_err(|e| format!("无法准备翻译请求：{e}"))?,
            credential,
        })
    }
}

struct Entry {
    key: CacheKey,
    output: TranslationOutput,
    created: Instant,
    bytes: usize,
}

#[derive(Default)]
pub(crate) struct TranslationCache {
    entries: VecDeque<Entry>,
}

impl TranslationCache {
    fn prune(&mut self, now: Instant) {
        self.entries
            .retain(|e| now.saturating_duration_since(e.created) < TTL);
    }

    fn get(&mut self, key: &CacheKey, now: Instant) -> Option<TranslationOutput> {
        self.prune(now);
        let index = self.entries.iter().position(|e| &e.key == key)?;
        let entry = self.entries.remove(index)?;
        let output = entry.output.clone();
        self.entries.push_back(entry);
        Some(output)
    }

    fn insert(&mut self, key: CacheKey, output: TranslationOutput, now: Instant) {
        self.prune(now);
        let bytes =
            key.input.len() + output.text.len() + output.tone_note.as_ref().map_or(0, String::len);
        // Large documents should never evict the whole small-snippet cache.
        if bytes > MAX_BYTES || output.text.trim().is_empty() {
            return;
        }
        self.entries.retain(|e| e.key != key);
        while self.entries.len() >= MAX_ENTRIES
            || self.entries.iter().map(|e| e.bytes).sum::<usize>() + bytes > MAX_BYTES
        {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            key,
            output,
            created: now,
            bytes,
        });
    }
}

pub(crate) async fn cached<F, Fut>(
    cache: &Mutex<TranslationCache>,
    key: CacheKey,
    fetch: F,
) -> Result<TranslationOutput, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<TranslationOutput, String>>,
{
    if let Some(output) = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key, Instant::now())
    {
        return Ok(output);
    }
    // Never hold the cache lock across a model request. Only finalized successful
    // outputs are retained; failures remain retryable. In-flight calls are independent.
    let output = fetch().await?;
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, output.clone(), Instant::now());
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(s: &str) -> CacheKey {
        CacheKey {
            input: s.into(),
            credential: [1, 2],
        }
    }

    #[test]
    fn hit_preserves_note_and_expires_without_sliding_deadline() {
        let mut cache = TranslationCache::default();
        let now = Instant::now();
        let output = TranslationOutput {
            text: "有道理。".into(),
            tone_note: Some("表示认可。".into()),
        };
        cache.insert(key("a"), output.clone(), now);
        assert_eq!(
            cache.get(&key("a"), now + TTL - Duration::from_secs(1)),
            Some(output)
        );
        assert!(cache.get(&key("a"), now + TTL).is_none());
    }

    #[test]
    fn credentials_and_inputs_do_not_cross_hit() {
        let mut cache = TranslationCache::default();
        let now = Instant::now();
        cache.insert(key("a"), "译文".to_owned().into(), now);
        assert!(cache.get(&key("b"), now).is_none());
        assert!(
            cache
                .get(
                    &CacheKey {
                        input: "a".into(),
                        credential: [3, 4]
                    },
                    now
                )
                .is_none()
        );
    }

    #[test]
    fn changed_translation_context_and_model_settings_miss() {
        use crate::{
            model_config::ModelConfig,
            translation::{ContentType, TranslationMode, TranslationRequest},
        };
        let request = TranslationRequest::new(
            "That tracks.",
            TranslationMode::Conversational,
            "Reader",
            ContentType::Unknown,
        );
        let config = ModelConfig::default();
        let mut cache = TranslationCache::default();
        let now = Instant::now();
        cache.insert(
            CacheKey::new(&request, &config, [1, 2]).unwrap(),
            "有道理。".to_owned().into(),
            now,
        );
        for field in 0..7 {
            let mut changed = request.clone();
            match field {
                0 => changed.source_text.push(' '),
                1 => changed.context_before = Some("Different context".into()),
                2 => changed.context_after = Some("Different context".into()),
                3 => changed.mode = TranslationMode::Academic,
                4 => changed.target_language = "en".into(),
                5 => changed.application = "Other reader".into(),
                _ => changed.preserve_format = false,
            }
            assert!(
                cache
                    .get(&CacheKey::new(&changed, &config, [1, 2]).unwrap(), now)
                    .is_none()
            );
        }
        for field in 0..4 {
            let mut changed = config.clone();
            match field {
                0 => changed.local.model = "other-model".into(),
                1 => changed.local.base_url = "http://localhost:12345/v1".into(),
                2 => changed.reasoning_enabled = !changed.reasoning_enabled,
                _ => changed.backend = crate::model_config::ModelBackend::Api,
            }
            assert!(
                cache
                    .get(&CacheKey::new(&request, &changed, [1, 2]).unwrap(), now)
                    .is_none()
            );
        }
    }

    #[test]
    fn bounded_lru_and_large_entry_bypass() {
        let mut cache = TranslationCache::default();
        let now = Instant::now();
        for i in 0..MAX_ENTRIES {
            cache.insert(key(&i.to_string()), "译文".to_owned().into(), now);
        }
        cache.get(&key("0"), now);
        cache.insert(key("new"), "译文".to_owned().into(), now);
        assert!(cache.get(&key("1"), now).is_none());
        assert!(cache.get(&key("0"), now).is_some());
        cache.insert(key("large"), "x".repeat(MAX_BYTES).into(), now);
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        assert!(cache.get(&key("large"), now).is_none());
        for i in 0..5 {
            cache.insert(
                key(&format!("big{i}")),
                "x".repeat(MAX_BYTES / 3).into(),
                now,
            );
        }
        assert!(cache.entries.iter().map(|e| e.bytes).sum::<usize>() <= MAX_BYTES);
    }

    #[test]
    fn successful_result_skips_fetch_but_failure_is_retried() {
        tauri::async_runtime::block_on(async {
            let cache = Mutex::new(TranslationCache::default());
            assert!(
                cached(&cache, key("a"), || async { Err("offline".into()) })
                    .await
                    .is_err()
            );
            let result = cached(&cache, key("a"), || async {
                Ok("译文".to_owned().into())
            })
            .await
            .unwrap();
            let hit = cached(&cache, key("a"), || async {
                panic!("cache hit must not call model")
            })
            .await
            .unwrap();
            assert_eq!(result, hit);
        });
    }
}
