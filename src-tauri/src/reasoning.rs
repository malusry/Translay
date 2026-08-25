use reqwest::Url;
use serde::Serialize;

use crate::model_config::{EndpointConfig, ModelBackend, ModelConfig};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OpenAiReasoningPolicy {
    pub thinking: Option<ThinkingControl>,
    pub reasoning_effort: Option<&'static str>,
    pub prompt_control: Option<&'static str>,
}

impl OpenAiReasoningPolicy {
    pub(crate) fn has_request_control(self) -> bool {
        self.thinking.is_some() || self.reasoning_effort.is_some()
    }

    pub(crate) fn without_request_control(self) -> Self {
        Self {
            thinking: None,
            reasoning_effort: None,
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ThinkingControl {
    #[serde(rename = "type")]
    pub kind: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct AnthropicThinkingControl {
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
}

pub(crate) fn openai_reasoning_policy(
    config: &ModelConfig,
    endpoint: &EndpointConfig,
) -> OpenAiReasoningPolicy {
    let enabled = config.reasoning_enabled;
    let model = normalized_model(&endpoint.model);
    let provider = provider_for(config, endpoint);

    match provider {
        Provider::DeepSeek if is_fixed_reasoning_model(&model) => OpenAiReasoningPolicy::default(),
        Provider::DeepSeek => thinking_type_policy(enabled),
        Provider::Zhipu if zhipu_supports_thinking(&model) => thinking_type_policy(enabled),
        Provider::Zhipu => OpenAiReasoningPolicy::default(),
        Provider::Moonshot => moonshot_policy(enabled, &model),
        Provider::Gemini => gemini_policy(enabled, &model),
        Provider::OpenAi => openai_official_policy(enabled, &model),
        Provider::Ollama => ollama_policy(enabled, &model),
        Provider::Vllm => vllm_policy(enabled, &model),
        Provider::LmStudio => lm_studio_policy(enabled, &model),
        Provider::Generic => generic_compatible_policy(config, enabled, &model),
    }
}

pub(crate) fn anthropic_reasoning_policy(
    model: &str,
    enabled: bool,
) -> Option<AnthropicThinkingControl> {
    if !enabled {
        return None;
    }
    let model = normalized_model(model);
    if is_adaptive_claude(&model) {
        Some(AnthropicThinkingControl {
            kind: "adaptive",
            budget_tokens: None,
        })
    } else if is_extended_thinking_claude(&model) {
        Some(AnthropicThinkingControl {
            kind: "enabled",
            budget_tokens: Some(1024),
        })
    } else {
        None
    }
}

fn thinking_type_policy(enabled: bool) -> OpenAiReasoningPolicy {
    OpenAiReasoningPolicy {
        thinking: Some(ThinkingControl {
            kind: if enabled { "enabled" } else { "disabled" },
        }),
        ..OpenAiReasoningPolicy::default()
    }
}

fn effort_policy(
    enabled: bool,
    disabled_effort: &'static str,
    prompt_control: Option<&'static str>,
) -> OpenAiReasoningPolicy {
    OpenAiReasoningPolicy {
        reasoning_effort: Some(if enabled { "medium" } else { disabled_effort }),
        prompt_control,
        ..OpenAiReasoningPolicy::default()
    }
}

fn moonshot_policy(enabled: bool, model: &str) -> OpenAiReasoningPolicy {
    if contains_any(model, &["kimi-k2.5", "kimi-k2.6"]) {
        thinking_type_policy(enabled)
    } else if model.contains("kimi-k3") {
        effort_policy(enabled, "low", None)
    } else {
        OpenAiReasoningPolicy::default()
    }
}

fn gemini_policy(enabled: bool, model: &str) -> OpenAiReasoningPolicy {
    if model.contains("gemini-2.5") && !model.contains("pro") {
        effort_policy(enabled, "none", None)
    } else if model.contains("gemini-2.5-pro") || model_starts_with_major(model, "gemini-", 3) {
        effort_policy(enabled, "minimal", None)
    } else {
        OpenAiReasoningPolicy::default()
    }
}

fn openai_official_policy(enabled: bool, model: &str) -> OpenAiReasoningPolicy {
    if model.contains("-pro") {
        OpenAiReasoningPolicy::default()
    } else if model.contains("gpt-oss") {
        effort_policy(enabled, "low", None)
    } else if openai_model_supports_none(model) {
        effort_policy(enabled, "none", None)
    } else if is_openai_reasoning_model(model) {
        effort_policy(enabled, "low", None)
    } else {
        OpenAiReasoningPolicy::default()
    }
}

fn ollama_policy(enabled: bool, model: &str) -> OpenAiReasoningPolicy {
    let disabled_effort = if model.contains("gpt-oss") {
        "low"
    } else {
        "none"
    };
    effort_policy(
        enabled,
        disabled_effort,
        qwen_prompt_control(enabled, model),
    )
}

fn vllm_policy(enabled: bool, model: &str) -> OpenAiReasoningPolicy {
    if is_fixed_reasoning_model(model) {
        return effort_policy(enabled, "low", None);
    }
    effort_policy(enabled, "none", qwen_prompt_control(enabled, model))
}

fn lm_studio_policy(enabled: bool, model: &str) -> OpenAiReasoningPolicy {
    let disabled_effort = if is_fixed_reasoning_model(model) {
        "low"
    } else {
        "none"
    };
    effort_policy(
        enabled,
        disabled_effort,
        qwen_prompt_control(enabled, model),
    )
}

fn generic_compatible_policy(
    config: &ModelConfig,
    enabled: bool,
    model: &str,
) -> OpenAiReasoningPolicy {
    if config.backend != ModelBackend::Local && !is_recognized_reasoning_model(model) {
        return OpenAiReasoningPolicy::default();
    }
    let disabled_effort = if is_fixed_reasoning_model(model) {
        "low"
    } else {
        "none"
    };
    effort_policy(
        enabled,
        disabled_effort,
        (config.backend == ModelBackend::Local)
            .then(|| qwen_prompt_control(enabled, model))
            .flatten(),
    )
}

fn qwen_prompt_control(enabled: bool, model: &str) -> Option<&'static str> {
    is_qwen_hybrid_model(model).then_some(if enabled { "/think" } else { "/no_think" })
}

fn normalized_model(model: &str) -> String {
    model.trim().to_ascii_lowercase()
}

fn zhipu_supports_thinking(model: &str) -> bool {
    contains_any(
        model,
        &["glm-4.5", "glm-4.6", "glm-4.7", "glm-5", "glm-6", "glm-z1"],
    )
}

fn is_adaptive_claude(model: &str) -> bool {
    contains_any(
        model,
        &[
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-opus-5",
            "claude-sonnet-5",
            "claude-haiku-5",
        ],
    )
}

fn is_extended_thinking_claude(model: &str) -> bool {
    contains_any(
        model,
        &[
            "claude-3-7",
            "claude-sonnet-4",
            "claude-opus-4",
            "claude-haiku-4",
        ],
    )
}

fn is_qwen_hybrid_model(model: &str) -> bool {
    contains_any(model, &["qwen3", "qwen-3"])
}

fn openai_model_supports_none(model: &str) -> bool {
    if !model.starts_with("gpt-5") || model.contains("gpt-5-pro") {
        return false;
    }
    model
        .strip_prefix("gpt-5.")
        .and_then(|suffix| suffix.split(['-', '_']).next())
        .and_then(|minor| minor.parse::<u32>().ok())
        .is_some_and(|minor| minor >= 1)
}

fn is_openai_reasoning_model(model: &str) -> bool {
    model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
        || model.starts_with("gpt-5")
        || model.contains("gpt-oss")
}

fn is_fixed_reasoning_model(model: &str) -> bool {
    contains_any(
        model,
        &[
            "gpt-oss",
            "deepseek-r1",
            "deepseek-reasoner",
            "kimi-k2.7-code",
            "kimi-k3",
            "command-a-reasoning",
        ],
    )
}

fn is_recognized_reasoning_model(model: &str) -> bool {
    is_openai_reasoning_model(model)
        || is_qwen_hybrid_model(model)
        || contains_any(
            model,
            &[
                "deepseek-r1",
                "deepseek-v3.1",
                "deepseek-v4",
                "deepseek-reasoner",
                "gemma-4",
                "granite-3.2",
                "nemotron",
                "command-a-reasoning",
                "glm-4.5",
                "glm-4.6",
                "glm-4.7",
                "glm-5",
                "kimi-k2.5",
                "kimi-k2.6",
                "kimi-k2.7-code",
                "kimi-k3",
            ],
        )
}

fn model_starts_with_major(model: &str, prefix: &str, minimum: u32) -> bool {
    model
        .strip_prefix(prefix)
        .and_then(|suffix| suffix.split(['.', '-', '_']).next())
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= minimum)
}

fn contains_any(value: &str, candidates: &[&str]) -> bool {
    candidates.iter().any(|candidate| value.contains(candidate))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Provider {
    DeepSeek,
    OpenAi,
    Zhipu,
    Moonshot,
    Gemini,
    Ollama,
    Vllm,
    LmStudio,
    Generic,
}

fn provider_for(config: &ModelConfig, endpoint: &EndpointConfig) -> Provider {
    let Ok(url) = Url::parse(endpoint.base_url.trim()) else {
        return Provider::Generic;
    };
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    match host.as_str() {
        "api.deepseek.com" => Provider::DeepSeek,
        "api.openai.com" => Provider::OpenAi,
        "open.bigmodel.cn" => Provider::Zhipu,
        "api.moonshot.cn" | "api.kimi.com" => Provider::Moonshot,
        "generativelanguage.googleapis.com" => Provider::Gemini,
        _ if config.backend == ModelBackend::Local => {
            let port = url.port_or_known_default();
            if port == Some(11434) || host.contains("ollama") {
                Provider::Ollama
            } else if port == Some(1234) || host.contains("lmstudio") {
                Provider::LmStudio
            } else if port == Some(8000) || host.contains("vllm") {
                Provider::Vllm
            } else {
                Provider::Generic
            }
        }
        _ => Provider::Generic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_config(base_url: &str, model: &str, enabled: bool) -> ModelConfig {
        let mut config = ModelConfig::default();
        config.backend = ModelBackend::Api;
        config.api.base_url = base_url.to_owned();
        config.api.model = model.to_owned();
        config.reasoning_enabled = enabled;
        config
    }

    fn local_config(base_url: &str, model: &str, enabled: bool) -> ModelConfig {
        let mut config = ModelConfig::default();
        config.local.base_url = base_url.to_owned();
        config.local.model = model.to_owned();
        config.reasoning_enabled = enabled;
        config
    }

    fn policy(config: &ModelConfig) -> OpenAiReasoningPolicy {
        let endpoint = match config.backend {
            ModelBackend::Local => &config.local,
            ModelBackend::Api => &config.api,
        };
        openai_reasoning_policy(config, endpoint)
    }

    #[test]
    fn official_thinking_type_providers_use_their_native_switch() {
        for (base_url, model) in [
            ("https://api.deepseek.com", "deepseek-v4-flash"),
            ("https://open.bigmodel.cn/api/paas/v4", "glm-5.2"),
            ("https://api.moonshot.cn/v1", "kimi-k2.6"),
        ] {
            let disabled = policy(&api_config(base_url, model, false));
            let enabled = policy(&api_config(base_url, model, true));
            assert_eq!(disabled.thinking.unwrap().kind, "disabled");
            assert_eq!(enabled.thinking.unwrap().kind, "enabled");
        }
    }

    #[test]
    fn official_openai_distinguishes_none_from_minimum_effort() {
        let modern = policy(&api_config(
            "https://api.openai.com/v1",
            "gpt-5.6-terra",
            false,
        ));
        let legacy = policy(&api_config("https://api.openai.com/v1", "o3", false));
        let fixed = policy(&api_config(
            "https://api.openai.com/v1",
            "gpt-oss-120b",
            false,
        ));
        let pro = policy(&api_config(
            "https://api.openai.com/v1",
            "gpt-5.6-pro",
            false,
        ));
        assert_eq!(modern.reasoning_effort, Some("none"));
        assert_eq!(legacy.reasoning_effort, Some("low"));
        assert_eq!(fixed.reasoning_effort, Some("low"));
        assert!(!pro.has_request_control());
    }

    #[test]
    fn gemini_uses_none_only_for_models_that_can_stop_thinking() {
        let flash = policy(&api_config(
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "gemini-2.5-flash",
            false,
        ));
        let pro = policy(&api_config(
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "gemini-2.5-pro",
            false,
        ));
        let gemini_three = policy(&api_config(
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "gemini-3.6-flash",
            false,
        ));
        assert_eq!(flash.reasoning_effort, Some("none"));
        assert_eq!(pro.reasoning_effort, Some("minimal"));
        assert_eq!(gemini_three.reasoning_effort, Some("minimal"));
    }

    #[test]
    fn ollama_and_vllm_receive_structured_controls() {
        let ollama = policy(&local_config(
            "http://127.0.0.1:11434/v1",
            "qwen3:14b",
            false,
        ));
        let vllm = policy(&local_config(
            "http://127.0.0.1:8000/v1",
            "Qwen/Qwen3-32B",
            true,
        ));
        assert_eq!(ollama.reasoning_effort, Some("none"));
        assert_eq!(ollama.prompt_control, Some("/no_think"));
        assert_eq!(vllm.reasoning_effort, Some("medium"));
        assert_eq!(vllm.prompt_control, Some("/think"));
    }

    #[test]
    fn generic_unknown_models_do_not_receive_risky_extra_fields() {
        let config = api_config("https://example.com/v1", "ordinary-chat-model", false);
        assert_eq!(policy(&config), OpenAiReasoningPolicy::default());
    }

    #[test]
    fn anthropic_uses_adaptive_or_budgeted_thinking_by_generation() {
        assert_eq!(
            anthropic_reasoning_policy("claude-sonnet-5", true)
                .unwrap()
                .kind,
            "adaptive"
        );
        assert_eq!(
            anthropic_reasoning_policy("claude-3-7-sonnet-latest", true)
                .unwrap()
                .budget_tokens,
            Some(1024)
        );
        assert!(anthropic_reasoning_policy("claude-sonnet-5", false).is_none());
    }
}
