use crate::{
    model_config::{EndpointConfig, ModelBackend, ModelConfig, validate_config},
    reasoning::{
        AnthropicThinkingControl, OpenAiReasoningPolicy, ThinkingControl,
        anthropic_reasoning_policy, openai_reasoning_policy,
    },
};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct ModelClient {
    client: Client,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub success: bool,
    pub message: String,
    pub elapsed_ms: u128,
}

impl ModelClient {
    pub fn with_client(client: Client) -> Self {
        Self { client }
    }

    pub async fn test_connection_with_config(
        &self,
        config: ModelConfig,
        api_key: Option<String>,
    ) -> ConnectionTestResult {
        let started = Instant::now();
        let result = self
            .send_messages(
                &config,
                vec![ChatMessage {
                    role: "user",
                    content: "Reply with OK only.".to_owned(),
                }],
                api_key.as_deref(),
            )
            .await;
        ConnectionTestResult {
            success: result.is_ok(),
            message: result
                .map(|_| "连接成功，模型已返回响应".to_owned())
                .unwrap_or_else(|error| error),
            elapsed_ms: started.elapsed().as_millis(),
        }
    }

    pub async fn send_messages(
        &self,
        config: &ModelConfig,
        mut messages: Vec<ChatMessage>,
        api_key: Option<&str>,
    ) -> Result<String, String> {
        validate_config(config)?;
        let endpoint = active_endpoint(config);
        let api_key = if config.backend == ModelBackend::Api {
            Some(
                api_key
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| "尚未保存 API Key，请打开“配置”中的模型页面".to_owned())?,
            )
        } else {
            None
        };
        if is_anthropic_api(config, endpoint) {
            let api_key = api_key
                .as_deref()
                .ok_or_else(|| "尚未保存 API Key，请打开“配置”中的模型页面".to_owned())?;
            return self
                .send_anthropic_messages(config, endpoint, messages, api_key)
                .await;
        }

        let reasoning = apply_reasoning_policy(config, endpoint, &mut messages);
        let url = chat_completions_url(&endpoint.base_url)?;
        let (mut status, mut body) = self
            .post_chat_completion(
                config,
                endpoint,
                url.clone(),
                &messages,
                api_key.as_deref(),
                reasoning,
            )
            .await?;
        if reasoning.has_request_control() && should_retry_without_reasoning(status, &body) {
            (status, body) = self
                .post_chat_completion(
                    config,
                    endpoint,
                    url,
                    &messages,
                    api_key.as_deref(),
                    reasoning.without_request_control(),
                )
                .await?;
        }
        if !status.is_success() {
            return Err(http_error_message(status, &body));
        }
        let json: serde_json::Value = serde_json::from_str(&body).map_err(|_| {
            "模型返回的内容不是 JSON，请检查服务地址是否指向 OpenAI 兼容接口".to_owned()
        })?;
        if let Some(message) = json
            .get("error")
            .and_then(|error| error.get("message").or(Some(error)))
            .and_then(serde_json::Value::as_str)
        {
            return Err(format!("模型服务错误：{message}"));
        }
        let completion: ChatCompletionResponse = serde_json::from_value(json).map_err(|_| {
            "模型响应缺少 choices.message.content，请检查模型服务兼容模式".to_owned()
        })?;
        completion
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .ok_or_else(|| "模型响应中没有可用文本".to_owned())
    }

    async fn post_chat_completion(
        &self,
        config: &ModelConfig,
        endpoint: &EndpointConfig,
        url: Url,
        messages: &[ChatMessage],
        api_key: Option<&str>,
        reasoning: OpenAiReasoningPolicy,
    ) -> Result<(StatusCode, String), String> {
        let mut request = self
            .client
            .post(url)
            .timeout(Duration::from_secs(config.timeout_seconds))
            .json(&ChatCompletionRequest {
                model: endpoint.model.trim(),
                messages,
                stream: false,
                thinking: reasoning.thinking,
                reasoning_effort: reasoning.reasoning_effort,
            });
        if let Some(api_key) = api_key {
            request = request.bearer_auth(api_key);
        }
        let response = request
            .send()
            .await
            .map_err(|error| request_error_message(error, config.backend))?;
        read_model_response(response, config.backend).await
    }

    async fn send_anthropic_messages(
        &self,
        config: &ModelConfig,
        endpoint: &EndpointConfig,
        messages: Vec<ChatMessage>,
        api_key: &str,
    ) -> Result<String, String> {
        let system = messages
            .iter()
            .filter(|message| message.role == "system")
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let messages = messages
            .into_iter()
            .filter(|message| message.role != "system")
            .collect::<Vec<_>>();
        let url = anthropic_messages_url(&endpoint.base_url)?;
        let thinking = anthropic_reasoning_policy(&endpoint.model, config.reasoning_enabled);
        let (mut status, mut body) = self
            .post_anthropic_messages(
                config,
                endpoint,
                url.clone(),
                &messages,
                (!system.is_empty()).then_some(system.as_str()),
                api_key,
                thinking,
            )
            .await?;
        if thinking.is_some() && should_retry_without_reasoning(status, &body) {
            (status, body) = self
                .post_anthropic_messages(
                    config,
                    endpoint,
                    url,
                    &messages,
                    (!system.is_empty()).then_some(system.as_str()),
                    api_key,
                    None,
                )
                .await?;
        }
        if !status.is_success() {
            return Err(http_error_message(status, &body));
        }
        let completion: AnthropicMessageResponse = serde_json::from_str(&body)
            .map_err(|_| "Anthropic 响应缺少 content 文本，请检查模型名称与服务地址".to_owned())?;
        let output = completion
            .content
            .into_iter()
            .filter(|block| block.kind == "text")
            .filter_map(|block| block.text)
            .collect::<Vec<_>>()
            .join("");
        if output.trim().is_empty() {
            Err("模型响应中没有可用文本".to_owned())
        } else {
            Ok(output)
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn post_anthropic_messages(
        &self,
        config: &ModelConfig,
        endpoint: &EndpointConfig,
        url: Url,
        messages: &[ChatMessage],
        system: Option<&str>,
        api_key: &str,
        thinking: Option<AnthropicThinkingControl>,
    ) -> Result<(StatusCode, String), String> {
        let response = self
            .client
            .post(url)
            .timeout(Duration::from_secs(config.timeout_seconds))
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&AnthropicMessageRequest {
                model: endpoint.model.trim(),
                max_tokens: 4096,
                messages,
                system,
                stream: false,
                thinking,
            })
            .send()
            .await
            .map_err(|error| request_error_message(error, config.backend))?;
        read_model_response(response, config.backend).await
    }
}

pub async fn read_model_response(
    response: reqwest::Response,
    backend: ModelBackend,
) -> Result<(StatusCode, String), String> {
    let status = response.status();
    let body = response.text().await.map_err(|error| {
        if error.is_timeout() {
            timeout_error_message(backend).to_owned()
        } else {
            "模型响应接收中断，请重试".to_owned()
        }
    })?;
    Ok((status, body))
}

pub fn active_endpoint(config: &ModelConfig) -> &EndpointConfig {
    match config.backend {
        ModelBackend::Local => &config.local,
        ModelBackend::Api => &config.api,
    }
}

pub fn apply_reasoning_policy(
    config: &ModelConfig,
    endpoint: &EndpointConfig,
    messages: &mut [ChatMessage],
) -> OpenAiReasoningPolicy {
    let policy = openai_reasoning_policy(config, endpoint);
    let Some(control) = policy.prompt_control else {
        return policy;
    };
    let Some(last_user_message) = messages
        .iter_mut()
        .rev()
        .find(|message| message.role == "user")
    else {
        return policy;
    };
    if !last_user_message.content.contains(control) {
        last_user_message.content.push('\n');
        last_user_message.content.push_str(control);
    }
    policy
}

pub fn is_anthropic_api(config: &ModelConfig, endpoint: &EndpointConfig) -> bool {
    config.backend == ModelBackend::Api
        && Url::parse(endpoint.base_url.trim())
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .is_some_and(|host| host.eq_ignore_ascii_case("api.anthropic.com"))
}

pub fn anthropic_messages_url(base_url: &str) -> Result<Url, String> {
    let base = base_url.trim().trim_end_matches('/');
    let parsed = Url::parse(base).ok();
    let value = if base.ends_with("/messages") {
        base.to_owned()
    } else if parsed
        .as_ref()
        .is_some_and(|url| url.path().trim_matches('/').is_empty())
    {
        format!("{base}/v1/messages")
    } else {
        format!("{base}/messages")
    };
    Url::parse(&value).map_err(|_| "无法构建 Anthropic Messages 地址".to_owned())
}

pub fn chat_completions_url(base_url: &str) -> Result<Url, String> {
    let base = base_url.trim().trim_end_matches('/');
    let parsed = Url::parse(base).ok();
    let value = if base.ends_with("/chat/completions") {
        base.to_owned()
    } else if parsed.as_ref().is_some_and(|url| {
        url.host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("api.deepseek.com"))
            && url.path().trim_matches('/').is_empty()
    }) {
        format!("{base}/chat/completions")
    } else if parsed
        .as_ref()
        .is_some_and(|url| url.path().trim_matches('/').is_empty())
    {
        format!("{base}/v1/chat/completions")
    } else {
        format!("{base}/chat/completions")
    };
    Url::parse(&value).map_err(|_| "无法构建 Chat Completions 地址".to_owned())
}

pub fn request_error_message(error: reqwest::Error, backend: ModelBackend) -> String {
    if error.is_timeout() {
        timeout_error_message(backend).to_owned()
    } else if error.is_connect() {
        connect_error_message(backend).to_owned()
    } else {
        "模型请求未完成，请检查服务后重试".to_owned()
    }
}

pub fn timeout_error_message(backend: ModelBackend) -> &'static str {
    match backend {
        ModelBackend::Local => "连接超时，请检查本地服务",
        ModelBackend::Api => "连接超时，请稍后重试",
    }
}

pub fn connect_error_message(backend: ModelBackend) -> &'static str {
    match backend {
        ModelBackend::Local => "连接失败，请启动本地模型",
        ModelBackend::Api => "连接失败，请检查网络或地址",
    }
}

pub fn http_error_message(status: StatusCode, body: &str) -> String {
    // Inspect only error fields. Never expose raw HTML/JSON or a provider's
    // echoed prompt, URL or credentials in the reading overlay.
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let error = parsed.as_ref().map(|v| v.get("error").unwrap_or(v));
    let field = |name: &str| {
        error
            .and_then(|v| v.get(name))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    let code = field("code");
    let kind = field("type");
    let message = field("message");
    let is_code = |values: &[&str]| values.iter().any(|v| code == *v || kind == *v);
    if status == StatusCode::UNAUTHORIZED {
        return "身份验证失败，请在配置中检查 API Key".to_owned();
    }
    if status == StatusCode::FORBIDDEN {
        return "服务拒绝访问，请检查账户或模型权限".to_owned();
    }
    if status.is_server_error() {
        return format!("模型服务暂时不可用，请稍后重试（HTTP {}）", status.as_u16());
    }
    if status == StatusCode::PAYMENT_REQUIRED
        || (status.is_client_error()
            && is_code(&[
                "insufficient_quota",
                "insufficient_balance",
                "credit_balance_too_low",
                "billing_hard_limit_reached",
            ]))
    {
        return "账户余额或可用额度不足，请在模型服务商处检查".to_owned();
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return "请求过于频繁或已达速率限制，请稍后重试".to_owned();
    }
    if status == StatusCode::REQUEST_TIMEOUT {
        return "模型服务等待超时，请稍后重试".to_owned();
    }
    if status == StatusCode::PAYLOAD_TOO_LARGE
        || (status == StatusCode::BAD_REQUEST
            && (is_code(&["context_length_exceeded"])
                || message.contains("exceeds the maximum context length")
                || message.contains("exceeds the context window")))
    {
        return "内容超过模型可处理长度，请缩小选取范围后重试".to_owned();
    }
    if status.is_client_error() && is_code(&["model_not_found", "model_not_available"]) {
        return "模型不存在或当前账户不可用，请在配置中检查模型名称与权限".to_owned();
    }
    match status {
        StatusCode::NOT_FOUND => "找不到模型或服务接口，请在配置中检查模型名称与地址".to_owned(),
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => {
            "模型服务不接受当前请求，请在配置中检查模型与服务是否兼容".to_owned()
        }
        _ => format!(
            "模型服务返回异常，请检查服务状态（HTTP {}）",
            status.as_u16()
        ),
    }
}

pub fn should_retry_without_reasoning(status: StatusCode, body: &str) -> bool {
    if !matches!(
        status,
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ) {
        return false;
    }
    let body = body.to_ascii_lowercase();
    body.contains("reasoning")
        || body.contains("thinking")
        || body.contains("unknown field")
        || body.contains("unexpected field")
        || body.contains("extra inputs")
        || body.contains("not permitted")
}

#[derive(Serialize)]
pub struct ChatCompletionRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessage],
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<&'static str>,
}

#[derive(Serialize)]
pub struct AnthropicMessageRequest<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    pub messages: &'a [ChatMessage],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<&'a str>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<AnthropicThinkingControl>,
}

#[derive(Serialize)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

#[derive(Deserialize)]
pub struct ChatCompletionResponse {
    pub choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
pub struct ChatChoice {
    pub message: ChatResponseMessage,
}

#[derive(Deserialize)]
pub struct ChatResponseMessage {
    pub content: Option<String>,
}

#[derive(Deserialize)]
pub struct AnthropicMessageResponse {
    pub content: Vec<AnthropicContentBlock>,
}

#[derive(Deserialize)]
pub struct AnthropicContentBlock {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: Option<String>,
}
