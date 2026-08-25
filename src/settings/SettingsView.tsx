import type { FormEvent, MouseEvent } from "react";

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
import translayWordmark from "../../assets/icon-concepts/translay-desktop-v3/translay-wordmark-flowline.svg";

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

function SaveStatusIcon({ saving }: { saving: boolean }) {
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true">
      {saving ? (
        <circle className="save-button-spinner" cx="10" cy="10" r="5.6" />
      ) : (
        <path d="m4.5 10.4 3.4 3.4 7.6-7.6" />
      )}
    </svg>
  );
}

function ConnectionIcon({ health }: { health: ConnectionHealth }) {
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true">
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
              <p data-tauri-drag-region>控制译文表达与模型推理</p>
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
              <div className="panel" aria-labelledby="translation-style-label">
                <div className="panel-heading">
                  <div>
                    <h2 id="translation-style-label">翻译风格</h2>
                    <p>选择更适合当前阅读场景的表达方式</p>
                  </div>
                </div>

                <div
                  className="style-options"
                  aria-label="翻译风格"
                  aria-busy={translationSaving}
                >
                  <button
                    type="button"
                    className={
                      config.mode === "conversational" ? "active" : ""
                    }
                    onClick={() => onSelectMode("conversational")}
                    disabled={!loaded || translationSaving}
                  >
                    <span className="choice-indicator" aria-hidden="true" />
                    <span>
                      <strong>口语</strong>
                      <small>自然流畅，保留原文语气</small>
                    </span>
                  </button>
                  <button
                    type="button"
                    className={config.mode === "academic" ? "active" : ""}
                    onClick={() => onSelectMode("academic")}
                    disabled={!loaded || translationSaving}
                  >
                    <span className="choice-indicator" aria-hidden="true" />
                    <span>
                      <strong>学术</strong>
                      <small>逻辑严谨，保持术语一致</small>
                    </span>
                  </button>
                </div>

                <div className="setting-line reasoning-row">
                  <div>
                    <strong>模型思考</strong>
                    <small>{reasoningDescription}</small>
                  </div>
                  <button
                    className={`switch-button ${
                      config.reasoningEnabled ? "active" : ""
                    }`}
                    type="button"
                    role="switch"
                    aria-checked={config.reasoningEnabled}
                    aria-label="模型思考"
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
                  {connectionStatus.kind !== "idle" && (
                    <span
                      className={`connection-feedback ${connectionStatus.kind}`}
                      role="status"
                      title={connectionStatus.message}
                    >
                      {connectionStatus.message}
                    </span>
                  )}
                  <button
                    className={`provider-test-button ${connectionStatus.kind}`}
                    type="button"
                    onClick={onTestConnection}
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
                    {(modelSaving || saveConfirmed) && (
                      <SaveStatusIcon saving={modelSaving} />
                    )}
                    <span>
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
