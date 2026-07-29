// Pure settings state helpers, kept separate from Tauri commands and React.
import type {
  ConnectionTestResult,
  ModelConfigView,
  SaveModelConfigInput,
} from "./modelSettings";

export type ConnectionHealth = "idle" | "working" | "success" | "error";

export type ProviderPreset =
  | "ollama"
  | "lm-studio"
  | "deepseek"
  | "glm"
  | "custom";

export type TranslationPreferences = Pick<
  ModelConfigView,
  "mode" | "reasoningEnabled"
>;

const providerPresetEndpoints = {
  ollama: {
    backend: "local",
    baseUrl: "http://127.0.0.1:11434/v1",
  },
  "lm-studio": {
    backend: "local",
    baseUrl: "http://127.0.0.1:1234/v1",
  },
  deepseek: {
    backend: "api",
    baseUrl: "https://api.deepseek.com",
    model: "deepseek-v4-flash",
  },
  glm: {
    backend: "api",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    model: "glm-5.2",
  },
} as const;

function normalizeEndpoint(value: string): string {
  return value.trim().toLowerCase().replace(/\/+$/, "");
}

export function providerPresetForConfig(
  config: ModelConfigView,
): ProviderPreset {
  const endpoint = normalizeEndpoint(config[config.backend].baseUrl);

  if (config.backend === "local") {
    if (
      endpoint === normalizeEndpoint(providerPresetEndpoints.ollama.baseUrl) ||
      endpoint === "http://localhost:11434/v1"
    ) {
      return "ollama";
    }
    if (
      endpoint ===
        normalizeEndpoint(providerPresetEndpoints["lm-studio"].baseUrl) ||
      endpoint === "http://localhost:1234/v1"
    ) {
      return "lm-studio";
    }
    return "custom";
  }

  if (
    endpoint === normalizeEndpoint(providerPresetEndpoints.deepseek.baseUrl) ||
    endpoint ===
      `${normalizeEndpoint(providerPresetEndpoints.deepseek.baseUrl)}/chat/completions`
  ) {
    return "deepseek";
  }
  if (
    endpoint === normalizeEndpoint(providerPresetEndpoints.glm.baseUrl) ||
    endpoint ===
      `${normalizeEndpoint(providerPresetEndpoints.glm.baseUrl)}/chat/completions`
  ) {
    return "glm";
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

  const next = providerPresetEndpoints[preset];
  if (next.backend !== config.backend) {
    return config;
  }

  if (next.backend === "local") {
    return {
      ...config,
      local: {
        ...config.local,
        baseUrl: next.baseUrl,
      },
    };
  }

  return {
    ...config,
    api: {
      baseUrl: next.baseUrl,
      model: next.model,
    },
  };
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

export function activationActionLabel(
  backend: ModelConfigView["backend"],
): string {
  return backend === "api" ? "切换到 API" : "切换到本地模型";
}

export function connectionFeedbackMessage(
  result: ConnectionTestResult,
): string {
  if (result.success) {
    return `连接成功 · ${result.elapsedMs} ms`;
  }
  return result.message.trim() || "连接失败";
}
