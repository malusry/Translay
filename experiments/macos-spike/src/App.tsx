import { useCallback, useEffect, useRef, useState } from "react";
import { Settings } from "../../../src/settings/Settings";
import { previewSettings } from "./previewSettings";
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

export default function App() {
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

  return (
    <Settings
      windowChrome="native"
      initialSection="translation"
      readOnly={!native}
      ipc={native ? undefined : previewSettings}
      notice={
        <div className="mac-preview-notice" role="note">
          <span>{native ? "Mac 开发预览" : "浏览器布局预览"}</span>
          <span>{native ? "使用独立配置；划词取词暂不可用。" : "仅预览布局，不连接或保存配置。"}</span>
          {error && <span role="alert">{error}</span>}
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
    />
  );
}
