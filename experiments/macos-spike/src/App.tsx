import { useCallback, useEffect, useRef, useState } from "react";
import { SettingsView, type SettingsSection } from "../../../src/settings/SettingsView";
import type { ModelBackend, ModelConfigView } from "../../../src/settings/modelSettings";
import { apiKeyLabelForPreset, providerPresetForConfig, reasoningDescriptionForConfig } from "../../../src/settings/settingsState";
import {
  acceptSnapshot,
  getAccessibility,
  getSnapshot,
  native,
  onNativeError,
  onSnapshot,
  openSystemSettings,
  requestAccessibility,
  type AccessibilityStatus,
  type Snapshot,
} from "./ipc";

const idle = { kind: "idle", message: "" } as const;
const unavailable = () => {};

export default function App() {
  const [section, setSection] = useState<SettingsSection>("translation");
  const [backend, setBackend] = useState<ModelBackend>("local");
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [accessibility, setAccessibility] = useState<AccessibilityStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const requestGeneration = useRef(0);
  const mounted = useRef(false);

  const refreshAccessibility = useCallback(async () => {
    const generation = ++requestGeneration.current;
    try {
      const status = await getAccessibility();
      if (mounted.current && generation === requestGeneration.current) {
        setAccessibility(status);
      }
    } catch (reason) {
      if (mounted.current && generation === requestGeneration.current) setError(String(reason));
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    let disposed = false;
    const unsubscribers: Array<() => void> = [];
    const accept = (next: Snapshot) => {
      if (!disposed) setSnapshot((current) => acceptSnapshot(current, next));
    };
    const subscribe = async () => {
      const subscriptions = await Promise.allSettled([
        onSnapshot(accept),
        onNativeError((message) => { if (!disposed) setError(message); }),
      ]);
      for (const subscription of subscriptions) {
        if (subscription.status === "fulfilled") {
          if (disposed) subscription.value();
          else unsubscribers.push(subscription.value);
        } else if (!disposed) {
          setError(String(subscription.reason));
        }
      }
      if (disposed) return;
      accept(await getSnapshot());
      await refreshAccessibility();
    };
    void subscribe().catch((reason) => { if (!disposed) setError(String(reason)); });
    window.addEventListener("focus", refreshAccessibility);
    return () => {
      disposed = true;
      mounted.current = false;
      ++requestGeneration.current;
      unsubscribers.forEach((unsubscribe) => unsubscribe());
      window.removeEventListener("focus", refreshAccessibility);
    };
  }, [refreshAccessibility]);

  async function authorize() {
    setBusy(true);
    setError("");
    try {
      // Native prompting is asynchronous; always refresh from the system query.
      await requestAccessibility();
      await refreshAccessibility();
    } catch (reason) {
      if (mounted.current) setError(String(reason));
    } finally {
      if (mounted.current) setBusy(false);
    }
  }

  async function openSettings() {
    setError("");
    try { await openSystemSettings(); }
    catch (reason) { if (mounted.current) setError(String(reason)); }
  }

  const permissionLabel = !accessibility
    ? "检查中"
    : !accessibility.supported
      ? "需要 Mac 验证"
      : accessibility.trusted ? "已授权" : "未授权";

  // Display the real shared view without connecting prototype fields to model
  // services or credentials. Backend tabs only select which form to preview.
  const config: ModelConfigView = {
    backend,
    mode: snapshot?.mode ?? "conversational",
    reasoningEnabled: false,
    local: { baseUrl: "http://127.0.0.1:11434/v1", model: "" },
    api: { baseUrl: "https://api.openai.com/v1", model: "" },
    localModels: {},
    apiModels: {},
    timeoutSeconds: 60,
    hasApiKey: false,
    apiKeyHint: null,
  };

  return (
    <SettingsView
      windowChrome="native"
      readOnly
      section={section}
      config={config}
      activeHealth="idle"
      activeSummary="配置样式预览 · 尚未连接模型"
      reasoningDescription={reasoningDescriptionForConfig(config)}
      loaded={snapshot !== null && !snapshot.exiting}
      translationSaving={false}
      modelSaving={false}
      modelSwitching={false}
      saveConfirmed={false}
      providerDirty={false}
      apiKeyLabel={apiKeyLabelForPreset(providerPresetForConfig(config))}
      apiKey=""
      editingApiKey={false}
      connectionStatus={idle}
      status={error ? { kind: "error", message: error } : idle}
      modelSourceDescription="切换查看配置样式；模型连接与保存尚未接入"
      notice={
        <div className="mac-preview-notice" role="note">
          <span>{native ? "Mac 开发预览" : "浏览器布局预览"}</span>
          <span>配置项仅供预览，尚未接入模型与保存。</span>
          {snapshot?.exiting && <span role="status">正在退出…</span>}
        </div>
      }
      translationExtras={
        <section className="panel mac-system-panel" aria-labelledby="permission-heading">
          <div className="setting-line mac-selection-status" aria-live="polite">
            <div><strong>划词图标</strong><small>单击菜单栏图标切换开关，右键切换日常／学习模式。</small></div>
            <span className={`mac-state-pill ${snapshot?.selectionIconEnabled ? "enabled" : ""}`}>
              {!snapshot ? "读取中" : snapshot.selectionIconEnabled ? "已开启" : "已关闭"}
            </span>
          </div>
          <div className="panel-heading mac-permission-heading">
            <div><h2 id="permission-heading">辅助功能权限</h2><p>用于读取其他应用中的选中文字</p></div>
            <span className={`mac-permission-status ${accessibility?.trusted ? "trusted" : ""}`} aria-live="polite">{permissionLabel}</span>
          </div>
          <p className="mac-permission-help">在系统设置的「隐私与安全性 → 辅助功能」中允许 Translay，返回此窗口后重新检查。</p>
          <div className="mac-permission-actions">
            <button type="button" disabled={!native || busy || accessibility?.trusted === true} onClick={() => void authorize()}>{busy ? "正在请求…" : accessibility?.trusted ? "已获授权" : "请求授权"}</button>
            <button type="button" disabled={!native} onClick={() => void openSettings()}>打开系统设置</button>
            <button type="button" disabled={!native || busy} onClick={() => void refreshAccessibility()}>重新检查</button>
          </div>
          <p className="mac-scope-note">划词开关暂不启动自动取词。菜单栏、窗口与权限需在 Mac 中验证。</p>
        </section>
      }
      onSelectSection={setSection}
      onWindowMouseDown={unavailable}
      onMinimize={unavailable}
      onClose={unavailable}
      onSubmit={(event) => event.preventDefault()}
      onSelectMode={unavailable}
      onToggleReasoning={unavailable}
      onSelectBackend={setBackend}
      onSelectProviderPreset={unavailable}
      onOpenProviderPortal={unavailable}
      onSetEndpoint={unavailable}
      onRefreshApiKeyStatus={unavailable}
      onBeginApiKeyEdit={unavailable}
      onSetApiKey={unavailable}
      onFinishApiKeyEdit={unavailable}
      onSetTimeoutSeconds={unavailable}
      onTestConnection={unavailable}
      onClearApiKey={unavailable}
    />
  );
}
