const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("playwright");
const { installMacFixture } = require("./settings-fixture.cjs");
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
    await nativePage.addInitScript(installMacFixture);
    await nativePage.goto(server.resolvedUrls.local[0]);
    await nativePage.waitForFunction(() => window.initialReads.length > 0 && document.querySelector(".daily-mode")?.disabled === false);
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
    // The real shared Settings controller is editable in native mode.
    await nativePage.locator(".daily-mode").click();
    await nativePage.waitForFunction(() => window.modelConfig.mode === "conversational");
    await nativePage.getByRole("switch", { name: "翻译思考" }).click();
    await nativePage.waitForFunction(() => window.modelConfig.reasoningEnabled);
    await nativePage.getByRole("button", { name: "模型", exact: true }).click();
    await nativePage.locator("#model-name").fill("fixture-updated-local");
    await nativePage.evaluate(() => { window.failNextSave = true; });
    await nativePage.getByRole("button", { name: "保存当前配置", exact: true }).click();
    await nativePage.getByText(/Fixture save failure/).waitFor();
    assert.equal(await nativePage.locator("#model-name").inputValue(), "fixture-updated-local");
    await nativePage.getByRole("button", { name: "保存当前配置", exact: true }).click();
    await nativePage.getByRole("button", { name: "当前配置已保存", exact: true }).waitFor();
    assert.equal(await nativePage.evaluate(() => window.saveRequests), 2);
    await nativePage.locator(".provider-test-button").click();
    await nativePage.locator("#model-name").fill("fixture-new-request");
    await nativePage.locator(".provider-test-button").click();
    await nativePage.waitForFunction(() => window.connectionRequests.length === 2);
    await nativePage.evaluate(() => {
      window.connectionRequests[1].resolve({ success: true, message: "new fixture", elapsedMs: 222 });
      window.connectionRequests[0].resolve({ success: false, message: "stale fixture failure", elapsedMs: 999 });
    });
    await nativePage.getByText("连接成功 · 222 ms", { exact: true }).waitFor();
    assert.equal(await nativePage.getByText("stale fixture failure", { exact: false }).count(), 0);
    await nativePage.getByRole("button", { name: "切换到在线 API", exact: true }).click();
    await nativePage.locator("#api-key").fill("fake-fixture-only-7F2A");
    await nativePage.getByRole("button", { name: "保存当前配置", exact: true }).click();
    await nativePage.getByRole("button", { name: "当前配置已保存", exact: true }).waitFor();
    await nativePage.locator(".credential-slot").click();
    await nativePage.getByRole("button", { name: /移除 .*API Key/ }).click();
    await nativePage.waitForFunction(() => document.querySelector("#api-key")?.value === "");
    await nativePage.locator(".provider-openai").click({ button: "right" });
    assert.deepEqual(await nativePage.evaluate(() => window.openedPortals), ["openai"]);
    await nativePage.screenshot({ path: path.join(artifacts, "native-model-ipc-fixture.png") });
    await nativePage.getByRole("button", { name: "翻译", exact: true }).click();
    await nativePage.screenshot({ path: path.join(artifacts, "frontend-ipc-fixture.png") });
    assert.deepEqual(errors, []);
    console.log("PASS: shared translation/local/API settings views and bundled fonts, read-only preview, small-window scrolling, native chrome separation, unsupported preview permissions, stale snapshots, asynchronous authorization/revocation, editable native settings, failed-save retry, stale connection isolation, masked credentials and deletion. Native Mac controls and focus were not exercised.");
  } finally {
    if (browser) await browser.close();
    await server.close();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
