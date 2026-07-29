// Behavioral tests for the settings state helpers.
import { describe, expect, it } from "vitest";
import type { ModelConfigView, SaveModelConfigInput } from "./modelSettings";
import {
  activeModelSummary,
  activationActionLabel,
  applyProviderPreset,
  applySavedProviderConfig,
  applyTranslationPreferences,
  connectionFeedbackMessage,
  draftSignature,
  hasUnsavedProviderChanges,
  isEditingActiveConfiguration,
  providerPresetForConfig,
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
  timeoutSeconds: 60,
  hasApiKey: true,
  apiKeyHint: "sk-••••••••8F3A",
};

describe("settings state", () => {
  it("shows only the active model without repeating its provider", () => {
    expect(activeModelSummary(activeApi)).toBe("deepseek-v4-flash");
    expect(
      activeModelSummary({
        ...activeApi,
        api: { ...activeApi.api, model: "  " },
      }),
    ).toBe("未配置模型");
  });

  it("keeps editing state separate from the active backend", () => {
    const localDraft = { ...activeApi, backend: "local" as const };
    expect(isEditingActiveConfiguration(localDraft, activeApi)).toBe(false);
    expect(activationActionLabel("local")).toBe("切换到本地模型");
    expect(activationActionLabel("api")).toBe("切换到 API");
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
    ).toBe("glm");
    expect(
      providerPresetForConfig({
        ...activeApi,
        backend: "local",
        local: {
          baseUrl: "http://localhost:11434/v1/",
          model: "qwen3-14B",
        },
      }),
    ).toBe("ollama");
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
    const lmStudio = applyProviderPreset(localDraft, "lm-studio");
    expect(lmStudio.local).toEqual({
      baseUrl: "http://127.0.0.1:1234/v1",
      model: "qwen3-14B",
    });
    expect(lmStudio.api).toEqual(activeApi.api);

    const glm = applyProviderPreset(activeApi, "glm");
    expect(glm.api).toEqual({
      baseUrl: "https://open.bigmodel.cn/api/paas/v4",
      model: "glm-5.2",
    });
    expect(glm.local).toEqual(activeApi.local);
    expect(hasUnsavedProviderChanges(glm, activeApi, "")).toBe(true);
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
    ).toBe("连接超时");
  });
});
