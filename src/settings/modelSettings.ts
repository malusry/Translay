// Shared contracts for the settings UI and backend commands.
import type { TranslationMode } from "../shared/types";

export type { TranslationMode } from "../shared/types";

export type ModelBackend = "local" | "api";

export interface EndpointConfig {
  baseUrl: string;
  model: string;
}

export interface ModelConfigView {
  backend: ModelBackend;
  mode: TranslationMode;
  reasoningEnabled: boolean;
  local: EndpointConfig;
  api: EndpointConfig;
  timeoutSeconds: number;
  hasApiKey: boolean;
  apiKeyHint: string | null;
}

export interface ApiKeyStatus {
  hasApiKey: boolean;
  apiKeyHint: string | null;
}

export interface SaveModelConfigInput {
  backend: ModelBackend;
  mode: TranslationMode;
  reasoningEnabled: boolean;
  local: EndpointConfig;
  api: EndpointConfig;
  timeoutSeconds: number;
  apiKey: string | null;
}

export interface ConnectionTestResult {
  success: boolean;
  message: string;
  elapsedMs: number;
}
