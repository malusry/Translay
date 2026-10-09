// Pure settings state helpers, kept separate from Tauri commands and React.
import type {
  ConnectionTestResult,
  ModelConfigView,
  SaveModelConfigInput,
} from "./modelSettings";

export type ConnectionHealth = "idle" | "working" | "success" | "error";

export const apiProviderPresets = [
  {
    id: "deepseek",
    label: "DeepSeek",
    baseUrl: "https://api.deepseek.com",
    defaultModel: "deepseek-v4-flash",
    apiKeyLabel: "DeepSeek API Key",
  },
  {
    id: "openai",
    label: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-5.6-terra",
    apiKeyLabel: "OpenAI API Key",
  },
  {
    id: "anthropic",
    label: "Anthropic",
    baseUrl: "https://api.anthropic.com/v1",
    defaultModel: "claude-sonnet-5",
    apiKeyLabel: "Anthropic API Key",
  },
  {
    id: "zhipu",
    label: "智谱 AI",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    defaultModel: "glm-5.2",
    apiKeyLabel: "智谱 API Key",
  },
  {
    id: "moonshot",
    label: "月之暗面",
    baseUrl: "https://api.moonshot.cn/v1",
    defaultModel: "kimi-k2.6",
    apiKeyLabel: "月之暗面 API Key",
  },
  {
    id: "gemini",
    label: "Google Gemini",
    baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
    defaultModel: "gemini-3.6-flash",
    apiKeyLabel: "Gemini API Key",
  },
] as const;

export const localProviderPresets = [
  {
    id: "ollama",
    label: "Ollama",
    baseUrl: "http://127.0.0.1:11434/v1",
  },
  {
    id: "lmstudio",
    label: "LM Studio",
    baseUrl: "http://127.0.0.1:1234/v1",
  },
  {
    id: "jan",
    label: "Jan",
    baseUrl: "http://127.0.0.1:1337/v1",
  },
  {
    id: "llamacpp",
    label: "llama.cpp",
    baseUrl: "http://127.0.0.1:8080/v1",
  },
  {
    id: "vllm",
    label: "vLLM",
    baseUrl: "http://127.0.0.1:8000/v1",
  },
] as const;

export type ApiProviderPreset = (typeof apiProviderPresets)[number]["id"];
export type LocalProviderPreset =
  (typeof localProviderPresets)[number]["id"];
export type ProviderPortalPreset = ApiProviderPreset | LocalProviderPreset;
export type ProviderPreset = ProviderPortalPreset | "custom";

export type TranslationPreferences = Pick<
  ModelConfigView,
  "mode" | "reasoningEnabled"
>;

export type ReasoningSupport =
  | "exact"
  | "minimum"
  | "fixed"
  | "unavailable"
  | "automatic";

function normalizeEndpoint(value: string): string {
  return value.trim().toLowerCase().replace(/\/+$/, "");
}

export function providerPresetForConfig(
  config: ModelConfigView,
): ProviderPreset {
  const endpoint = normalizeEndpoint(config[config.backend].baseUrl);

  const presets =
    config.backend === "api" ? apiProviderPresets : localProviderPresets;
  for (const preset of presets) {
    const baseUrl = normalizeEndpoint(preset.baseUrl);
    if (
      endpoint === baseUrl ||
      endpoint === `${baseUrl}/chat/completions` ||
      endpoint === `${baseUrl}/messages`
    ) {
      return preset.id;
    }
  }
  return "custom";
}

export function applyProviderPreset(
  config: ModelConfigView,
  preset: ProviderPreset,
): ModelConfigView {
  if (preset === "custom") {
    return config;
  }

  const isApiPreset = apiProviderPresets.some(
    (candidate) => candidate.id === preset,
  );
  if ((config.backend === "api") !== isApiPreset) {
    return config;
  }

  if (config.backend === "local") {
    const next = localProviderPresets.find(
      (candidate) => candidate.id === preset,
    )!;
    const currentPreset = providerPresetForConfig(config);
    const localModels = { ...config.localModels };
    if (currentPreset !== "custom" && config.local.model.trim()) {
      localModels[currentPreset] = config.local.model.trim();
    }
    const rememberedModel = localModels[preset]?.trim();

    return {
      ...config,
      localModels,
      local: {
        baseUrl: next.baseUrl,
        model:
          rememberedModel ||
          (currentPreset === "custom" ? config.local.model : ""),
      },
    };
  }

  const next = apiProviderPresets.find(
    (candidate) => candidate.id === preset,
  )!;
  const currentPreset = providerPresetForConfig(config);
  const apiModels = { ...config.apiModels };
  if (currentPreset !== "custom" && config.api.model.trim()) {
    apiModels[currentPreset] = config.api.model.trim();
  }
  const rememberedModel = apiModels[preset]?.trim();

  return {
    ...config,
    apiModels,
    api: {
      baseUrl: next.baseUrl,
      model: rememberedModel || next.defaultModel,
    },
  };
}

export function apiKeyLabelForPreset(preset: ProviderPreset): string {
  if (preset === "custom") {
    return "API Key";
  }
  return (
    apiProviderPresets.find((candidate) => candidate.id === preset)
      ?.apiKeyLabel ?? "API Key"
  );
}

export function applyTranslationPreferences(
  config: ModelConfigView,
  preferences: TranslationPreferences,
): ModelConfigView {
  return {
    ...config,
    ...preferences,
  };
}

export function hasUnsavedProviderChanges(
  draft: ModelConfigView,
  saved: ModelConfigView | null,
  apiKey: string,
): boolean {
  if (!saved) {
    return true;
  }
  const provider = draft.backend;
  const draftEndpoint = draft[provider];
  const savedEndpoint = saved[provider];
  return (
    draftEndpoint.baseUrl.trim() !== savedEndpoint.baseUrl.trim() ||
    draftEndpoint.model.trim() !== savedEndpoint.model.trim() ||
    draft.timeoutSeconds !== saved.timeoutSeconds ||
    (provider === "api" && apiKey.trim().length > 0)
  );
}

export function applySavedProviderConfig(
  draft: ModelConfigView,
  saved: ModelConfigView,
  provider: ModelConfigView["backend"],
): ModelConfigView {
  return {
    ...draft,
    mode: saved.mode,
    reasoningEnabled: saved.reasoningEnabled,
    localModels: saved.localModels,
    apiModels: saved.apiModels,
    [provider]: saved[provider],
    timeoutSeconds: saved.timeoutSeconds,
    hasApiKey: saved.hasApiKey,
    apiKeyHint: saved.apiKeyHint,
  };
}

export function activeModelSummary(config: ModelConfigView): string {
  const endpoint = config[config.backend];
  const model = endpoint.model.trim();
  return model || "未配置模型";
}

export function isEditingActiveConfiguration(
  draft: ModelConfigView,
  active: ModelConfigView,
): boolean {
  if (draft.backend !== active.backend) {
    return false;
  }
  const draftEndpoint = draft[draft.backend];
  const activeEndpoint = active[active.backend];
  return (
    draftEndpoint.baseUrl.trim() === activeEndpoint.baseUrl.trim() &&
    draftEndpoint.model.trim() === activeEndpoint.model.trim() &&
    draft.timeoutSeconds === active.timeoutSeconds &&
    draft.reasoningEnabled === active.reasoningEnabled
  );
}

export function draftSignature(input: SaveModelConfigInput): string {
  const endpoint = input[input.backend];
  return JSON.stringify({
    backend: input.backend,
    baseUrl: endpoint.baseUrl.trim(),
    model: endpoint.model.trim(),
    timeoutSeconds: input.timeoutSeconds,
    reasoningEnabled: input.reasoningEnabled,
    apiKeySource:
      input.backend === "api" && input.apiKey?.trim() ? "new" : "saved",
  });
}

export function connectionFeedbackMessage(
  result: ConnectionTestResult,
): string {
  if (result.success) {
    return `连接成功 · ${result.elapsedMs} ms`;
  }
  return connectionErrorMessage(result.message);
}

export function connectionErrorMessage(error: unknown): string {
  let detail: string;
  if (error instanceof Error) {
    detail = error.message;
  } else if (typeof error === "string") {
    detail = error;
  } else {
    try {
      detail = JSON.stringify(error) ?? "";
    } catch {
      detail = String(error);
    }
  }
  detail = detail.trim();
  if (!detail) return "连接失败，未收到可用的错误信息";

  // Explain bare diagnostics only; preserve provider prose and full raw details.
  const http = /^HTTP\s+(\d{3})$/i.exec(detail);
  const httpHints: Record<string, string> = {
    "401": "认证未通过，请检查 API Key",
    "403": "服务拒绝访问，请检查模型权限",
    "404": "服务地址或模型不存在，请检查配置",
    "408": "服务响应超时",
    "429": "请求受限，请稍后再试",
  };
  const networkHints: Record<string, string> = {
    ECONNREFUSED: "服务拒绝连接，请确认服务已启动及地址正确",
    ENOTFOUND: "无法解析服务地址，请检查地址与网络",
    ETIMEDOUT: "连接超时，请检查服务与网络",
  };
  const hint = http
    ? httpHints[http[1]] ?? (http[1].startsWith("5") ? "服务端暂时无法处理请求" : undefined)
    : networkHints[detail.toUpperCase()];
  if (hint) return `连接失败 · ${hint}（${detail}）`;
  return detail.startsWith("连接失败") ? detail : `连接失败 · ${detail}`;
}

export function reasoningDescriptionForConfig(
  config: ModelConfigView,
): string {
  const support = reasoningSupportForConfig(config);
  if (support === "exact") {
    return config.reasoningEnabled
      ? "当前模型支持按开关启用思考"
      : "当前模型已关闭思考，将优先快速响应";
  }
  if (support === "minimum") {
    return config.reasoningEnabled
      ? "当前模型已使用适中的思考强度"
      : "该模型不能完全关闭，已降至最低思考";
  }
  if (support === "fixed") {
    return "该模型固定启用思考，无法通过接口关闭";
  }
  if (support === "unavailable") {
    return "该模型没有可用的思考控制，将保持默认行为";
  }
  return config.reasoningEnabled
    ? "将自动匹配模型的思考能力"
    : "将尝试关闭思考，不兼容时保持模型默认行为";
}

export function reasoningSupportForConfig(
  config: ModelConfigView,
): ReasoningSupport {
  const endpoint = config[config.backend];
  const model = endpoint.model.trim().toLowerCase();
  const preset = providerPresetForConfig(config);

  if (preset === "deepseek") {
    return containsAny(model, ["deepseek-r1", "deepseek-reasoner"])
      ? "fixed"
      : "exact";
  }
  if (preset === "openai") {
    if (model.includes("-pro")) {
      return "fixed";
    }
    if (model.includes("gpt-oss")) {
      return "minimum";
    }
    if (openAiSupportsNoReasoning(model)) {
      return "exact";
    }
    if (isOpenAiReasoningModel(model)) {
      return "minimum";
    }
    return "unavailable";
  }
  if (preset === "anthropic") {
    return isClaudeThinkingModel(model) ? "exact" : "unavailable";
  }
  if (preset === "zhipu") {
    return containsAny(model, [
      "glm-4.5",
      "glm-4.6",
      "glm-4.7",
      "glm-5",
      "glm-6",
      "glm-z1",
    ])
      ? "exact"
      : "unavailable";
  }
  if (preset === "moonshot") {
    if (containsAny(model, ["kimi-k2.5", "kimi-k2.6"])) {
      return "exact";
    }
    if (model.includes("kimi-k3")) {
      return "minimum";
    }
    if (model.includes("kimi-k2.7-code")) {
      return "fixed";
    }
    return "unavailable";
  }
  if (preset === "gemini") {
    if (model.includes("gemini-2.5") && !model.includes("pro")) {
      return "exact";
    }
    if (
      model.includes("gemini-2.5-pro") ||
      modelMajorAtLeast(model, "gemini-", 3)
    ) {
      return "minimum";
    }
    return "unavailable";
  }

  if (isFixedReasoningModel(model)) {
    return model.includes("deepseek-r1") || model.includes("kimi-k2.7-code")
      ? "fixed"
      : "minimum";
  }
  if (isRecognizedReasoningModel(model)) {
    return "exact";
  }
  return config.backend === "local" ? "automatic" : "unavailable";
}

function openAiSupportsNoReasoning(model: string): boolean {
  const match = /^gpt-5\.(\d+)/.exec(model);
  return match !== null && Number(match[1]) >= 1;
}

function isOpenAiReasoningModel(model: string): boolean {
  return /^(o1|o3|o4|gpt-5)/.test(model) || model.includes("gpt-oss");
}

function isClaudeThinkingModel(model: string): boolean {
  return containsAny(model, [
    "claude-3-7",
    "claude-sonnet-4",
    "claude-opus-4",
    "claude-haiku-4",
    "claude-sonnet-5",
    "claude-opus-5",
    "claude-haiku-5",
  ]);
}

function isFixedReasoningModel(model: string): boolean {
  return containsAny(model, [
    "gpt-oss",
    "deepseek-r1",
    "deepseek-reasoner",
    "kimi-k2.7-code",
    "kimi-k3",
    "command-a-reasoning",
  ]);
}

function isRecognizedReasoningModel(model: string): boolean {
  return (
    isOpenAiReasoningModel(model) ||
    containsAny(model, [
      "qwen3",
      "qwen-3",
      "deepseek-v3.1",
      "deepseek-v4",
      "gemma-4",
      "granite-3.2",
      "nemotron",
      "glm-4.5",
      "glm-4.6",
      "glm-4.7",
      "glm-5",
      "kimi-k2.5",
      "kimi-k2.6",
    ])
  );
}

function modelMajorAtLeast(
  model: string,
  prefix: string,
  minimum: number,
): boolean {
  if (!model.startsWith(prefix)) {
    return false;
  }
  const major = Number(model.slice(prefix.length).split(/[.\-_]/)[0]);
  return Number.isFinite(major) && major >= minimum;
}

function containsAny(value: string, candidates: string[]): boolean {
  return candidates.some((candidate) => value.includes(candidate));
}
