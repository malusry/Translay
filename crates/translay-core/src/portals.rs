pub const PROVIDER_API_PORTALS: [(&str, &str); 11] = [
    ("deepseek", "https://platform.deepseek.com/api_keys"),
    ("openai", "https://platform.openai.com/api-keys"),
    ("anthropic", "https://platform.claude.com/settings/keys"),
    (
        "zhipu",
        "https://open.bigmodel.cn/usercenter/proj-mgmt/apikeys",
    ),
    ("moonshot", "https://platform.moonshot.cn/console/api-keys"),
    ("gemini", "https://aistudio.google.com/apikey"),
    ("ollama", "https://docs.ollama.com/api/openai-compatibility"),
    ("lmstudio", "https://lmstudio.ai/docs/developer"),
    ("jan", "https://www.jan.ai/docs/desktop/api-server"),
    (
        "llamacpp",
        "https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md",
    ),
    (
        "vllm",
        "https://docs.vllm.ai/en/stable/getting_started/quickstart/",
    ),
];

pub fn provider_api_portal_url(provider: &str) -> Option<&'static str> {
    PROVIDER_API_PORTALS
        .iter()
        .find_map(|(id, url)| (*id == provider).then_some(*url))
}
