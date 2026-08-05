import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ApiKeyStatus,
  ConnectionTestResult,
  ModelBackend,
  ModelConfigView,
  SaveModelConfigInput,
  TranslationMode,
} from "./modelSettings";

const commands = {
  activateModelBackend: "activate_model_backend",
  clearModelApiKey: "clear_model_api_key",
  getModelApiKeyStatus: "get_model_api_key_status",
  getModelConfig: "get_model_config",
  hideSettingsWindow: "hide_settings_window",
  minimizeSettingsWindow: "minimize_settings_window",
  saveModelProviderConfig: "save_model_provider_config",
  saveTranslationPreferences: "save_translation_preferences",
  startSettingsDragging: "start_settings_dragging",
  testModelConnection: "test_model_connection",
} as const;

const events = {
  modelBackendChanged: "model-backend-changed",
} as const;

export function getModelConfig(): Promise<ModelConfigView> {
  return invoke<ModelConfigView>(commands.getModelConfig);
}

export function getModelApiKeyStatus(
  baseUrl: string,
): Promise<ApiKeyStatus> {
  return invoke<ApiKeyStatus>(commands.getModelApiKeyStatus, { baseUrl });
}

export function saveTranslationPreferences(
  mode: TranslationMode,
  reasoningEnabled: boolean,
): Promise<void> {
  return invoke(commands.saveTranslationPreferences, {
    mode,
    reasoningEnabled,
  });
}

export function saveModelProviderConfig(
  input: SaveModelConfigInput,
): Promise<ModelConfigView> {
  return invoke<ModelConfigView>(commands.saveModelProviderConfig, { input });
}

export function testModelConnection(
  input: SaveModelConfigInput,
): Promise<ConnectionTestResult> {
  return invoke<ConnectionTestResult>(commands.testModelConnection, { input });
}

export function clearModelApiKey(baseUrl: string): Promise<ApiKeyStatus> {
  return invoke<ApiKeyStatus>(commands.clearModelApiKey, { baseUrl });
}

export function activateModelBackend(
  backend: ModelBackend,
): Promise<ModelConfigView> {
  return invoke<ModelConfigView>(commands.activateModelBackend, { backend });
}

export function hideSettingsWindow(): Promise<void> {
  return invoke(commands.hideSettingsWindow);
}

export function minimizeSettingsWindow(): Promise<void> {
  return invoke(commands.minimizeSettingsWindow);
}

export function startSettingsDragging(): Promise<void> {
  return invoke(commands.startSettingsDragging);
}

export function onModelBackendChanged(
  handler: (config: ModelConfigView) => void,
): Promise<UnlistenFn> {
  return listen<ModelConfigView>(
    events.modelBackendChanged,
    ({ payload }) => handler(payload),
  );
}
