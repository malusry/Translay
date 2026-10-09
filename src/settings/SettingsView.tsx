import { useLayoutEffect, useRef, type FormEvent, type MouseEvent } from "react";

import type {
  ModelBackend,
  ModelConfigView,
  TranslationMode,
} from "./modelSettings";
import type {
  ConnectionHealth,
  ProviderPortalPreset,
  ProviderPreset,
} from "./settingsState";
import {
  apiProviderPresets,
  localProviderPresets,
} from "./settingsState";
import translayWordmark from "../../assets/brand/folded-ribbon/wordmark.svg?no-inline";

export type SettingsSection = "translation" | "model";

export type SettingsStatus = {
  kind: ConnectionHealth;
  message: string;
};

interface SettingsViewProps {
  section: SettingsSection;
  config: ModelConfigView;
  activeHealth: ConnectionHealth;
  activeSummary: string;
  reasoningDescription: string;
  loaded: boolean;
  translationSaving: boolean;
  modelSaving: boolean;
  modelSwitching: boolean;
  saveConfirmed: boolean;
  providerDirty: boolean;
  apiKeyLabel: string;
  apiKey: string;
  editingApiKey: boolean;
  connectionStatus: SettingsStatus;
  status: SettingsStatus;
  onSelectSection: (section: SettingsSection) => void;
  onWindowMouseDown: (event: MouseEvent<HTMLElement>) => void;
  onMinimize: () => void;
  onClose: () => void;
  onSubmit: (event: FormEvent) => void;
  onSelectMode: (mode: TranslationMode) => void;
  onToggleReasoning: () => void;
  onSelectBackend: (backend: ModelBackend) => void;
  onSelectProviderPreset: (preset: ProviderPreset) => void;
  onOpenProviderPortal: (preset: ProviderPortalPreset) => void;
  onSetEndpoint: (
    backend: ModelBackend,
    field: "baseUrl" | "model",
    value: string,
  ) => void;
  onRefreshApiKeyStatus: (baseUrl: string) => void;
  onBeginApiKeyEdit: () => void;
  onSetApiKey: (apiKey: string) => void;
  onFinishApiKeyEdit: () => void;
  onSetTimeoutSeconds: (seconds: number) => void;
  onTestConnection: () => void;
  onClearApiKey: () => void;
}

function SaveButtonBorder() {
  return (
    <svg
      className="save-button-border"
      viewBox="0 0 74 28"
      preserveAspectRatio="none"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <linearGradient id="settings-save-border-gradient" x1="0" y1="0" x2="1" y2="1">
          <stop className="save-border-sage" offset="0" />
          <stop className="save-border-blend" offset="0.42" />
          <stop className="save-border-purple" offset="0.76" />
          <stop className="save-border-purple" offset="1" />
        </linearGradient>
      </defs>
      <rect
        x="0.5"
        y="0.5"
        width="73"
        height="27"
        rx="6.5"
        fill="none"
        stroke="url(#settings-save-border-gradient)"
        strokeWidth="1"
      />
    </svg>
  );
}

function SaveStatusIcon({ saving }: { saving: boolean }) {
  return (
    <svg className="save-status-icon" viewBox="0 0 20 20" aria-hidden="true">
      {saving ? (
        <circle className="save-button-spinner" cx="10" cy="10" r="5.6" />
      ) : (
        <path d="m4.5 10.4 3.4 3.4 7.6-7.6" />
      )}
    </svg>
  );
}

function ConnectionButtonBorder() {
  return (
    <svg className="connection-button-border" viewBox="0 0 32 32" aria-hidden="true" focusable="false">
      <defs>
        <linearGradient id="settings-connection-border-gradient" x1="0" y1="0" x2="1" y2="1">
          <stop className="connection-border-sage" offset="0" />
          <stop className="connection-border-blend" offset="0.42" />
          <stop className="connection-border-purple" offset="0.76" />
          <stop className="connection-border-purple" offset="1" />
        </linearGradient>
      </defs>
      <circle cx="16" cy="16" r="15.5" fill="none" stroke="url(#settings-connection-border-gradient)" strokeWidth="1" />
    </svg>
  );
}

function ConnectionIcon({ health }: { health: ConnectionHealth }) {
  return (
    <svg className="connection-status-icon" viewBox="0 0 20 20" aria-hidden="true">
      {health === "working" ? (
        <circle className="connection-test-spinner" cx="10" cy="10" r="5.6" />
      ) : health === "success" ? (
        <>
          <circle cx="10" cy="10" r="6.2" />
          <path d="m6.8 10.2 2.1 2.1 4.5-4.6" />
        </>
      ) : health === "error" ? (
        <>
          <circle cx="10" cy="10" r="6.2" />
          <path d="M10 6.7v4.2" />
          <path d="M10 13.7h.01" />
        </>
      ) : (
        <>
          <path d="m8.1 12.8-1.2 1.3a3.1 3.1 0 0 1-4.4-4.4l2.4-2.4a3.1 3.1 0 0 1 4.4 0" />
          <path d="m11.9 7.2 1.2-1.3a3.1 3.1 0 0 1 4.4 4.4l-2.4 2.4a3.1 3.1 0 0 1-4.4 0" />
          <path d="m7.4 12.6 5.2-5.2" />
        </>
      )}
    </svg>
  );
}

function ModeChoice({
  mode,
  label,
  description,
  active,
  disabled,
  onSelect,
}: {
  mode: TranslationMode;
  label: string;
  description: string;
  active: boolean;
  disabled: boolean;
  onSelect: (mode: TranslationMode) => void;
}) {
  const buttonRef = useRef<HTMLButtonElement>(null);

  useLayoutEffect(() => {
    const button = buttonRef.current;
    if (!button) return;
    const marker = button.querySelector<SVGSVGElement>(".choice-indicator")!;
    const wash = button.querySelector<HTMLElement>(".mode-choice-wash")!;
    const measure = () => {
      const card = button.getBoundingClientRect();
      const circle = marker.getBoundingClientRect();
      const style = getComputedStyle(button);
      const borderLeft = parseFloat(style.borderLeftWidth);
      const borderTop = parseFloat(style.borderTopWidth);
      const width = card.width - borderLeft - parseFloat(style.borderRightWidth);
      const height = card.height - borderTop - parseFloat(style.borderBottomWidth);
      const x = circle.left + circle.width / 2 - card.left - borderLeft;
      const y = circle.top + circle.height / 2 - card.top - borderTop;
      const distance = Math.hypot(Math.max(x, width - x), Math.max(y, height - y));
      wash.style.setProperty("--mode-wash-x", `${x}px`);
      wash.style.setProperty("--mode-wash-y", `${y}px`);
      // The feathered edge ends at 86%; keep the rounded card fully covered.
      wash.style.setProperty(
        "--mode-wash-size",
        `${Math.ceil((distance + 4) / 0.86) * 2}px`,
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(button);
    return () => observer.disconnect();
  }, []);

  return (
    <button
      ref={buttonRef}
      type="button"
      className={`${mode === "conversational" ? "daily-mode" : "study-mode"} ${active ? "active" : ""}`}
      onClick={() => onSelect(mode)}
      disabled={disabled}
      aria-pressed={active}
    >
      <span className="mode-choice-wash" aria-hidden="true" />
      <span className="mode-choice-copy">
        <span className="mode-choice-heading">
          <svg
            className="choice-indicator"
            viewBox="0 0 14 14"
            aria-hidden="true"
            focusable="false"
          >
            <circle className="choice-ring" cx="7" cy="7" r="6.5" />
            <circle className="choice-dot" cx="7" cy="7" r="3" />
          </svg>
          <strong>{label}</strong>
        </span>
        <small>{description}</small>
      </span>
    </button>
  );
}

export function SettingsView({
  section,
  config,
  activeHealth,
  activeSummary,
  reasoningDescription,
  loaded,
  translationSaving,
  modelSaving,
  modelSwitching,
  saveConfirmed,
  providerDirty,
  apiKeyLabel,
  apiKey,
  editingApiKey,
  connectionStatus,
  status,
  onSelectSection,
  onWindowMouseDown,
  onMinimize,
  onClose,
  onSubmit,
  onSelectMode,
  onToggleReasoning,
  onSelectBackend,
  onSelectProviderPreset,
  onOpenProviderPortal,
  onSetEndpoint,
  onRefreshApiKeyStatus,
  onBeginApiKeyEdit,
  onSetApiKey,
  onFinishApiKeyEdit,
  onSetTimeoutSeconds,
  onTestConnection,
  onClearApiKey,
}: SettingsViewProps) {
  const endpoint = config[config.backend];

  return (
    <main className="settings-shell">
      <aside
        className="settings-sidebar"
        data-tauri-drag-region
        onMouseDown={onWindowMouseDown}
      >
        <div className="sidebar-brand" data-tauri-drag-region>
          <img
            className="brand-wordmark"
            src={translayWordmark}
            alt="Translay"
            data-tauri-drag-region
          />
        </div>
        <nav className="settings-nav" aria-label="设置分类">
          <button
            type="button"
            className={`translation-nav ${
              section === "translation" ? "active" : ""
            }`}
            onClick={() => onSelectSection("translation")}
          >
            翻译
          </button>
          <button
            type="button"
            className={`model-nav ${section === "model" ? "active" : ""}`}
            onClick={() => onSelectSection("model")}
          >
            模型
          </button>
        </nav>
      </aside>

      <section className="settings-content">
        <header
          className="settings-header"
          data-tauri-drag-region
          onMouseDown={onWindowMouseDown}
        >
          <div className="settings-title" data-tauri-drag-region>
            <h1
              className={`settings-heading ${section}`}
              data-tauri-drag-region
            >
              {section === "translation" ? "翻译" : "模型"}
            </h1>
            {section === "translation" ? (
              <p data-tauri-drag-region>选择使用场景与模型思考方式</p>
            ) : (
              <p
                className={`active-model-status ${activeHealth}`}
                data-tauri-drag-region
                aria-label={`当前使用 ${activeSummary}`}
              >
                <span className="connection-dot" aria-hidden="true" />
                <span data-tauri-drag-region>{activeSummary}</span>
              </p>
            )}
          </div>
          <div className="window-controls">
            <button
              className="icon-button"
              type="button"
              onClick={onMinimize}
              aria-label="最小化"
              title="最小化"
            >
              <span className="minimize-mark" aria-hidden="true" />
            </button>
            <button
              className="icon-button"
              type="button"
              onClick={onClose}
              aria-label="关闭"
              title="关闭"
            >
              <span className="close-mark" aria-hidden="true" />
            </button>
          </div>
        </header>

        <form className="settings-form" onSubmit={onSubmit}>
          <div className="settings-body">
            {section === "translation" ? (
              <div className="panel" aria-labelledby="translation-mode-label">
                <div className="panel-heading">
                  <div>
                    <h2 id="translation-mode-label">使用模式</h2>
                    <p>选择适合当前内容的翻译方式</p>
                  </div>
                </div>

                <div
                  className="style-options"
                  aria-label="使用模式"
                  aria-busy={translationSaving}
                >
                  <ModeChoice
                    mode="conversational"
                    label="日常"
                    description="网页、消息与常用词句的快速翻译"
                    active={config.mode === "conversational"}
                    disabled={!loaded || translationSaving}
                    onSelect={onSelectMode}
                  />
                  <ModeChoice
                    mode="academic"
                    label="学习"
                    description="结合上下文理解论文、教材与专业内容"
                    active={config.mode === "academic"}
                    disabled={!loaded || translationSaving}
                    onSelect={onSelectMode}
                  />
                </div>

                <div className="setting-line reasoning-row">
                  <div>
                    <strong>翻译思考</strong>
                    <small>{reasoningDescription}；详细解释会单独优先使用思考模式。</small>
                  </div>
                  <button
                    className={`switch-button ${
                      config.reasoningEnabled ? "active" : ""
                    }`}
                    type="button"
                    role="switch"
                    aria-checked={config.reasoningEnabled}
                    aria-label="翻译思考"
                    disabled={!loaded || translationSaving}
                    onClick={onToggleReasoning}
                  >
                    <span />
                  </button>
                </div>
              </div>
            ) : (
              <div className="panel" aria-labelledby="model-source-label">
                <div className="model-toolbar">
                  <div>
                    <h2 id="model-source-label">模型来源</h2>
                    <p>选择后立即切换当前模型来源</p>
                  </div>
                  <div
                    className={`source-tabs ${config.backend} ${
                      modelSwitching ? "switching" : ""
                    }`}
                    aria-label={`当前正在使用${
                      config.backend === "local" ? "本地模型" : "在线 API"
                    }`}
                    aria-busy={modelSwitching}
                  >
                    <button
                      type="button"
                      className={config.backend === "local" ? "active" : ""}
                      aria-pressed={config.backend === "local"}
                      aria-label={
                        config.backend === "local"
                          ? "当前正在使用本地模型"
                          : "切换到本地模型"
                      }
                      title={
                        providerDirty && config.backend !== "local"
                          ? "切换到本地模型；当前未保存的修改不会应用"
                          : undefined
                      }
                      disabled={
                        !loaded ||
                        modelSaving ||
                        modelSwitching ||
                        translationSaving
                      }
                      onClick={() => onSelectBackend("local")}
                    >
                      <span className="source-mode-dot" aria-hidden="true" />
                      本地
                    </button>
                    <button
                      type="button"
                      className={config.backend === "api" ? "active" : ""}
                      aria-pressed={config.backend === "api"}
                      aria-label={
                        config.backend === "api"
                          ? "当前正在使用在线 API"
                          : "切换到在线 API"
                      }
                      title={
                        providerDirty && config.backend !== "api"
                          ? "切换到在线 API；当前未保存的修改不会应用"
                          : undefined
                      }
                      disabled={
                        !loaded ||
                        modelSaving ||
                        modelSwitching ||
                        translationSaving
                      }
                      onClick={() => onSelectBackend("api")}
                    >
                      <span className="source-mode-dot" aria-hidden="true" />
                      API
                    </button>
                  </div>
                </div>

                <div className="form-rows">
                  <div className="form-row provider-preset-row">
                    <span className="row-label">快速配置</span>
                    <div className="provider-preset-content">
                      <div
                        className="provider-preset-list"
                        aria-label="快速切换模型服务配置"
                      >
                        {(config.backend === "api"
                          ? apiProviderPresets
                          : localProviderPresets
                        ).map((preset) => {
                          const portalName =
                            config.backend === "api"
                              ? "官方 API 平台"
                              : "官方配置文档";
                          return (
                            <button
                              key={preset.id}
                              className={`provider-preset provider-${preset.id}`}
                              type="button"
                              onClick={() =>
                                onSelectProviderPreset(preset.id)
                              }
                              onContextMenu={(event) => {
                                event.preventDefault();
                                onOpenProviderPortal(preset.id);
                              }}
                              aria-label={`${preset.label}：左键应用预设，右键打开${portalName}`}
                              title={`左键应用 ${preset.label} 预设 · 右键打开${portalName}`}
                            >
                              {preset.label}
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  </div>

                  <div className="form-row">
                    <label htmlFor="base-url">服务地址</label>
                    <input
                      id="base-url"
                      value={endpoint.baseUrl}
                      onChange={(event) =>
                        onSetEndpoint(
                          config.backend,
                          "baseUrl",
                          event.target.value,
                        )
                      }
                      onBlur={(event) => {
                        if (config.backend === "api") {
                          onRefreshApiKeyStatus(event.currentTarget.value);
                        }
                      }}
                      placeholder="https://…/v1"
                      spellCheck={false}
                    />
                  </div>

                  <div className="form-row">
                    <label htmlFor="model-name">模型名称</label>
                    <input
                      id="model-name"
                      value={endpoint.model}
                      onChange={(event) =>
                        onSetEndpoint(
                          config.backend,
                          "model",
                          event.target.value,
                        )
                      }
                      placeholder={
                        config.backend === "local"
                          ? "例如 qwen3:8b"
                          : "例如模型服务提供的 model id"
                      }
                      spellCheck={false}
                    />
                  </div>

                  {config.backend === "api" && (
                    <div className="form-row">
                      <span className="row-label">{apiKeyLabel}</span>
                      {config.hasApiKey && !editingApiKey ? (
                        <button
                          className="credential-slot"
                          type="button"
                          aria-label={`${
                            config.apiKeyHint ?? `已保存 ${apiKeyLabel}`
                          }，点击修改`}
                          onClick={onBeginApiKeyEdit}
                        >
                          <span className="credential-mask">
                            {config.apiKeyHint ?? "••••••••"}
                          </span>
                          <span className="credential-prompt">
                            点击修改 {apiKeyLabel}
                          </span>
                        </button>
                      ) : (
                        <input
                          id="api-key"
                          type="password"
                          value={apiKey}
                          autoFocus={editingApiKey}
                          onChange={(event) => onSetApiKey(event.target.value)}
                          onBlur={onFinishApiKeyEdit}
                          placeholder={
                            config.hasApiKey
                              ? `输入新的 ${apiKeyLabel}`
                              : `输入 ${apiKeyLabel}`
                          }
                          aria-label={apiKeyLabel}
                          autoComplete="off"
                        />
                      )}
                    </div>
                  )}

                  <div className="form-row timeout-row">
                    <label htmlFor="timeout-seconds">超时时间</label>
                    <div className="number-control">
                      <input
                        id="timeout-seconds"
                        type="number"
                        min={5}
                        max={180}
                        value={config.timeoutSeconds}
                        onChange={(event) =>
                          onSetTimeoutSeconds(Number(event.target.value))
                        }
                      />
                      <span>秒</span>
                    </div>
                  </div>
                </div>

                <div className="provider-actions">
                  <span
                    id="connection-feedback"
                    className={`connection-feedback ${connectionStatus.kind}`}
                    role="status"
                    aria-live="polite"
                    aria-atomic="true"
                    title={connectionStatus.message}
                  >
                    {connectionStatus.message}
                  </span>
                  <button
                    className={`provider-test-button ${connectionStatus.kind}`}
                    type="button"
                    onClick={onTestConnection}
                    aria-busy={connectionStatus.kind === "working"}
                    aria-describedby={connectionStatus.message ? "connection-feedback" : undefined}
                    disabled={
                      !loaded ||
                      connectionStatus.kind === "working" ||
                      modelSaving ||
                      modelSwitching ||
                      translationSaving
                    }
                    aria-label={
                      connectionStatus.kind === "working"
                        ? "正在测试连接"
                        : connectionStatus.kind === "idle"
                          ? "测试连接"
                          : "重新测试连接"
                    }
                    title={
                      connectionStatus.kind === "working"
                        ? "正在测试连接"
                        : connectionStatus.kind === "idle"
                          ? "测试连接"
                          : "重新测试连接"
                    }
                  >
                    <ConnectionButtonBorder />
                    <ConnectionIcon health={connectionStatus.kind} />
                  </button>
                  <button
                    className={`provider-save-button ${
                      modelSaving
                        ? "saving"
                        : saveConfirmed
                          ? "confirmed"
                          : providerDirty
                            ? "dirty"
                            : "idle"
                    }`}
                    type="submit"
                    aria-busy={modelSaving}
                    disabled={
                      !loaded ||
                      !providerDirty ||
                      modelSaving ||
                      modelSwitching ||
                      translationSaving
                    }
                    aria-label={
                      modelSaving
                        ? "正在保存当前配置"
                        : saveConfirmed
                          ? "当前配置已保存"
                          : "保存当前配置"
                    }
                  >
                    <SaveButtonBorder />
                    {(modelSaving || saveConfirmed) && (
                      <SaveStatusIcon saving={modelSaving} />
                    )}
                    <span className="save-button-label">
                      {modelSaving ? "保存中" : saveConfirmed ? "已保存" : "保存"}
                    </span>
                  </button>
                </div>
              </div>
            )}
          </div>

          {(status.kind !== "idle" ||
            (section === "model" &&
              config.backend === "api" &&
              config.hasApiKey &&
              editingApiKey)) && (
            <footer className="settings-footer">
              <div className={`settings-status ${status.kind}`} role="status">
                <span>{status.message}</span>
                {section === "model" &&
                  config.backend === "api" &&
                  config.hasApiKey &&
                  editingApiKey && (
                    <button type="button" onClick={onClearApiKey}>
                      移除 {apiKeyLabel}
                    </button>
                  )}
              </div>
            </footer>
          )}
        </form>
      </section>
    </main>
  );
}
