// Deterministic IPC scheduling around the real Overlay component. No desktop input or model calls.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');

(async () => {
  const { createServer } = await import('vite');
  const server = await createServer({ server: { host: '127.0.0.1', port: 0 } });
  await server.listen();
  let browser;
  try {
    browser = await chromium.launch({ headless: true, channel: process.env.QA_BROWSER || 'msedge' });
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
              return new Promise((resolve, reject) => window.heldReads.push({ snapshot, resolve, reject }));
            }
            return snapshot;
          }
          if (cmd === 'plugin:event|listen') { const id = next++; events.set(args.event, { id, handler: args.handler }); return id; }
          if (cmd === 'plugin:event|unlisten') return;
          if (cmd === 'fit_overlay_height') return window.nativeFit(args.logicalHeight);
          if (cmd === 'retry_capture') return new Promise((resolve, reject) => window.pendingRetry = { resolve, reject });
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
    async function holdSnapshot() {
      const count = await page.evaluate(() => window.heldReads.length);
      await page.evaluate(() => { window.holdRead = true; window.emit(); });
      await page.waitForFunction(count => window.heldReads.length > count, count);
      return count;
    }
    async function releaseSnapshot(index) {
      await page.evaluate(index => { const read = window.heldReads[index]; read.resolve(read.snapshot); }, index);
      // Allow the deliberately delayed invoke promise and React commit to complete.
      await page.waitForTimeout(120);
    }

    const oldRead = await holdSnapshot();
    await deliver({ requestId: 101, phase: 'translated', text: '最新译文', toneNote: '当前选区的附注。' });
    await page.getByText('当前选区的附注。', { exact: true }).waitFor();
    await releaseSnapshot(oldRead);
    assert.equal(await page.locator('.tone-note').innerText(), '当前选区的附注。');
    console.log('PASS: a delayed read from the previous selection cannot replace the latest result.');

    const failingRead = await holdSnapshot();
    await deliver({ phase: 'translated', text: '通信延迟后的有效译文', toneNote: '有效译文应当保留。' });
    await page.getByText('有效译文应当保留。', { exact: true }).waitFor();
    await page.evaluate(index => window.heldReads[index].reject(new Error('Synthetic delayed IPC read failure')), failingRead);
    await page.waitForTimeout(120);
    assert.equal(await page.locator('.tone-note').count(), 1, 'Late IPC failure erased an already delivered translation');
    assert.equal(await page.locator('.tone-note').innerText(), '有效译文应当保留。');
    console.log('PASS: a delayed IPC failure preserves an already delivered translation.');

    await deliver({ requestId: 102, phase: 'translating', text: '', toneNote: null });
    await page.locator('.overlay.translating').waitFor();
    const earlierPhase = await holdSnapshot();
    await deliver({ phase: 'translated', text: '同一次请求的最终译文', toneNote: '最终附注。' });
    await page.getByText('最终附注。', { exact: true }).waitFor();
    await releaseSnapshot(earlierPhase);
    assert.equal(await page.locator('.overlay.translating').count(), 0, 'Late loading snapshot replaced a completed translation');
    assert.equal(await page.locator('.tone-note').innerText(), '最终附注。');
    console.log('PASS: a delayed loading snapshot cannot roll back the same request.');

    await deliver({ requestId: 103, phase: 'translating', text: '', toneNote: null });
    await page.locator('.overlay.translating').waitFor();
    const closingRead = await holdSnapshot();
    await page.evaluate(index => { window.heldReads[index].snapshot = { ...window.payload, phase: 'translated', text: 'DISMISSED RESULT', toneNote: 'DISMISSED NOTE' }; }, closingRead);
    await page.evaluate(() => { window.holdDismiss = true; });
    await page.getByRole('button', { name: '关闭翻译', exact: true }).click();
    await page.waitForFunction(() => window.heldDismiss.length === 1);
    await releaseSnapshot(closingRead);
    assert.equal(await page.getByText('DISMISSED NOTE', { exact: true }).count(), 0);
    await deliver({ requestId: 104, phase: 'translated', text: '新的学术选区', toneNote: null, translationMode: 'academic' });
    await page.getByRole('button', { name: '查看详细解释', exact: true }).click();
    await page.getByText('NEW EXPLANATION', { exact: true }).waitFor();
    await page.evaluate(outcome => {
      if (outcome === 'reject') window.heldDismiss[0].reject(new Error('Synthetic delayed close error'));
      else window.heldDismiss[0].resolve(outcome !== 'false');
    }, process.env.QA_DISMISS_RESULT || 'true');
    await page.waitForTimeout(120);
    assert.equal(await page.getByText('NEW EXPLANATION', { exact: true }).count(), 1, 'Old close completion cleared the new selection explanation');
    assert.equal(await page.getByText('DISMISSED NOTE', { exact: true }).count(), 0);
    console.log('PASS: closing while translating and reopening ignores the old close completion.');

    await deliver({ requestId: 105, phase: 'translated', text: '再次划词的日常译文', translationMode: 'conversational', toneNote: null });
    await page.getByText('再次划词的日常译文', { exact: true }).waitFor();
    assert.equal(await page.locator('.tone-note').count(), 0);
    await page.getByRole('button', { name: '复制译文', exact: true }).click();
    assert.equal(await page.evaluate(() => window.copied), '再次划词的日常译文');
    // Leave room outside the surface while keeping browser focus during drag.
    await page.setViewportSize({ width: 640, height: 300 });
    await page.addStyleTag({ content: "main.overlay-stage { width: 360px; height: 100px; }" });
    await page.mouse.move(500, 200);
    await page.evaluate(() => window.emitAutomaticDismiss());
    await page.waitForTimeout(600);
    assert.notEqual(await page.locator('main').getAttribute('data-motion-state'), 'dismissing', 'Copy feedback disappeared before one second');
    await page.waitForFunction(() => window.calls.filter(c => c.cmd === 'set_overlay_hovered').at(-1)?.args.hovered === false);
    console.log('PASS: copy feedback survives leaving and expiry, then releases its temporary pin.');
    // Real browser mouse/selection events, with only the native expiry event mocked.
    await page.locator('.overlay').hover();
    await page.evaluate(() => window.emitAutomaticDismiss());
    await page.waitForTimeout(100);
    assert.notEqual(await page.locator('main').getAttribute('data-motion-state'), 'dismissing');
    await page.mouse.down();
    await page.mouse.move(500, 200);
    await page.evaluate(() => window.emitAutomaticDismiss());
    await page.waitForTimeout(100);
    assert.notEqual(await page.locator('main').getAttribute('data-motion-state'), 'dismissing');
    await page.mouse.up();
    await page.waitForFunction(() => window.calls.filter(c => c.cmd === 'set_overlay_hovered').at(-1)?.args.hovered === false);
    await page.evaluate(() => window.emitAutomaticDismiss());
    await page.waitForFunction(() => document.querySelector('main').dataset.motionState === 'dismissing');
    await page.mouse.move(100, 25);
    await page.waitForFunction(() => document.querySelector('main').dataset.motionState !== 'dismissing');
    console.log('PASS: hover and selection drag reject automatic dismissal; release allows it; reentry cancels it.');
    for (const [offset, failure] of [[0, false], [2, true]]) {
      await deliver({ requestId: 106 + offset, text: 'Copy before switching' });
      await page.getByRole('button', { name: '复制译文', exact: true }).waitFor();
      await page.evaluate(() => { window.holdCopy = true; });
      await page.getByRole('button', { name: '复制译文', exact: true }).click();
      await page.waitForFunction(() => !!window.pendingCopy);
      await deliver({ requestId: 107 + offset, text: 'Copy after switching' });
      await page.getByRole('button', { name: '复制译文', exact: true }).click();
      await page.getByRole('button', { name: '已复制', exact: true }).waitFor();
      await page.evaluate(failure => {
        if (failure) window.pendingCopy.reject(new Error('Synthetic clipboard failure'));
        else window.pendingCopy.resolve();
        window.pendingCopy = null;
      }, failure);
      await page.waitForTimeout(60);
      assert.equal(await page.getByRole('button', { name: '已复制', exact: true }).count(), 1);
    }
    console.log('PASS: delayed clipboard success/failure cannot clear new selection copy feedback.');
    for (const [offset, outcome] of [[0, 'true'], [2, 'false'], [4, 'reject']]) {
      await deliver({ requestId: 110 + offset, phase: 'translationFailed', success: false, text: '', errorMessage: '测试翻译失败' });
      await page.getByRole('button', { name: '重新翻译', exact: true }).click();
      await page.waitForFunction(() => !!window.pendingRetry);
      await deliver({ requestId: 111 + offset, phase: 'translated', success: true, text: '重试途中重新划词的译文', errorMessage: null });
      await page.getByText('重试途中重新划词的译文', { exact: true }).waitFor();
      await page.evaluate(outcome => {
        if (outcome === 'reject') window.pendingRetry.reject(new Error('Synthetic delayed retry failure'));
        else window.pendingRetry.resolve(outcome === 'true');
        window.pendingRetry = null;
      }, outcome);
      await page.waitForTimeout(120);
      assert.equal(await page.getByText('重试途中重新划词的译文', { exact: true }).count(), 1);
      assert.equal(await page.getByText('未能开始重试，请再试一次', { exact: true }).count(), 0);
    }
    console.log('PASS: delayed retry success, refusal and failure cannot alter a newer selection.');
    assert.deepEqual(errors, []);
    console.log(`PASS: new daily selection has no stale note and copies only its own translation; old close outcome=${process.env.QA_DISMISS_RESULT || 'true'}.`);
  } finally {
    if (browser) await browser.close();
    await server.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
