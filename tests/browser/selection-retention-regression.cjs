const assert = require('node:assert/strict');
const { chromium } = require('playwright');

(async () => {
  const { createServer } = await import('vite');
  const server = await createServer({ server: { host: '127.0.0.1', port: 0 } });
  let browser;
  try {
    await server.listen();
    browser = await chromium.launch({ channel: 'msedge', headless: true });
    const page = await browser.newPage({ viewport: { width: 40, height: 40 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(() => {
      const callbacks = new Map(), listeners = new Map(); let id = 0;
      window.selectionCalls = 0;
      window.emitVisibility = payload => {
        for (const handler of listeners.values()) callbacks.get(handler)?.({ payload });
      };
      window.selectionReady = () => listeners.size > 0;
      window.__TAURI_INTERNALS__ = {
        transformCallback: fn => { callbacks.set(++id, fn); return id; },
        unregisterCallback: key => callbacks.delete(key),
        invoke: async (command, args) => {
          if (command === 'plugin:event|listen') { listeners.set(++id, args.handler); return id; }
          if (command === 'plugin:event|unlisten') { listeners.delete(args.eventId); return; }
          if (command === 'translate_detected_selection') {
            window.selectionCalls++;
            return new Promise(resolve => { window.finishTranslation = resolve; });
          }
        },
      };
    });
    await page.goto(server.resolvedUrls.local[0] + '?view=selection-button');
    await page.waitForFunction(() => window.selectionReady?.());
    const shell = page.locator('.selection-button-shell');
    const mark = page.locator('.selection-button-mark');
    const button = page.getByRole('button', { name: '翻译选中文字' });
    await page.waitForFunction(() => document.querySelector('img')?.naturalWidth > 0);
    const initial = await mark.boundingBox();
    await button.hover();
    await page.waitForTimeout(160);
    assert.deepEqual(await mark.boundingBox(), initial);
    await page.evaluate(() => window.emitVisibility({ revision: 1, visible: false }));
    await page.waitForFunction(() => document.querySelector('main').dataset.visible === 'false');
    assert.equal(await shell.evaluate(e => getComputedStyle(e).transitionDuration), '0.12s');
    await page.waitForFunction(() => getComputedStyle(document.querySelector('main')).opacity === '0', null, { timeout: 3000 });
    // A new selection or returning pointer restores immediately; an old event cannot undo it.
    await page.evaluate(() => {
      window.emitVisibility({ revision: 3, visible: true });
      window.emitVisibility({ revision: 2, visible: false });
    });
    await page.waitForFunction(() => document.querySelector('main').dataset.visible === 'true');
    assert.equal(await shell.evaluate(e => getComputedStyle(e).opacity), '1');
    assert.deepEqual(await mark.boundingBox(), initial);
    await button.click();
    await button.click();
    assert.equal(await page.evaluate(() => window.selectionCalls), 1);
    await page.evaluate(() => window.finishTranslation(true));
    await page.waitForTimeout(30);
    await button.click();
    assert.equal(await page.evaluate(() => window.selectionCalls), 2);
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.evaluate(() => window.emitVisibility({ revision: 4, visible: false }));
    await page.waitForFunction(() => document.querySelector('main').dataset.visible === 'false');
    assert.equal(await shell.evaluate(e => getComputedStyle(e).transitionDuration), '0s');
    assert.deepEqual(errors, []);
    console.log('PASS selection fade, restore, stale-event isolation, stable position, click guard, reduced motion');
  } finally {
    await browser?.close();
    await server.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
