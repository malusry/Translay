// Settings-window UI; system behavior remains in the Rust backend.
import {
  FormEvent,
  MouseEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import type {
  ModelBackend,
  ModelConfigView,
  SaveModelConfigInput,
  TranslationMode,
} from "./modelSettings";
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
  type ConnectionHealth,
  type ProviderPreset,
  type TranslationPreferences,
} from "./settingsState";
import {
  activateModelBackend,
  clearModelApiKey,
  getModelApiKeyStatus,
  getModelConfig,
  hideSettingsWindow,
  minimizeSettingsWindow,
  onModelBackendChanged,
  saveModelProviderConfig,
  saveTranslationPreferences as saveTranslationPreferencesToBackend,
  startSettingsDragging,
  testModelConnection,
} from "./settingsIpc";
import {
  SettingsView,
  type SettingsSection,
  type SettingsStatus,
} from "./SettingsView";
import "./settings.css";

const initialConfig: ModelConfigView = {
  backend: "local",
  mode: "conversational",
  reasoningEnabled: false,
  local: {
    baseUrl: "http://127.0.0.1:11434/v1",
    model: "",
  },
  api: {
    baseUrl: "https://api.openai.com/v1",
    model: "",
  },
  timeoutSeconds: 60,
  hasApiKey: false,
  apiKeyHint: null,
};

export function Settings() {
  const [section, setSection] = useState<SettingsSection>("model");
  const [config, setConfig] = useState(initialConfig);
  const [activeConfig, setActiveConfig] = useState<ModelConfigView | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [editingApiKey, setEditingApiKey] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [translationSaving, setTranslationSaving] = useState(false);
  const [modelSaving, setModelSaving] = useState(false);
  const [modelSwitching, setModelSwitching] = useState(false);
  const [saveConfirmed, setSaveConfirmed] = useState(false);
  const translationSaveInFlight = useRef(false);
  const saveFeedbackTimer = useRef<number | null>(null);
  const apiKeyStatusRequest = useRef(0);
  const [activeHealth, setActiveHealth] =
    useState<ConnectionHealth>("idle");
  const [testedDraft, setTestedDraft] = useState<{
    signature: string;
    health: "success" | "error";
  } | null>(null);
  const [status, setStatus] = useState<SettingsStatus>({
    kind: "idle",
    message: "",
  });
  const [connectionStatus, setConnectionStatus] = useState<SettingsStatus>({
    kind: "idle",
    message: "",
  });

  const load = useCallback(async () => {
    try {
      const next = await getModelConfig();
      setConfig(next);
      setActiveConfig(next);
      setActiveHealth("idle");
      setTestedDraft(null);
      setConnectionStatus({ kind: "idle", message: "" });
      setEditingApiKey(false);
      setLoaded(true);
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let disposed = false;
    let stopListening: (() => void) | undefined;

    void onModelBackendChanged((payload) => {
      setActiveConfig(payload);
      setConfig((current) => ({
        ...current,
        backend: payload.backend,
        hasApiKey: payload.hasApiKey,
        apiKeyHint: payload.apiKeyHint,
      }));
      setActiveHealth("idle");
      setTestedDraft(null);
      setStatus({ kind: "idle", message: "" });
    })
      .then((unlisten) => {
        if (disposed) {
          unlisten();
        } else {
          stopListening = unlisten;
        }
      })
      .catch(() => {
        if (!disposed) {
          void load();
        }
      });

    return () => {
      disposed = true;
      stopListening?.();
    };
  }, [load]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setApiKey("");
        setEditingApiKey(false);
        void hideSettingsWindow();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  useEffect(
    () => () => {
      if (saveFeedbackTimer.current !== null) {
        window.clearTimeout(saveFeedbackTimer.current);
      }
    },
    [],
  );

  const refreshApiKeyStatus = useCallback(async (baseUrl: string) => {
    const request = ++apiKeyStatusRequest.current;
    try {
      const next = await getModelApiKeyStatus(baseUrl);
      if (request !== apiKeyStatusRequest.current) {
        return;
      }
      setConfig((current) =>
        current.api.baseUrl.trim() === baseUrl.trim()
          ? {
              ...current,
              hasApiKey: next.hasApiKey,
              apiKeyHint: next.apiKeyHint,
            }
          : current,
      );
    } catch (error) {
      if (request !== apiKeyStatusRequest.current) {
        return;
      }
      setConfig((current) =>
        current.api.baseUrl.trim() === baseUrl.trim()
          ? { ...current, hasApiKey: false, apiKeyHint: null }
          : current,
      );
      setStatus({ kind: "error", message: String(error) });
    }
  }, []);

  const setBackend = (backend: ModelBackend) => {
    setConfig((current) => ({ ...current, backend }));
    setApiKey("");
    setEditingApiKey(false);
    setTestedDraft(null);
    setConnectionStatus({ kind: "idle", message: "" });
    setSaveConfirmed(false);
    setStatus({ kind: "idle", message: "" });
  };

  const saveTranslationPreferences = async (
    preferences: TranslationPreferences,
  ) => {
    if (
      !loaded ||
      activeConfig === null ||
      translationSaveInFlight.current ||
      modelSaving ||
      modelSwitching ||
      (config.mode === preferences.mode &&
        config.reasoningEnabled === preferences.reasoningEnabled)
    ) {
      return;
    }

    const previous: TranslationPreferences = {
      mode: config.mode,
      reasoningEnabled: config.reasoningEnabled,
    };
    translationSaveInFlight.current = true;
    setTranslationSaving(true);
    setConfig((current) =>
      applyTranslationPreferences(current, preferences),
    );
    setTestedDraft(null);
    setConnectionStatus({ kind: "idle", message: "" });
    setStatus({ kind: "idle", message: "" });

    try {
      await saveTranslationPreferencesToBackend(
        preferences.mode,
        preferences.reasoningEnabled,
      );
      setActiveConfig((current) =>
        current ? applyTranslationPreferences(current, preferences) : current,
      );
      setTestedDraft(null);
      setStatus({ kind: "idle", message: "" });
    } catch (error) {
      setConfig((current) => applyTranslationPreferences(current, previous));
      setStatus({ kind: "error", message: String(error) });
    } finally {
      translationSaveInFlight.current = false;
      setTranslationSaving(false);
    }
  };

  const setMode = (mode: TranslationMode) => {
    void saveTranslationPreferences({
      mode,
      reasoningEnabled: config.reasoningEnabled,
    });
  };

  const setEndpoint = (
    target: ModelBackend,
    field: "baseUrl" | "model",
    value: string,
  ) => {
    const apiAddressChanged = target === "api" && field === "baseUrl";
    if (apiAddressChanged) {
      apiKeyStatusRequest.current += 1;
      setApiKey("");
      setEditingApiKey(false);
    }
    setTestedDraft(null);
    setConnectionStatus({ kind: "idle", message: "" });
    setConfig((current) => ({
      ...current,
      ...(apiAddressChanged
        ? { hasApiKey: false, apiKeyHint: null }
        : undefined),
      [target]: {
        ...current[target],
        [field]: value,
      },
    }));
  };

  const setProviderPreset = (preset: ProviderPreset) => {
    const next = applyProviderPreset(config, preset);
    const apiAddressChanged =
      next.backend === "api" &&
      next.api.baseUrl.trim() !== config.api.baseUrl.trim();
    setConfig({
      ...next,
      ...(apiAddressChanged
        ? { hasApiKey: false, apiKeyHint: null }
        : undefined),
    });
    if (apiAddressChanged) {
      setApiKey("");
      setEditingApiKey(false);
      void refreshApiKeyStatus(next.api.baseUrl);
    }
    setTestedDraft(null);
    setConnectionStatus({ kind: "idle", message: "" });
    setSaveConfirmed(false);
    setStatus({ kind: "idle", message: "" });
  };

  const buildInput = (): SaveModelConfigInput => ({
    backend: config.backend,
    mode: config.mode,
    reasoningEnabled: config.reasoningEnabled,
    local: config.local,
    api: config.api,
    timeoutSeconds: config.timeoutSeconds,
    apiKey: apiKey.trim() || null,
  });

  const showSaveConfirmation = () => {
    if (saveFeedbackTimer.current !== null) {
      window.clearTimeout(saveFeedbackTimer.current);
    }
    setSaveConfirmed(true);
    saveFeedbackTimer.current = window.setTimeout(() => {
      setSaveConfirmed(false);
      saveFeedbackTimer.current = null;
    }, 900);
  };

  const handleSave = async (event: FormEvent) => {
    event.preventDefault();
    if (
      section !== "model" ||
      !loaded ||
      modelSaving ||
      modelSwitching ||
      translationSaving ||
      !hasUnsavedProviderChanges(config, activeConfig, apiKey)
    ) {
      return;
    }

    const provider = config.backend;
    const input = buildInput();
    const signature = draftSignature(input);
    setModelSaving(true);
    setStatus({ kind: "idle", message: "" });
    try {
      const saved = await saveModelProviderConfig(input);
      setConfig((current) =>
        applySavedProviderConfig(current, saved, provider),
      );
      setActiveConfig(saved);
      if (provider === saved.backend) {
        setActiveHealth(
          testedDraft?.signature === signature ? testedDraft.health : "idle",
        );
      }
      if (provider === "api") {
        setApiKey("");
        setEditingApiKey(false);
      }
      showSaveConfirmation();
      setStatus({ kind: "idle", message: "" });
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    } finally {
      setModelSaving(false);
    }
  };

  const handleTest = async () => {
    const input = buildInput();
    const signature = draftSignature(input);
    const testingActive =
      activeConfig !== null &&
      isEditingActiveConfiguration(config, activeConfig) &&
      !(config.backend === "api" && apiKey.trim());
    setStatus({ kind: "idle", message: "" });
    setConnectionStatus({ kind: "working", message: "正在连接…" });
    if (testingActive) {
      setActiveHealth("working");
    }
    try {
      const result = await testModelConnection(input);
      const health = result.success ? "success" : "error";
      setTestedDraft({ signature, health });
      if (testingActive) {
        setActiveHealth(health);
      }
      setConnectionStatus({
        kind: health,
        message: connectionFeedbackMessage(result),
      });
    } catch (error) {
      setTestedDraft({ signature, health: "error" });
      if (testingActive) {
        setActiveHealth("error");
      }
      setConnectionStatus({ kind: "error", message: String(error) });
    }
  };

  const clearApiKey = async () => {
    try {
      const baseUrl = config.api.baseUrl;
      const next = await clearModelApiKey(baseUrl);
      setConfig((current) => ({
        ...current,
        hasApiKey: next.hasApiKey,
        apiKeyHint: next.apiKeyHint,
      }));
      setActiveConfig((current) =>
        current?.api.baseUrl.trim() === baseUrl.trim()
          ? {
              ...current,
              hasApiKey: next.hasApiKey,
              apiKeyHint: next.apiKeyHint,
            }
          : current,
      );
      setActiveHealth("idle");
      setTestedDraft(null);
      setConnectionStatus({ kind: "idle", message: "" });
      setApiKey("");
      setEditingApiKey(false);
      setStatus({ kind: "success", message: "已移除当前服务的 API Key" });
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    }
  };

  const activateBackend = async () => {
    if (
      !loaded ||
      activeConfig === null ||
      config.backend === activeConfig.backend ||
      hasUnsavedProviderChanges(config, activeConfig, apiKey) ||
      modelSaving ||
      modelSwitching ||
      translationSaving
    ) {
      return;
    }

    const backend = config.backend;
    const signature = draftSignature(buildInput());
    setModelSwitching(true);
    setStatus({ kind: "working", message: "正在切换…" });
    try {
      const next = await activateModelBackend(backend);
      setActiveConfig(next);
      setConfig((current) => ({
        ...current,
        mode: next.mode,
        reasoningEnabled: next.reasoningEnabled,
        hasApiKey: next.hasApiKey,
        apiKeyHint: next.apiKeyHint,
      }));
      setActiveHealth(
        testedDraft?.signature === signature ? testedDraft.health : "idle",
      );
      setStatus({ kind: "idle", message: "" });
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    } finally {
      setModelSwitching(false);
    }
  };

  const close = () => {
    setApiKey("");
    setEditingApiKey(false);
    void hideSettingsWindow();
  };

  const minimize = () => {
    void minimizeSettingsWindow();
  };

  const startWindowDragging = (event: MouseEvent<HTMLElement>) => {
    if (
      event.button === 0 &&
      !(event.target as HTMLElement).closest("button")
    ) {
      void startSettingsDragging();
    }
  };

  const beginApiKeyEdit = () => {
    setApiKey("");
    setEditingApiKey(true);
  };

  const updateApiKey = (value: string) => {
    setApiKey(value);
    setTestedDraft(null);
    setConnectionStatus({ kind: "idle", message: "" });
  };

  const finishApiKeyEdit = () => {
    if (config.hasApiKey && !apiKey.trim()) {
      setEditingApiKey(false);
    }
  };

  const setTimeoutSeconds = (timeoutSeconds: number) => {
    setConfig((current) => ({ ...current, timeoutSeconds }));
    setTestedDraft(null);
    setConnectionStatus({ kind: "idle", message: "" });
  };

  const providerPreset = providerPresetForConfig(config);
  const apiKeyLabel =
    providerPreset === "deepseek"
      ? "DeepSeek Key"
      : providerPreset === "glm"
        ? "GLM Key"
        : "API Key";
  const activeSummary = activeConfig
    ? activeModelSummary(activeConfig)
    : "模型配置加载中";
  const providerDirty = hasUnsavedProviderChanges(
    config,
    activeConfig,
    apiKey,
  );
  const viewingActiveBackend =
    activeConfig !== null && config.backend === activeConfig.backend;
  const switchLabel = activationActionLabel(config.backend);

  return (
    <SettingsView
      section={section}
      config={config}
      activeHealth={activeHealth}
      activeSummary={activeSummary}
      loaded={loaded}
      translationSaving={translationSaving}
      modelSaving={modelSaving}
      modelSwitching={modelSwitching}
      saveConfirmed={saveConfirmed}
      providerDirty={providerDirty}
      viewingActiveBackend={viewingActiveBackend}
      switchLabel={switchLabel}
      providerPreset={providerPreset}
      apiKeyLabel={apiKeyLabel}
      apiKey={apiKey}
      editingApiKey={editingApiKey}
      connectionStatus={connectionStatus}
      status={status}
      onSelectSection={setSection}
      onWindowMouseDown={startWindowDragging}
      onMinimize={minimize}
      onClose={close}
      onSubmit={handleSave}
      onSelectMode={setMode}
      onToggleReasoning={() =>
        void saveTranslationPreferences({
          mode: config.mode,
          reasoningEnabled: !config.reasoningEnabled,
        })
      }
      onSelectBackend={setBackend}
      onSelectProviderPreset={setProviderPreset}
      onSetEndpoint={setEndpoint}
      onRefreshApiKeyStatus={(baseUrl) => void refreshApiKeyStatus(baseUrl)}
      onBeginApiKeyEdit={beginApiKeyEdit}
      onSetApiKey={updateApiKey}
      onFinishApiKeyEdit={finishApiKeyEdit}
      onSetTimeoutSeconds={setTimeoutSeconds}
      onTestConnection={() => void handleTest()}
      onClearApiKey={() => void clearApiKey()}
      onActivateBackend={() => void activateBackend()}
    />
  );
}
