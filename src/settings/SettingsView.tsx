import type { FormEvent, MouseEvent } from "react";

import type {
  ModelBackend,
  ModelConfigView,
  TranslationMode,
} from "./modelSettings";
import type {
  ConnectionHealth,
  ProviderPreset,
} from "./settingsState";

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
  loaded: boolean;
  translationSaving: boolean;
  modelSaving: boolean;
  modelSwitching: boolean;
  saveConfirmed: boolean;
  providerDirty: boolean;
  viewingActiveBackend: boolean;
  switchLabel: string;
  providerPreset: ProviderPreset;
  apiKeyLabel: string;
  apiKey: string;
  editingApiKey: boolean;
  connectionStatus: SettingsStatus;
  status: SettingsStatus;
  onSelectSection: (section: SettingsSection) => void;
  onWindowMouseDown: (event: MouseEvent<HTMLElement>) => void;
  onClose: () => void;
  onSubmit: (event: FormEvent) => void;
  onSelectMode: (mode: TranslationMode) => void;
  onToggleReasoning: () => void;
  onSelectBackend: (backend: ModelBackend) => void;
  onSelectProviderPreset: (preset: ProviderPreset) => void;
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
  onActivateBackend: () => void;
}

function SaveIcon({ confirmed }: { confirmed: boolean }) {
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true">
      {confirmed ? (
        <path d="m4.5 10.4 3.4 3.4 7.6-7.6" />
      ) : (
        <>
          <path d="M4 3.5h9.4L16 6.1v10.4H4z" />
          <path d="M6.4 3.5v4.1h6.7V3.5M6.4 16.5v-5.2h7.2v5.2" />
        </>
      )}
    </svg>
  );
}

function ConnectionIcon({ health }: { health: ConnectionHealth }) {
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true">
      {health === "working" ? (
        <g className="connection-test-dots">
          <circle cx="5.5" cy="10" r="1.35" />
          <circle cx="10" cy="10" r="1.35" />
          <circle cx="14.5" cy="10" r="1.35" />
        </g>
      ) : health === "success" ? (
        <path d="m4.6 10.4 3.4 3.4 7.5-7.5" />
      ) : health === "error" ? (
        <>
          <path d="m6 6 8 8" />
          <path d="m14 6-8 8" />
        </>
      ) : (
        <>
          <path d="M6.8 3.6v3.2M13.2 3.6v3.2" />
          <path d="M5.2 6.8h9.6v1.5a4.8 4.8 0 0 1-9.6 0z" />
          <path d="M10 13.1v3.3" />
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
  loaded,
  translationSaving,
  modelSaving,
  modelSwitching,
  saveConfirmed,
  providerDirty,
  viewingActiveBackend,
  switchLabel,
  providerPreset,
  apiKeyLabel,
  apiKey,
  editingApiKey,
  connectionStatus,
  status,
  onSelectSection,
  onWindowMouseDown,
  onClose,
  onSubmit,
  onSelectMode,
  onToggleReasoning,
  onSelectBackend,
  onSelectProviderPreset,
  onSetEndpoint,
  onRefreshApiKeyStatus,
  onBeginApiKeyEdit,
  onSetApiKey,
  onFinishApiKeyEdit,
  onSetTimeoutSeconds,
  onTestConnection,
  onClearApiKey,
  onActivateBackend,
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
          <span className="brand-mark" aria-hidden="true">
            T
          </span>
          <span data-tauri-drag-region>Translay</span>
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
          <button
            className="icon-button"
            type="button"
            onClick={onClose}
            aria-label="关闭"
          >
            <span className="close-mark" aria-hidden="true" />
          </button>
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
                    <small>开启后允许模型推理，关闭时优先快速响应</small>
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
                    <p>选择本地服务或在线 API</p>
                  </div>
                  <div className="source-tabs" aria-label="模型来源">
                    <button
                      type="button"
                      className={config.backend === "local" ? "active" : ""}
                      onClick={() => onSelectBackend("local")}
                    >
                      本地
                    </button>
                    <button
                      type="button"
                      className={config.backend === "api" ? "active" : ""}
                      onClick={() => onSelectBackend("api")}
                    >
                      API
                    </button>
                  </div>
                </div>

                <div className="form-rows">
                  <div className="form-row preset-select-row">
                    <label htmlFor="provider-preset">服务预设</label>
                    <div className="select-control">
                      <select
                        id="provider-preset"
                        value={providerPreset}
                        onChange={(event) =>
                          onSelectProviderPreset(
                            event.target.value as ProviderPreset,
                          )
                        }
                      >
                        {config.backend === "local" ? (
                          <>
                            <option value="ollama">Ollama</option>
                            <option value="lm-studio">LM Studio</option>
                          </>
                        ) : (
                          <>
                            <option value="deepseek">DeepSeek</option>
                            <option value="glm">GLM</option>
                          </>
                        )}
                        {providerPreset === "custom" && (
                          <option value="custom">自定义</option>
                        )}
                      </select>
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
                      saveConfirmed ? "confirmed" : ""
                    }`}
                    type="submit"
                    disabled={
                      !loaded ||
                      !providerDirty ||
                      modelSaving ||
                      modelSwitching ||
                      translationSaving
                    }
                    aria-label={saveConfirmed ? "已保存" : "保存当前配置"}
                  >
                    <SaveIcon confirmed={saveConfirmed} />
                    <span>保存</span>
                  </button>
                </div>
              </div>
            )}
          </div>

          {(section === "model" || status.kind === "error") && (
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
              {section === "model" && (
                <div className="settings-actions">
                  {viewingActiveBackend ? (
                    <span className="active-provider-note">
                      <span aria-hidden="true" />
                      当前正在使用
                    </span>
                  ) : (
                    <button
                      className="primary switch-provider-button"
                      type="button"
                      onClick={onActivateBackend}
                      disabled={
                        !loaded ||
                        providerDirty ||
                        modelSaving ||
                        modelSwitching ||
                        translationSaving
                      }
                      title={providerDirty ? "请先保存当前配置" : undefined}
                    >
                      {modelSwitching ? "正在切换…" : switchLabel}
                    </button>
                  )}
                </div>
              )}
            </footer>
          )}
        </form>
      </section>
    </main>
  );
}
