use std::{collections::BTreeSet, time::Duration};

use reqwest::{Client, Url};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiscoveryKind {
    Ollama,
    LmStudio,
    OpenAiCompatible,
}

#[derive(Deserialize)]
struct OllamaResponse {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    #[serde(default)]
    model: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct LmStudioResponse {
    #[serde(default)]
    models: Vec<LmStudioModel>,
}

#[derive(Deserialize)]
struct LmStudioModel {
    #[serde(rename = "type", default)]
    model_type: String,
    #[serde(default)]
    loaded_instances: Vec<LmStudioInstance>,
}

#[derive(Deserialize)]
struct LmStudioInstance {
    #[serde(default)]
    id: String,
}

#[derive(Deserialize)]
struct OpenAiModelsResponse {
    #[serde(default)]
    data: Vec<OpenAiModel>,
}

#[derive(Deserialize)]
struct OpenAiModel {
    #[serde(default)]
    id: String,
}

pub(super) async fn detect(provider: &str, base_url: &str) -> Option<String> {
    let (url, kind) = discovery_request(provider, base_url)?;
    let client = Client::builder()
        .connect_timeout(Duration::from_millis(450))
        .timeout(Duration::from_millis(1400))
        .build()
        .ok()?;
    let response = client.get(url).send().await.ok()?.error_for_status().ok()?;

    match kind {
        DiscoveryKind::Ollama => {
            let payload = response.json::<OllamaResponse>().await.ok()?;
            unique_model(payload.models.into_iter().map(|model| {
                if model.model.trim().is_empty() {
                    model.name
                } else {
                    model.model
                }
            }))
        }
        DiscoveryKind::LmStudio => {
            let payload = response.json::<LmStudioResponse>().await.ok()?;
            unique_model(
                payload
                    .models
                    .into_iter()
                    .filter(|model| model.model_type == "llm")
                    .flat_map(|model| model.loaded_instances)
                    .map(|instance| instance.id),
            )
        }
        DiscoveryKind::OpenAiCompatible => {
            let payload = response.json::<OpenAiModelsResponse>().await.ok()?;
            unique_model(payload.data.into_iter().map(|model| model.id))
        }
    }
}

fn discovery_request(provider: &str, base_url: &str) -> Option<(Url, DiscoveryKind)> {
    let kind = match provider {
        "ollama" => DiscoveryKind::Ollama,
        "lmstudio" => DiscoveryKind::LmStudio,
        "jan" | "llamacpp" | "vllm" => DiscoveryKind::OpenAiCompatible,
        _ => return None,
    };
    let mut url = Url::parse(base_url.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
    {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    match kind {
        DiscoveryKind::Ollama => url.set_path("/api/ps"),
        DiscoveryKind::LmStudio => url.set_path("/api/v1/models"),
        DiscoveryKind::OpenAiCompatible => {
            let path = url.path().trim_end_matches('/');
            let path = if path.ends_with("/models") {
                path.to_owned()
            } else if path.ends_with("/v1") {
                format!("{path}/models")
            } else {
                format!("{path}/v1/models")
            };
            url.set_path(&path);
        }
    }
    Some((url, kind))
}

fn unique_model(models: impl IntoIterator<Item = String>) -> Option<String> {
    let models = models
        .into_iter()
        .map(|model| model.trim().to_owned())
        .filter(|model| !model.is_empty())
        .collect::<BTreeSet<_>>();
    if models.len() == 1 {
        models.into_iter().next()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_only_loopback_discovery_requests() {
        let (ollama, kind) = discovery_request("ollama", "http://127.0.0.1:11434/v1").unwrap();
        assert_eq!(kind, DiscoveryKind::Ollama);
        assert_eq!(ollama.as_str(), "http://127.0.0.1:11434/api/ps");

        let (jan, kind) = discovery_request("jan", "http://localhost:1337/v1").unwrap();
        assert_eq!(kind, DiscoveryKind::OpenAiCompatible);
        assert_eq!(jan.as_str(), "http://localhost:1337/v1/models");

        assert!(discovery_request("ollama", "https://example.com/v1").is_none());
        assert!(discovery_request("unknown", "http://127.0.0.1:9000/v1").is_none());
    }

    #[test]
    fn fills_only_a_single_unambiguous_model() {
        assert_eq!(
            unique_model([" qwen3:8b ".to_owned(), "qwen3:8b".to_owned()]),
            Some("qwen3:8b".to_owned())
        );
        assert_eq!(
            unique_model(["qwen3:8b".to_owned(), "gemma3:4b".to_owned()]),
            None
        );
        assert_eq!(unique_model([String::new()]), None);
    }
}
