// Settings-window UI; system behavior remains in the Rust backend.
import {
  FormEvent,
  MouseEvent,
  useCallback,
  useEffect,
  useLayoutEffect,
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
  type ConnectionHealth,
  type LocalProviderPreset,
  type ProviderPortalPreset,
  type ProviderPreset,
  type TranslationPreferences,
} from "./settingsState";
import {
  activateModelBackend,
  clearModelApiKey,
  detectActiveLocalModel,
  getModelApiKeyStatus,
  getModelConfig,
  hideSettingsWindow,
  minimizeSettingsWindow,
  onModelBackendChanged,
  openProviderApiPortal,
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
import "../shared/uiFonts";
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
  localModels: {},
  apiModels: {},
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
  const statusFeedbackTimer = useRef<number | null>(null);
  const apiKeyStatusRequest = useRef(0);
  const localModelDetectionRequest = useRef(0);
  const connectionTestRequest = useRef(0);
  const connectionTestInFlight = useRef(false);
  const connectionViewMounted = useRef(false);
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

  const invalidateConnectionTest = useCallback(() => {
    connectionTestRequest.current += 1;
    connectionTestInFlight.current = false;
    setTestedDraft(null);
    setConnectionStatus({ kind: "idle", message: "" });
    setActiveHealth((current) => current === "working" ? "idle" : current);
  }, []);

  const testSignature = draftSignature({ ...config, apiKey: apiKey.trim() || null });
  const testApiKey = config.backend === "api" ? apiKey.trim() : "";
  const activeSignature = activeConfig
    ? draftSignature({ ...activeConfig, apiKey: null })
    : null;
  const connectionContext = useRef({ testSignature, testApiKey, activeSignature });

  useLayoutEffect(() => {
    connectionViewMounted.current = true;
    return () => {
      connectionViewMounted.current = false;
      connectionTestRequest.current += 1;
      connectionTestInFlight.current = false;
    };
  }, []);

  useLayoutEffect(() => {
    connectionContext.current = { testSignature, testApiKey, activeSignature };
    // Also covers input changes from existing asynchronous model detection/save.
    // Compare a new key in memory: draftSignature deliberately contains no secret.
    invalidateConnectionTest();
  }, [testSignature, testApiKey, activeSignature, invalidateConnectionTest]);

  const load = useCallback(async () => {
    invalidateConnectionTest();
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
  }, [invalidateConnectionTest]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let disposed = false;
    let stopListening: (() => void) | undefined;

    void onModelBackendChanged((payload) => {
      invalidateConnectionTest();
      setActiveConfig(payload);
      setConfig((current) => ({
        ...current,
        backend: payload.backend,
        mode: payload.mode,
        reasoningEnabled: payload.reasoningEnabled,
        localModels: payload.localModels,
        apiModels: payload.apiModels,
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
  }, [load, invalidateConnectionTest]);

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
      if (statusFeedbackTimer.current !== null) {
        window.clearTimeout(statusFeedbackTimer.current);
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

  const setBackend = async (backend: ModelBackend) => {
    if (
      !loaded ||
      activeConfig === null ||
      backend === config.backend ||
      modelSaving ||
      modelSwitching ||
      translationSaving
    ) {
      return;
    }
    localModelDetectionRequest.current += 1;
    invalidateConnectionTest();
    const discardingUnsavedChanges = hasUnsavedProviderChanges(
      config,
      activeConfig,
      apiKey,
    );

    setModelSwitching(true);
    setStatus({ kind: "working", message: "正在切换模型来源…" });
    try {
      const next = await activateModelBackend(backend);
      setActiveConfig(next);
      setConfig((current) => ({
        ...current,
        backend: next.backend,
        mode: next.mode,
        reasoningEnabled: next.reasoningEnabled,
        local: next.local,
        api: next.api,
        localModels: next.localModels,
        apiModels: next.apiModels,
        timeoutSeconds: next.timeoutSeconds,
        hasApiKey: next.hasApiKey,
        apiKeyHint: next.apiKeyHint,
      }));
      setActiveHealth("idle");
      setTestedDraft(null);
      setConnectionStatus({ kind: "idle", message: "" });
      setSaveConfirmed(false);
      const sourceName = backend === "local" ? "本地模型" : "API";
      const message = discardingUnsavedChanges
        ? `已切换至${sourceName}，未保存的修改未应用`
        : `已切换至${sourceName}`;
      setStatus({ kind: "success", message });
      if (statusFeedbackTimer.current !== null) {
        window.clearTimeout(statusFeedbackTimer.current);
      }
      statusFeedbackTimer.current = window.setTimeout(() => {
        setStatus((current) =>
          current.message === message
            ? { kind: "idle", message: "" }
            : current,
        );
        statusFeedbackTimer.current = null;
      }, 1800);
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    } finally {
      setModelSwitching(false);
    }
    setApiKey("");
    setEditingApiKey(false);
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
    invalidateConnectionTest();
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
    if (target === "local") {
      localModelDetectionRequest.current += 1;
    }
    const apiAddressChanged = target === "api" && field === "baseUrl";
    if (apiAddressChanged) {
      apiKeyStatusRequest.current += 1;
      setApiKey("");
      setEditingApiKey(false);
    }
    invalidateConnectionTest();
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
    const detectionRequest = ++localModelDetectionRequest.current;
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
    invalidateConnectionTest();
    setSaveConfirmed(false);
    setStatus({ kind: "idle", message: "" });

    if (next.backend === "local" && preset !== "custom") {
      const localPreset = preset as LocalProviderPreset;
      void detectActiveLocalModel(localPreset, next.local.baseUrl).then(
        (model) => {
          if (!model || detectionRequest !== localModelDetectionRequest.current) {
            return;
          }
          setConfig((current) => {
            if (
              current.backend !== "local" ||
              current.local.baseUrl.trim() !== next.local.baseUrl.trim()
            ) {
              return current;
            }
            return {
              ...current,
              local: { ...current.local, model },
              localModels: { ...current.localModels, [localPreset]: model },
            };
          });
        },
        () => {},
      );
    }
  };

  const openProviderPortal = async (preset: ProviderPortalPreset) => {
    const provider = [...apiProviderPresets, ...localProviderPresets].find(
      (candidate) => candidate.id === preset,
    )!;
    const isApiProvider = apiProviderPresets.some(
      (candidate) => candidate.id === preset,
    );
    const message = `已在默认浏览器打开 ${provider.label}${
      isApiProvider ? " API 平台" : " 配置文档"
    }`;
    try {
      await openProviderApiPortal(preset);
      setStatus({ kind: "success", message });
      if (statusFeedbackTimer.current !== null) {
        window.clearTimeout(statusFeedbackTimer.current);
      }
      statusFeedbackTimer.current = window.setTimeout(() => {
        setStatus((current) =>
          current.message === message
            ? { kind: "idle", message: "" }
            : current,
        );
        statusFeedbackTimer.current = null;
      }, 1800);
    } catch (error) {
      setStatus({ kind: "error", message: String(error) });
    }
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
    }, 1500);
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
    if (
      !loaded ||
      modelSaving ||
      modelSwitching ||
      translationSaving ||
      connectionTestInFlight.current
    ) {
      return;
    }
    const input = buildInput();
    const signature = draftSignature(input);
    const request = ++connectionTestRequest.current;
    connectionTestInFlight.current = true;
    const currentRequest = () =>
      connectionViewMounted.current &&
      request === connectionTestRequest.current &&
      signature === connectionContext.current.testSignature &&
      (input.backend !== "api" || (input.apiKey ?? "") === connectionContext.current.testApiKey);
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
      if (!currentRequest()) return;
      const health = result.success ? "success" : "error";
      setTestedDraft({ signature, health });
      if (testingActive && activeSignature === connectionContext.current.activeSignature) {
        setActiveHealth(health);
      }
      setConnectionStatus({
        kind: health,
        message: connectionFeedbackMessage(result),
      });
    } catch (error) {
      if (!currentRequest()) return;
      setTestedDraft({ signature, health: "error" });
      if (testingActive && activeSignature === connectionContext.current.activeSignature) {
        setActiveHealth("error");
      }
      setConnectionStatus({ kind: "error", message: connectionErrorMessage(error) });
    } finally {
      if (request === connectionTestRequest.current) {
        connectionTestInFlight.current = false;
      }
    }
  };

  const clearApiKey = async () => {
    invalidateConnectionTest();
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
    invalidateConnectionTest();
  };

  const finishApiKeyEdit = () => {
    if (config.hasApiKey && !apiKey.trim()) {
      setEditingApiKey(false);
    }
  };

  const setTimeoutSeconds = (timeoutSeconds: number) => {
    setConfig((current) => ({ ...current, timeoutSeconds }));
    invalidateConnectionTest();
  };

  const providerPreset = providerPresetForConfig(config);
  const apiKeyLabel = apiKeyLabelForPreset(providerPreset);
  const activeSummary = activeConfig
    ? activeModelSummary(activeConfig)
    : "模型配置加载中";
  const providerDirty = hasUnsavedProviderChanges(
    config,
    activeConfig,
    apiKey,
  );
  const reasoningDescription = reasoningDescriptionForConfig(
    activeConfig
      ? { ...activeConfig, reasoningEnabled: config.reasoningEnabled }
      : config,
  );

  return (
    <SettingsView
      section={section}
      config={config}
      activeHealth={activeHealth}
      activeSummary={activeSummary}
      reasoningDescription={reasoningDescription}
      loaded={loaded}
      translationSaving={translationSaving}
      modelSaving={modelSaving}
      modelSwitching={modelSwitching}
      saveConfirmed={saveConfirmed}
      providerDirty={providerDirty}
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
      onSelectBackend={(backend) => void setBackend(backend)}
      onSelectProviderPreset={setProviderPreset}
      onOpenProviderPortal={(preset) => void openProviderPortal(preset)}
      onSetEndpoint={setEndpoint}
      onRefreshApiKeyStatus={(baseUrl) => void refreshApiKeyStatus(baseUrl)}
      onBeginApiKeyEdit={beginApiKeyEdit}
      onSetApiKey={updateApiKey}
      onFinishApiKeyEdit={finishApiKeyEdit}
      onSetTimeoutSeconds={setTimeoutSeconds}
      onTestConnection={() => void handleTest()}
      onClearApiKey={() => void clearApiKey()}
    />
  );
}
