// Behavioral tests for the settings state helpers.
import { describe, expect, it } from "vitest";
import type { ModelConfigView, SaveModelConfigInput } from "./modelSettings";
import {
  activeModelSummary,
  apiKeyLabelForPreset,
  apiProviderPresets,
  applyProviderPreset,
  applySavedProviderConfig,
  applyTranslationPreferences,
  connectionFeedbackMessage,
  connectionErrorMessage,
  draftSignature,
  hasUnsavedProviderChanges,
  isEditingActiveConfiguration,
  localProviderPresets,
  providerPresetForConfig,
  reasoningDescriptionForConfig,
  reasoningSupportForConfig,
} from "./settingsState";

const activeApi: ModelConfigView = {
  backend: "api",
  mode: "conversational",
  reasoningEnabled: false,
  local: {
    baseUrl: "http://127.0.0.1:1234/v1",
    model: "qwen3-14B",
  },
  api: {
    baseUrl: "https://api.deepseek.com/chat/completions",
    model: "deepseek-v4-flash",
  },
  localModels: {},
  apiModels: {
    deepseek: "deepseek-v4-flash",
  },
  timeoutSeconds: 60,
  hasApiKey: true,
  apiKeyHint: "sk-••••••••8F3A",
};

describe("settings state", () => {
  it("offers only the six confirmed API quick presets", () => {
    expect(apiProviderPresets.map((preset) => preset.label)).toEqual([
      "DeepSeek",
      "OpenAI",
      "Anthropic",
      "智谱 AI",
      "月之暗面",
      "Google Gemini",
    ]);
  });

  it("offers the five confirmed local tool presets", () => {
    expect(localProviderPresets.map((preset) => preset.label)).toEqual([
      "Ollama",
      "LM Studio",
      "Jan",
      "llama.cpp",
      "vLLM",
    ]);
  });

  it("shows only the active model without repeating its provider", () => {
    expect(activeModelSummary(activeApi)).toBe("deepseek-v4-flash");
    expect(
      activeModelSummary({
        ...activeApi,
        api: { ...activeApi.api, model: "  " },
      }),
    ).toBe("未配置模型");
  });

  it("recognizes whether the visible configuration is the active one", () => {
    const localDraft = { ...activeApi, backend: "local" as const };
    expect(isEditingActiveConfiguration(localDraft, activeApi)).toBe(false);
  });

  it("invalidates a tested draft after its active endpoint changes", () => {
    const input: SaveModelConfigInput = {
      ...activeApi,
      apiKey: null,
    };
    const changed = {
      ...input,
      api: { ...input.api, model: "deepseek-v4-pro" },
    };
    expect(draftSignature(input)).not.toBe(draftSignature(changed));
  });

  it("applies translation preferences without committing model drafts", () => {
    const draft = {
      ...activeApi,
      backend: "local" as const,
      local: {
        baseUrl: "http://127.0.0.1:11434/v1",
        model: "unsaved-model",
      },
      timeoutSeconds: 90,
    };
    const next = applyTranslationPreferences(draft, {
      mode: "academic",
      reasoningEnabled: true,
    });

    expect(next.mode).toBe("academic");
    expect(next.reasoningEnabled).toBe(true);
    expect(next.backend).toBe("local");
    expect(next.local).toEqual(draft.local);
    expect(next.timeoutSeconds).toBe(90);
  });

  it("tracks unsaved changes for the provider being viewed", () => {
    const localDraft = { ...activeApi, backend: "local" as const };
    expect(hasUnsavedProviderChanges(localDraft, activeApi, "")).toBe(false);
    expect(
      hasUnsavedProviderChanges(
        {
          ...localDraft,
          local: { ...localDraft.local, model: "new-local-model" },
        },
        activeApi,
        "",
      ),
    ).toBe(true);
    expect(hasUnsavedProviderChanges(activeApi, activeApi, "sk-new")).toBe(
      true,
    );
  });

  it("merges a saved provider without switching the selected or active source", () => {
    const draft = {
      ...activeApi,
      backend: "local" as const,
      api: { ...activeApi.api, model: "other-unsaved-api-model" },
      local: { ...activeApi.local, model: "new-local-model" },
    };
    const saved = {
      ...activeApi,
      local: { ...activeApi.local, model: "new-local-model" },
    };
    const merged = applySavedProviderConfig(draft, saved, "local");

    expect(merged.backend).toBe("local");
    expect(merged.local.model).toBe("new-local-model");
    expect(merged.api.model).toBe("other-unsaved-api-model");
  });

  it("recognizes built-in provider presets and leaves unknown endpoints custom", () => {
    expect(providerPresetForConfig(activeApi)).toBe("deepseek");
    expect(
      providerPresetForConfig({
        ...activeApi,
        api: {
          baseUrl: "https://open.bigmodel.cn/api/paas/v4/chat/completions",
          model: "glm-5.2",
        },
      }),
    ).toBe("zhipu");
    expect(
      providerPresetForConfig({
        ...activeApi,
        api: {
          baseUrl:
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
          model: "gemini-model",
        },
      }),
    ).toBe("gemini");
    expect(
      providerPresetForConfig({ ...activeApi, backend: "local" }),
    ).toBe("lmstudio");
    expect(
      providerPresetForConfig({
        ...activeApi,
        api: {
          baseUrl: "https://example.com/v1",
          model: "custom-model",
        },
      }),
    ).toBe("custom");
  });

  it("applies presets only to the current draft provider", () => {
    const localDraft = {
      ...activeApi,
      backend: "local" as const,
    };
    const ignored = applyProviderPreset(localDraft, "openai");
    expect(ignored).toEqual(localDraft);

    const zhipu = applyProviderPreset(activeApi, "zhipu");
    expect(zhipu.api).toEqual({
      baseUrl: "https://open.bigmodel.cn/api/paas/v4",
      model: "glm-5.2",
    });
    expect(zhipu.local).toEqual(activeApi.local);
    expect(hasUnsavedProviderChanges(zhipu, activeApi, "")).toBe(true);

    const ollama = applyProviderPreset(localDraft, "ollama");
    expect(ollama.local).toEqual({
      baseUrl: "http://127.0.0.1:11434/v1",
      model: "",
    });
    expect(ollama.localModels.lmstudio).toBe("qwen3-14B");
  });

  it("fills the official default model the first time a provider is used", () => {
    const openai = applyProviderPreset(activeApi, "openai");
    expect(openai.api).toEqual({
      baseUrl: "https://api.openai.com/v1",
      model: "gpt-5.6-terra",
    });
    expect(apiKeyLabelForPreset("anthropic")).toBe("Anthropic API Key");
    expect(apiKeyLabelForPreset("custom")).toBe("API Key");
  });

  it("remembers an editable model name independently for each provider", () => {
    const openai = applyProviderPreset(activeApi, "openai");
    const editedOpenAi = {
      ...openai,
      api: { ...openai.api, model: "my-openai-model" },
    };

    const deepseek = applyProviderPreset(editedOpenAi, "deepseek");
    const openaiAgain = applyProviderPreset(deepseek, "openai");

    expect(deepseek.api.model).toBe("deepseek-v4-flash");
    expect(deepseek.apiModels.openai).toBe("my-openai-model");
    expect(openaiAgain.api.model).toBe("my-openai-model");
  });

  it("remembers an editable model name independently for each local tool", () => {
    const localDraft = { ...activeApi, backend: "local" as const };
    const ollama = applyProviderPreset(localDraft, "ollama");
    const editedOllama = {
      ...ollama,
      local: { ...ollama.local, model: "qwen3:8b" },
    };

    const jan = applyProviderPreset(editedOllama, "jan");
    const ollamaAgain = applyProviderPreset(jan, "ollama");

    expect(jan.local.model).toBe("");
    expect(jan.localModels.ollama).toBe("qwen3:8b");
    expect(ollamaAgain.local.model).toBe("qwen3:8b");
  });

  it("keeps connection feedback concise beside the test action", () => {
    expect(
      connectionFeedbackMessage({
        success: true,
        message: "连接成功，模型已返回响应",
        elapsedMs: 326,
      }),
    ).toBe("连接成功 · 326 ms");
    expect(
      connectionFeedbackMessage({
        success: false,
        message: "  连接超时  ",
        elapsedMs: 60_000,
      }),
    ).toBe("连接失败 · 连接超时");
  });

  it("keeps full provider diagnostics rather than replacing them with a generic failure", () => {
    const detail = '模型 virtual-model 不存在；HTTP 404\nrequest_id=virtual-request; ' + '原始服务端诊断。'.repeat(50);
    expect(connectionFeedbackMessage({success:false,message:detail,elapsedMs:25}))
      .toBe(`连接失败 · ${detail}`);
    expect(connectionErrorMessage(new Error(detail))).toBe(`连接失败 · ${detail}`);
    expect(connectionErrorMessage({message:"服务拒绝访问",code:"virtual_denied"}))
      .toContain('"message":"服务拒绝访问","code":"virtual_denied"');
  });

  it("explains bare HTTP and network codes while retaining the original code", () => {
    expect(connectionErrorMessage("HTTP 401")).toBe("连接失败 · 认证未通过，请检查 API Key（HTTP 401）");
    expect(connectionErrorMessage("ECONNREFUSED")).toContain("确认服务已启动及地址正确（ECONNREFUSED）");
    expect(connectionErrorMessage("HTTP 503")).toContain("服务端暂时无法处理请求（HTTP 503）");
    expect(connectionErrorMessage("HTTP 404：virtual-model 不存在")).toBe("连接失败 · HTTP 404：virtual-model 不存在");
    expect(connectionErrorMessage(" ")).toBe("连接失败，未收到可用的错误信息");
  });

  it("describes exact, minimum and fixed reasoning behavior honestly", () => {
    expect(reasoningSupportForConfig(activeApi)).toBe("exact");
    expect(reasoningDescriptionForConfig(activeApi)).toBe(
      "当前模型已关闭思考，将优先快速响应",
    );

    const gemini = {
      ...activeApi,
      api: {
        baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai",
        model: "gemini-3.6-flash",
      },
    };
    expect(reasoningSupportForConfig(gemini)).toBe("minimum");
    expect(reasoningDescriptionForConfig(gemini)).toBe(
      "该模型不能完全关闭，已降至最低思考",
    );

    const openAiPro = {
      ...activeApi,
      api: {
        baseUrl: "https://api.openai.com/v1",
        model: "gpt-5.6-pro",
      },
    };
    expect(reasoningSupportForConfig(openAiPro)).toBe("fixed");

    const localR1 = {
      ...activeApi,
      backend: "local" as const,
      local: {
        baseUrl: "http://127.0.0.1:1234/v1",
        model: "deepseek-r1-distill-qwen-32b",
      },
    };
    expect(reasoningSupportForConfig(localR1)).toBe("fixed");
    expect(reasoningDescriptionForConfig(localR1)).toBe(
      "该模型固定启用思考，无法通过接口关闭",
    );
  });

  it("uses automatic safe fallback for unknown local models", () => {
    const unknownLocal = {
      ...activeApi,
      backend: "local" as const,
      local: {
        baseUrl: "http://127.0.0.1:9000/v1",
        model: "my-future-local-model",
      },
    };
    expect(reasoningSupportForConfig(unknownLocal)).toBe("automatic");
    expect(reasoningDescriptionForConfig(unknownLocal)).toBe(
      "将尝试关闭思考，不兼容时保持模型默认行为",
    );
  });
});
