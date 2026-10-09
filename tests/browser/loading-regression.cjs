// Deterministic IPC scheduling around the real Overlay component. No desktop input or model calls.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');

(async () => {
  const { createServer } = await import('vite');
  const server = await createServer({ server: { host: '127.0.0.1', port: 0 } });
  await server.listen();
  let browser;
  try {
    browser = await chromium.launch({ headless: true, channel: 'msedge' });
    const page = await browser.newPage({ viewport: { width: 360, height: 160 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.exposeFunction('nativeFit', async height => {
      await page.setViewportSize({ width: 360, height: Math.max(60, Math.min(648, height)) });
      return true;
    });
    await page.addInitScript(() => {
      const callbacks = new Map(), events = new Map();
      let next = 1;
      window.calls = []; window.heldReads = []; window.heldDismiss = [];
      window.holdRead = false; window.holdDismiss = false;
      window.payload = { requestId: 100, phase: 'translating', success: true, text: '',
        applicationName: 'Synthetic race test', processId: 1, captureMethod: 'Simulated IPC', elapsedMs: 0,
        selectionRect: null, errorCode: null, errorMessage: null, focusPreserved: true,
        clipboardRestored: true, warningCode: null, languageProfile: null,
        translationMode: 'conversational', toneNote: null };
      window.emit = () => {
        for (const [event, listener] of events) if (event === 'capture-result') {
          callbacks.get(listener.handler)?.({ event, payload: window.payload });
        }
      };
      window.emitAutomaticDismiss = () => {
        const listener = events.get('overlay-dismiss-requested');
        callbacks.get(listener.handler)?.({ event: 'overlay-dismiss-requested', payload: window.payload.requestId });
      };
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
        unregisterListener: (event, eventId) => {
          const listener = events.get(event);
          if (listener?.id === eventId) {
            callbacks.delete(listener.handler);
            events.delete(event);
          }
        },
      };
      window.__TAURI_INTERNALS__ = {
        transformCallback: fn => { const id = next++; callbacks.set(id, fn); return id; },
        unregisterCallback: id => callbacks.delete(id),
        invoke: async (cmd, args = {}) => {
          window.calls.push({ cmd, args });
          if (cmd === 'get_latest_capture') {
            const snapshot = structuredClone(window.payload);
            if (window.holdRead) {
              window.holdRead = false;
              return new Promise(resolve => window.heldReads.push({ snapshot, resolve }));
            }
            return snapshot;
          }
          if (cmd === 'plugin:event|listen') { const id = next++; events.set(args.event, { id, handler: args.handler }); return id; }
          if (cmd === 'plugin:event|unlisten') return;
          if (cmd === 'fit_overlay_height') return window.nativeFit(args.logicalHeight);
          if (cmd === 'copy_translation') { window.copied = args.text; if (window.holdCopy) { window.holdCopy = false; return new Promise((resolve, reject) => window.pendingCopy = { resolve, reject }); } return; }
          if (cmd === 'dismiss_overlay' && window.holdDismiss) {
            return new Promise((resolve, reject) => window.heldDismiss.push({ args, resolve, reject }));
          }
          if (cmd === 'explain_translation') return { coreExplanation: 'NEW EXPLANATION', keyConcepts: [], caveat: '' };
          return true;
        },
      };
    });
    await page.goto(server.resolvedUrls.local[0]);
    await page.locator('.overlay.translating').waitFor();

    async function deliver(update) {
      await page.evaluate(update => { window.payload = { ...window.payload, ...update }; window.emit(); }, update);
    }
    // Slow response: feedback changes without resizing or replacing the request.
    const initialSize = page.viewportSize();
    await page.waitForFunction(() => document.querySelector('.status')?.textContent.includes('仍在翻译'), null, { timeout: 10000 });
    assert.deepEqual(page.viewportSize(), initialSize);
    await deliver({ phase: 'translated', text: '等待结束后的译文。' });
    await page.waitForFunction(() => document.querySelector('.overlay-stage')?.dataset.motionState === 'settled');
    assert.equal(await page.getByText('仍在翻译', { exact: true }).count(), 0);

    // Fast response: send the result one paint after the new loading request.
    await page.evaluate(async () => {
      window.realNow = performance.now.bind(performance);
      const frozen = window.realNow();
      performance.now = () => frozen;
      window.payload = { ...window.payload, requestId: 200, phase: 'translating', text: '' }; window.emit();
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      window.payload = { ...window.payload, phase: 'translated', text: '快速结果。' }; window.emit();
    });
    await page.getByText('快速结果。', { exact: true }).waitFor();
    assert.equal(await page.locator('.departing-loading-shell').count(), 0);
    assert.equal(await page.locator('.overlay-stage').getAttribute('data-motion-state'), 'settled');
    await page.waitForFunction(() => window.calls.some(c => c.cmd === 'ack_capture' && c.args.requestId === 200));

    await page.evaluate(() => { performance.now = window.realNow; });

    // A new request resets the slow label; a completed request cannot be changed by its timer.
    await deliver({ requestId: 201, phase: 'translating', text: '' });
    await page.waitForTimeout(200);
    assert.equal((await page.locator('.status').textContent()).trim(), '翻译中');
    await deliver({ phase: 'translated', text: '新请求完成。' });
    await page.waitForTimeout(8100);
    assert.equal(await page.getByText('仍在翻译', { exact: true }).count(), 0);
    // Native windows can move between capture and translation. Simulate that move
    // and verify the result uses the latest loading origin, not the capture origin.
    await page.setViewportSize({ width: 148, height: 58 });
    await page.evaluate(() => {
      window.testScreenX = 0;
      Object.defineProperty(window, 'screenX', { configurable: true, get: () => window.testScreenX });
    });
    await deliver({ requestId: 202, phase: 'capturing', text: '' });
    await page.locator('.overlay.capturing').waitFor();
    await page.evaluate(() => { window.testScreenX = 120; });
    await deliver({ phase: 'translating' });
    await page.locator('.overlay.translating').waitFor();
    await page.waitForTimeout(200);
    await page.setViewportSize({ width: 360, height: 160 });
    await page.evaluate(() => { window.testScreenX = 0; });
    await deliver({ phase: 'translated', text: '用于检查窗口移动后展开位置的译文。'.repeat(10) });
    await page.waitForFunction(() => document.querySelector('.overlay-stage')?.style.getPropertyValue('--motion-loading-left') === '120px');
    await page.waitForFunction(() => document.querySelector('.overlay-stage')?.dataset.motionState === 'settled');
    await deliver({ requestId: 203, phase: 'capturing', success: false, text: '' });
    await page.locator('.overlay.capturing').waitFor();
    // The native capture-failure path resizes the window before emitting the payload.
    await page.setViewportSize({ width: 188, height: 88 });
    await deliver({ phase: 'captureFailed', errorCode: 'NO_TEXT_SELECTED', errorMessage: '请先选择文字' });
    await page.getByText('请先选择文字', { exact: true }).waitFor();
    assert.equal(await page.locator('.status').getAttribute('data-status'), 'neutral');
    assert.equal(await page.getByRole('button', { name: '重新翻译', exact: true }).count(), 0);
    await deliver({ requestId: 204, phase: 'capturing', errorCode: null, errorMessage: null });
    await page.locator('.overlay.capturing').waitFor();
    await page.setViewportSize({ width: 320, height: 94 });
    await deliver({ phase: 'captureFailed', errorCode: 'CLIPBOARD_TIMEOUT', errorMessage: '复制选区超时' });
    await page.getByText('复制选区超时', { exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '重新翻译', exact: true }).count(), 1);
    await deliver({ requestId: 203, phase: 'captureFailed', success: false, text: '', errorCode: 'NO_TEXT_SELECTED', errorMessage: '请先选择文字' });
    // A late empty-selection result must not replace the newer failure.
    await page.getByText('复制选区超时', { exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '重新翻译', exact: true }).count(), 1);
    assert.deepEqual(errors, []);
    console.log('PASS: fast result has no loading ghost; slow feedback keeps size; timers reset and result acknowledged.');
  } finally {
    if (browser) await browser.close();
    await server.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
