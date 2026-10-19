import type * as settingsIpc from "../../../src/settings/settingsIpc";
import type { ModelConfigView } from "../../../src/settings/modelSettings";

let config: ModelConfigView = {
  backend: "local", mode: "conversational", reasoningEnabled: false,
  local: { baseUrl: "http://127.0.0.1:11434/v1", model: "" },
  api: { baseUrl: "https://api.openai.com/v1", model: "" },
  localModels: {}, apiModels: {}, timeoutSeconds: 60, hasApiKey: false, apiKeyHint: null,
};
const unavailable = async (): Promise<never> => { throw new Error("浏览器仅预览布局，请在 Mac 原生程序中配置模型"); };

// Browser previews exercise the same controller without invoking native APIs.
// Form edits and connection/save actions remain disabled by Settings.readOnly.
export const previewSettings: typeof settingsIpc = {
  getModelConfig: async () => structuredClone(config),
  getModelApiKeyStatus: async () => ({ hasApiKey: false, apiKeyHint: null }),
  activateModelBackend: async (backend) => {
    config = { ...config, backend };
    return structuredClone(config);
  },
  detectActiveLocalModel: async () => null,
  onModelBackendChanged: async () => () => {},
  saveTranslationPreferences: unavailable,
  saveModelProviderConfig: unavailable,
  testModelConnection: unavailable,
  clearModelApiKey: unavailable,
  openProviderApiPortal: unavailable,
  startSettingsDragging: unavailable,
  hideSettingsWindow: unavailable,
  minimizeSettingsWindow: unavailable,
};
