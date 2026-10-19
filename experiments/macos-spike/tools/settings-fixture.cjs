// Executed inside a browser page. No host credentials, files or model services.
function installMacFixture() {
  window.isTauri = true;
  const callbacks = new Map(), registrations = new Map(), keys = new Map();
  let id = 0;
  window.permissionTrusted = false;
  window.authorizationRequests = 0;
  window.initialReads = [];
  window.connectionRequests = [];
  window.saveRequests = 0;
  window.failNextSave = false;
  window.openedPortals = [];
  window.modelConfig = {
    backend: "local", mode: "conversational", reasoningEnabled: false,
    local: { baseUrl: "http://127.0.0.1:11434/v1", model: "fixture-local" },
    api: { baseUrl: "https://api.openai.com/v1", model: "fixture-api" },
    localModels: {}, apiModels: {}, timeoutSeconds: 60, hasApiKey: false, apiKeyHint: null,
  };
  let snapshot = { selectionIconEnabled: true, mode: "conversational", revision: 0, exiting: false };
  const emit = (event, payload) => {
    for (const registration of registrations.values()) {
      if (registration.event === event) callbacks.get(registration.handler)?.({ payload: structuredClone(payload) });
    }
  };
  const keyStatus = (baseUrl) => {
    const key = keys.get(new URL(baseUrl).origin);
    return { hasApiKey: Boolean(key), apiKeyHint: key ? `••••••••${key.slice(-4)}` : null };
  };
  const view = () => ({ ...structuredClone(window.modelConfig), ...keyStatus(window.modelConfig.api.baseUrl) });
  window.emitSpikeState = (payload) => {
    snapshot = structuredClone(payload);
    window.modelConfig.mode = payload.mode;
    emit("spike-state", payload);
    emit("model-backend-changed", view());
  };
  window.releaseInitialReads = () => window.initialReads.splice(0).forEach((resolve) => resolve({
    selectionIconEnabled: true, mode: "conversational", revision: 0, exiting: false,
  }));
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (_event, listenerId) => {
      callbacks.delete(registrations.get(listenerId)?.handler);
      registrations.delete(listenerId);
    },
  };
  window.__TAURI_INTERNALS__ = {
    transformCallback: (callback) => { callbacks.set(++id, callback); return id; },
    unregisterCallback: (callbackId) => callbacks.delete(callbackId),
    invoke: async (command, args) => {
      if (command === "plugin:event|listen") { registrations.set(++id, args); return id; }
      if (command === "plugin:event|unlisten") { registrations.delete(args.eventId); return; }
      if (command === "get_spike_state") return new Promise((resolve) => window.initialReads.push(resolve));
      if (command === "get_model_config") return view();
      if (command === "get_model_api_key_status") return keyStatus(args.baseUrl);
      if (command === "detect_active_local_model") return null;
      if (command === "activate_model_backend") {
        window.modelConfig.backend = args.backend; emit("model-backend-changed", view()); return view();
      }
      if (command === "save_translation_preferences") {
        Object.assign(window.modelConfig, args);
        snapshot = { ...snapshot, mode: args.mode, revision: snapshot.revision + 1 };
        emit("spike-state", snapshot); emit("model-backend-changed", view()); return;
      }
      if (command === "save_model_provider_config") {
        window.saveRequests++;
        if (window.failNextSave) { window.failNextSave = false; throw new Error("Fixture save failure"); }
        const input = args.input;
        window.modelConfig[input.backend] = { ...input[input.backend] };
        window.modelConfig.timeoutSeconds = input.timeoutSeconds;
        if (input.backend === "api" && input.apiKey) keys.set(new URL(input.api.baseUrl).origin, input.apiKey);
        emit("model-backend-changed", view()); return view();
      }
      if (command === "test_model_connection") return new Promise((resolve) => window.connectionRequests.push({ input: args.input, resolve }));
      if (command === "clear_model_api_key") {
        keys.delete(new URL(args.baseUrl).origin); emit("model-backend-changed", view()); return keyStatus(args.baseUrl);
      }
      if (command === "request_accessibility") window.authorizationRequests++;
      if (command === "get_accessibility_status" || command === "request_accessibility") return { supported: true, trusted: window.permissionTrusted };
      if (command === "open_system_settings") return;
      if (command === "open_provider_api_portal") { window.openedPortals.push(args.provider); return; }
      if (command === "hide_settings_window") { window.fixtureHidden = true; return; }
      if (command === "minimize_settings_window" || command === "start_settings_dragging") return;
      throw new Error(`Unexpected fixture command: ${command}`);
    },
  };
}
module.exports = { installMacFixture };
