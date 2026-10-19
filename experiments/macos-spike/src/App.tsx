import { useCallback, useEffect, useRef, useState } from "react";
import wordmark from "../../../assets/brand/folded-ribbon/wordmark.svg?no-inline";
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
    <main className="spike-shell">
      <aside className="spike-sidebar">
        <img className="spike-wordmark" src={wordmark} alt="Translay" />
        <nav aria-label="设置分类"><a href="#translation" aria-current="page">翻译</a></nav>
        <span className="spike-build-label">Mac 开发预览</span>
      </aside>
      <section className="spike-content">
        <header><h1>翻译</h1><p>使用场景与系统权限</p></header>
        {!native && <p className="preview-note" role="note">浏览器布局预览 · 菜单栏和系统权限需在 Mac 中验证</p>}
        {error && <p className="error-note" role="alert">{error}</p>}
        {snapshot?.exiting && <p className="preview-note" role="status">正在退出…</p>}
        <div className="spike-body" id="translation">
          <section className="settings-card" aria-labelledby="mode-heading">
            <div className="card-heading"><h2 id="mode-heading">翻译模式</h2><span>从右键菜单切换</span></div>
            <div className="mode-grid">
              <div className={`mode-tile ${snapshot?.mode === "conversational" ? "selected" : ""}`}>
                <span className="mode-name">日常模式</span><span>自然、直接地理解文字</span>
                {snapshot?.mode === "conversational" && <span className="mode-check" aria-label="当前模式">✓</span>}
              </div>
              <div className={`mode-tile academic ${snapshot?.mode === "academic" ? "selected" : ""}`}>
                <span className="mode-name">学习模式</span><span>关注术语与原文表达</span>
                {snapshot?.mode === "academic" && <span className="mode-check" aria-label="当前模式">✓</span>}
              </div>
            </div>
            <div className="selection-status" aria-live="polite">
              <span>划词图标</span>
              <span className={`state-pill ${snapshot?.selectionIconEnabled ? "enabled" : ""}`}>
                {!snapshot ? "读取中" : snapshot.selectionIconEnabled ? "已开启" : "已关闭"}
              </span>
            </div>
            <p className="card-help">单击菜单栏图标切换开关，右键选择翻译模式。</p>
          </section>
          <section className="settings-card" aria-labelledby="permission-heading">
            <div className="card-heading"><h2 id="permission-heading">辅助功能权限</h2><span className={accessibility?.trusted ? "trusted" : ""} aria-live="polite">{permissionLabel}</span></div>
            <p className="permission-description">读取其他应用中的选中文字，需要你在系统设置中允许 Translay。</p>
            <p className="card-help">系统设置 → 隐私与安全性 → 辅助功能。授权后返回此窗口重新检查。</p>
            <div className="permission-actions">
              <button type="button" className="primary-button" disabled={!native || busy || accessibility?.trusted === true} onClick={() => void authorize()}>{busy ? "正在请求…" : accessibility?.trusted ? "已获授权" : "请求授权"}</button>
              <button type="button" disabled={!native} onClick={() => void openSettings()}>打开系统设置</button>
              <button type="button" disabled={!native || busy} onClick={() => void refreshAccessibility()}>重新检查</button>
            </div>
          </section>
        </div>
        <footer>此预览仅验证菜单栏、窗口和授权状态；划词开关暂不启动自动取词。</footer>
      </section>
    </main>
  );
}
