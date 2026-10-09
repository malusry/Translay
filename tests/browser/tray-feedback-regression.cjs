// Real tray feedback component with isolated, deterministic native events.
const assert = require('node:assert/strict');
const { chromium } = require('playwright');
const { assertFont } = require('./font-evidence.cjs');
const fs = require('node:fs');
(async () => {
  const { createServer } = await import('vite');
  const server = await createServer({ server: { host: '127.0.0.1', port: 0 } });
  await server.listen();
  let browser;
  try {
    browser = await chromium.launch({ channel: 'msedge', headless: true });
    const page = await browser.newPage({ viewport: { width: 64, height: 48 } });
    const evidence = [];
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(() => {
      const callbacks = new Map(), events = new Map(); let id = 0;
      window.emitTray = (name, payload) => callbacks.get(events.get(name))?.({ event: name, payload });
      window.trayReady = () => events.size === 2;
      window.__TAURI_INTERNALS__ = {
        transformCallback: fn => { callbacks.set(++id, fn); return id; },
        unregisterCallback: key => callbacks.delete(key),
        invoke: async (command, args) => {
          if (command === 'plugin:event|listen') { events.set(args.event, args.handler); return ++id; }
        },
      };
    });
    await page.goto(server.resolvedUrls.local[0] + '?view=tray-feedback');
    await page.waitForFunction(() => window.trayReady?.());
    for (const colorScheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme });
      const gradients = [];
      for (const [generation, mode, message] of [[1, 'conversational', '日常'], [2, 'academic', '学习']]) {
        await page.evaluate(payload => window.emitTray('tray-style-feedback-show', payload), { generation, mode, message });
        await page.waitForFunction(text => document.querySelector('.tray-feedback-label')?.textContent === text, message);
        await page.locator('.tray-feedback-pill').evaluate(async element => {
          await document.fonts.ready;
          await Promise.all(element.getAnimations().map(animation => animation.finished));
        });
        const result = await page.locator('.tray-feedback-pill').evaluate(element => {
          const r = element.getBoundingClientRect(), style = getComputedStyle(element);
          const label = element.querySelector('span').getBoundingClientRect();
          return { width: r.width, height: r.height, inside: r.left >= 0 && r.top >= 0 && r.right <= innerWidth && r.bottom <= innerHeight,
            leftSpace: label.left - r.left, rightSpace: r.right - label.right, fontSize: style.fontSize,
            border: style.borderTopWidth, background: style.backgroundImage, opacity: Number(style.opacity),
            gradient: getComputedStyle(element.querySelector('span')).backgroundImage };
        });
        assert.equal(result.inside, true);
        assert.equal(result.border, '0px');
        assert.equal(result.width, 36);
        assert.equal(result.height, 24);
        assert.equal(result.fontSize, '13px');
        assert(result.leftSpace >= 3 && result.rightSpace >= 3, 'both labels must retain visible side spacing without clipping');
        const backgroundAlpha = [...result.background.matchAll(/rgba\([^)]*,\s*([\d.]+)\)/g)].map(match => Number(match[1]));
        assert.equal(backgroundAlpha.length, 2);
        assert(backgroundAlpha.every(alpha => alpha > 0 && alpha < 1), 'both background stops must be translucent');
        assert(backgroundAlpha[0] > backgroundAlpha[1], 'background must become more transparent toward the second stop');
        assert.equal(result.opacity, 1, 'transparency belongs to the background, not the text');
        const fonts = await assertFont(page, '.tray-feedback-label', mode === 'academic' ? 'TranslayReading' : 'TranslayDisplay');
        evidence.push({ colorScheme, mode, message, ...result, fonts });
        gradients.push(result.gradient);
      }
      assert.notEqual(gradients[0], gradients[1]);
      await page.evaluate(() => window.emitTray('tray-style-feedback-dismiss', 1));
      await page.waitForTimeout(30);
      assert.equal(await page.locator('.tray-feedback-stage--visible').count(), 1);
      await page.evaluate(() => window.emitTray('tray-style-feedback-dismiss', 2));
      await page.locator('.tray-feedback-stage--leaving').waitFor();
    }
    assert.deepEqual(errors, []);
    fs.writeFileSync('tests/artifacts/font-design-feedback.json', JSON.stringify(evidence, null, 2));
    console.log('PASS: two-character 36x24 tray labels inside 64x48 viewport; clear side spacing and unchanged 13px fonts; translucent light/dark gradients and stale-dismiss isolation.');
  } finally { if (browser) await browser.close(); await server.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
