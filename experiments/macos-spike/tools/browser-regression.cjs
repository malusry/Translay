const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("playwright");
const { assertFont } = require("../../../tests/browser/font-evidence.cjs");

(async () => {
  const { createServer } = await import("vite");
  const root = path.resolve(__dirname, "../../..");
  const server = await createServer({
    configFile: path.resolve(__dirname, "../vite.config.ts"),
    server: { host: "127.0.0.1", port: 0 },
  });
  await server.listen();
  let browser;
  try {
    browser = await chromium.launch({ channel: "msedge", headless: true });
    const artifacts = path.join(root, "tests/artifacts/macos-spike");
    fs.mkdirSync(artifacts, { recursive: true });
    const page = await browser.newPage({ viewport: { width: 680, height: 640 } });
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.goto(server.resolvedUrls.local[0]);
    await page.getByText("需要 Mac 验证", { exact: true }).waitFor();
    assert(await page.getByRole("button", { name: "请求授权", exact: true }).isDisabled());
    assert(await page.getByRole("button", { name: "打开系统设置" }).isDisabled());
    assert.equal(await page.locator(".window-controls").count(), 0);
    assert.equal(await page.locator("[data-tauri-drag-region]").count(), 0);
    assert(await page.locator(".daily-mode").isDisabled());
    assert(await page.getByRole("switch", { name: "翻译思考" }).isDisabled());
    const fonts = {};
    for (const [selector, family] of [
      [".settings-header h1", "TranslayDisplay"],
      [".translation-nav", "TranslayDisplay"],
      [".daily-mode strong", "TranslayDisplay"],
      [".study-mode strong", "TranslayReading"],
      [".daily-mode small", "TranslayInterface"],
    ]) fonts[selector] = await assertFont(page, selector, family);
    await page.screenshot({ path: path.join(artifacts, "browser-preview.png") });
    await page.setViewportSize({ width: 640, height: 480 });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await page.getByRole("button", { name: "重新检查" }).scrollIntoViewIfNeeded();
    assert(await page.getByRole("button", { name: "重新检查" }).isVisible());
    assert(await page.locator(".settings-body").evaluate((body) => body.scrollTop > 0 && body.getBoundingClientRect().bottom <= innerHeight));
    await page.screenshot({ path: path.join(artifacts, "browser-preview-small.png") });
    await page.setViewportSize({ width: 680, height: 640 });
    await page.getByRole("button", { name: "模型", exact: true }).click();
    assert(await page.locator("#base-url").isDisabled());
    assert(await page.locator("#model-name").isDisabled());
    assert(await page.getByRole("button", { name: "测试连接", exact: true }).isDisabled());
    assert(await page.getByRole("button", { name: "保存当前配置", exact: true }).isDisabled());
    assert.equal(await page.locator("#base-url").inputValue(), "http://127.0.0.1:11434/v1");
    await page.screenshot({ path: path.join(artifacts, "model-local-preview.png") });
    await page.getByRole("button", { name: "切换到在线 API", exact: true }).click();
    assert(await page.locator("#api-key").isDisabled());
    assert.equal(await page.locator("#api-key").inputValue(), "");
    assert(await page.locator(".provider-preset").first().isDisabled());
    await page.screenshot({ path: path.join(artifacts, "model-api-preview.png") });
    await page.setViewportSize({ width: 640, height: 480 });
    await page.locator(".provider-save-button").scrollIntoViewIfNeeded();
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    const fields = await page.locator(".form-row input").evaluateAll((inputs) => inputs.map((input) => ({ disabled: input.disabled, inside: input.getBoundingClientRect().right <= innerWidth })));
    assert(fields.every((field) => field.disabled && field.inside));
    await page.screenshot({ path: path.join(artifacts, "model-api-preview-small.png") });
    fs.writeFileSync(path.join(artifacts, "shared-settings-fonts.json"), JSON.stringify(fonts, null, 2));
    await page.close();

    // This is a frontend IPC fixture. It does not exercise native Mac UI or grant permissions.
    const nativePage = await browser.newPage({ viewport: { width: 680, height: 640 } });
    nativePage.on("pageerror", (error) => errors.push(error.message));
    await nativePage.addInitScript(() => {
      window.isTauri = true;
      const callbacks = new Map();
      const listeners = new Map();
      const registrations = new Map();
      let id = 0;
      window.permissionTrusted = false;
      window.authorizationRequests = 0;
      window.initialReads = [];
      window.emitSpikeState = (payload) => callbacks.get(listeners.get("spike-state"))?.({ payload });
      window.releaseInitialReads = () => window.initialReads.splice(0).forEach((resolve) => resolve({
        selectionIconEnabled: true, mode: "conversational", revision: 0, exiting: false,
      }));
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event, listenerId) => {
          const handler = registrations.get(listenerId);
          if (listeners.get(event) === handler) listeners.delete(event);
          registrations.delete(listenerId);
          callbacks.delete(handler);
        },
      };
      window.__TAURI_INTERNALS__ = {
        transformCallback: (callback) => { callbacks.set(++id, callback); return id; },
        unregisterCallback: (callbackId) => callbacks.delete(callbackId),
        invoke: async (command, args) => {
          if (command === "plugin:event|listen") {
            listeners.set(args.event, args.handler);
            registrations.set(++id, args.handler);
            return id;
          }
          if (command === "plugin:event|unlisten") return;
          if (command === "get_spike_state") return new Promise((resolve) => window.initialReads.push(resolve));
          if (command === "request_accessibility") window.authorizationRequests++;
          if (command === "get_accessibility_status" || command === "request_accessibility") {
            return { supported: true, trusted: window.permissionTrusted };
          }
          if (command === "open_system_settings") return;
          throw new Error(`Unexpected fixture command: ${command}`);
        },
      };
    });
    await nativePage.goto(server.resolvedUrls.local[0]);
    await nativePage.waitForFunction(() => window.initialReads.length > 0);
    await nativePage.evaluate(() => {
      window.emitSpikeState({ selectionIconEnabled: false, mode: "academic", revision: 3, exiting: false });
      window.releaseInitialReads();
    });
    await nativePage.getByText("未授权", { exact: true }).waitFor();
    assert.equal(await nativePage.locator(".style-options > button.active strong").innerText(), "学习");
    assert.equal(await nativePage.locator(".mac-state-pill").innerText(), "已关闭");
    await nativePage.getByRole("button", { name: "请求授权", exact: true }).click();
    await nativePage.getByRole("button", { name: "请求授权", exact: true }).waitFor();
    assert.equal(await nativePage.evaluate(() => window.authorizationRequests), 1);
    assert(await nativePage.getByText("未授权", { exact: true }).isVisible());
    await nativePage.evaluate(() => { window.permissionTrusted = true; window.dispatchEvent(new Event("focus")); });
    await nativePage.getByText("已授权", { exact: true }).waitFor();
    assert(await nativePage.getByRole("button", { name: "已获授权" }).isDisabled());
    await nativePage.evaluate(() => { window.permissionTrusted = false; window.dispatchEvent(new Event("focus")); });
    await nativePage.getByText("未授权", { exact: true }).waitFor();
    await nativePage.screenshot({ path: path.join(artifacts, "frontend-ipc-fixture.png") });
    assert.deepEqual(errors, []);
    console.log("PASS: shared translation/local/API settings views and bundled fonts, read-only preview, small-window scrolling, native chrome separation, unsupported preview permissions, stale snapshots, asynchronous authorization and revocation. Native Mac controls and focus were not exercised.");
  } finally {
    if (browser) await browser.close();
    await server.close();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
