const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("playwright");

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
    const page = await browser.newPage({ viewport: { width: 740, height: 570 } });
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.goto(server.resolvedUrls.local[0]);
    await page.getByText("需要 Mac 验证", { exact: true }).waitFor();
    assert(await page.getByRole("button", { name: "请求授权", exact: true }).isDisabled());
    assert(await page.getByRole("button", { name: "打开系统设置" }).isDisabled());
    await page.screenshot({ path: path.join(artifacts, "browser-preview.png") });
    await page.setViewportSize({ width: 640, height: 480 });
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    assert(await page.locator("footer").evaluate((footer) => footer.getBoundingClientRect().bottom <= innerHeight));
    await page.screenshot({ path: path.join(artifacts, "browser-preview-small.png") });
    await page.close();

    // This is a frontend IPC fixture. It does not exercise native Mac UI or grant permissions.
    const nativePage = await browser.newPage({ viewport: { width: 740, height: 570 } });
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
    assert.equal(await nativePage.locator(".mode-tile.selected .mode-name").innerText(), "学习模式");
    assert.equal(await nativePage.locator(".state-pill").innerText(), "已关闭");
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
    console.log("PASS: browser layout, unsupported preview permissions, stale snapshot delivery, asynchronous authorization and revocation. Native Mac controls and focus were not exercised.");
  } finally {
    if (browser) await browser.close();
    await server.close();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
