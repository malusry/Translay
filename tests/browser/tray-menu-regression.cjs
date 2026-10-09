const assert = require('node:assert/strict');
const { chromium } = require('playwright');
const { assertFont, renderedFonts } = require('./font-evidence.cjs');
const fs = require('node:fs');
(async () => {
  const { createServer } = await import('vite');
  const server = await createServer({ server: { host: '127.0.0.1', port: 0 } });
  await server.listen(); let browser;
  try {
    browser = await chromium.launch({ channel: 'msedge', headless: true });
    const page = await browser.newPage({ viewport: { width: 188, height: 160 } });
    const errors = []; page.on('pageerror', e => errors.push(e.message));
    await page.addInitScript(() => {
      const callbacks = new Map(), events = new Map(); let id = 0;
      window.calls = []; window.completed = [];
      const held = new Set(), waiting = new Map();
      window.holdReply = (command, generation) => held.add(`${command}:${generation}`);
      window.releaseReply = (command, generation, fail = false) => {
        const key = `${command}:${generation}`;
        held.delete(key);
        const reply = waiting.get(key);
        if (!reply) throw new Error(`No pending reply: ${key}`);
        waiting.delete(key);
        fail ? reply.reject(new Error('synthetic IPC failure')) : reply.resolve();
      };
      window.openMenu = (generation, modelEnabled = true, selectionEnabled = true) => callbacks.get(events.get('tray-menu-open'))?.({ payload: { generation, open: true, modelLabel: '切换至本地模型', modelEnabled, selectionEnabled } });
      window.__TAURI_INTERNALS__ = {
        transformCallback: fn => { callbacks.set(++id, fn); return id; }, unregisterCallback: key => callbacks.delete(key),
        invoke: async (command, args) => {
          if (command === 'plugin:event|listen') { events.set(args.event, args.handler); return ++id; }
          window.calls.push({ command, args });
          if (command === 'get_tray_menu') return { generation: 1, open: true, modelLabel: '切换至本地模型', modelEnabled: false, selectionEnabled: true };
          const key = `${command}:${args?.generation}`;
          if (held.has(key)) await new Promise((resolve, reject) => waiting.set(key, { resolve, reject }));
          if (command === 'tray_menu_action') await new Promise(resolve => setTimeout(resolve, 150));
          window.completed.push(key);
        },
      };
    });
    await page.goto(server.resolvedUrls.local[0] + '?view=tray-menu');
    await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'menu');
    const fonts = {
      model: await assertFont(page, '.tray-menu button:first-child > span', 'TranslayInterface'),
      settings: await assertFont(page, '.tray-menu-settings > span', 'TranslayDisplay'),
      selection: await assertFont(page, '.tray-menu button[role="menuitemcheckbox"] > span', 'TranslayInterface'),
    };
    fs.writeFileSync('tests/artifacts/font-design-menu.json', JSON.stringify(fonts, null, 2));
    assert.equal(await page.getByRole('menuitem').first().isDisabled(), true);
    assert.equal(await page.locator('button:focus').count(), 0);
    await page.keyboard.press('ArrowDown'); assert.equal(await page.locator(':focus').innerText(), '划词图标');
    await page.keyboard.press('ArrowDown'); assert.equal(await page.locator(':focus').innerText(), '配置');
    await page.keyboard.press('End'); assert.equal(await page.locator(':focus').innerText(), '退出');
    await page.keyboard.press('Home'); assert.equal(await page.locator(':focus').innerText(), '划词图标');
    await page.keyboard.press('Escape');
    assert.ok(await page.evaluate(() => window.calls.some(c => c.command === 'dismiss_tray_menu' && c.args.generation === 1)));
    for (const colorScheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme });
      assert.equal(await page.locator('.tray-menu').evaluate(el => { const r = el.getBoundingClientRect(); return r.width === 164 && r.height === 136 && r.bottom <= innerHeight && r.right <= innerWidth; }), true);
    }
    let generation = 2;
    for (const [label, action] of [['切换至本地模型', 'switch-model-backend'], ['划词图标', 'selection-icon'], ['配置', 'settings'], ['退出', 'quit']]) {
      await page.evaluate(g => window.openMenu(g), generation++);
      const button = page.getByRole(label === '划词图标' ? 'menuitemcheckbox' : 'menuitem', { name: label, exact: true });
      await page.waitForFunction(g => window.calls.some(c => c.command === 'tray_menu_painted' && c.args.generation === g), generation - 1);
      await button.waitFor(); await button.evaluate(el => { el.click(); el.click(); });
      await page.waitForTimeout(180);
      assert.equal(await page.evaluate(a => window.calls.filter(c => c.command === 'tray_menu_action' && c.args.action === a).length, action), 1);
    }
    await page.evaluate(() => { window.openMenu(10, true, false); window.openMenu(2, false, true); });
    await page.waitForFunction(() => document.querySelector('[role="menuitemcheckbox"]')?.getAttribute('aria-checked') === 'false');
    await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'menu');
    const model = page.getByRole('menuitem').first();
    await page.mouse.move(1, 1);
    assert.equal(await model.evaluate(el => getComputedStyle(el).backgroundColor), 'rgba(0, 0, 0, 0)');
    await model.hover(); await page.waitForTimeout(120);
    assert.notEqual(await model.evaluate(el => getComputedStyle(el).backgroundColor), 'rgba(0, 0, 0, 0)');
    await page.mouse.move(1, 1); await page.waitForTimeout(120);
    assert.equal(await model.evaluate(el => getComputedStyle(el).backgroundColor), 'rgba(0, 0, 0, 0)');
    await page.mouse.click(2, 2);
    assert.ok(await page.evaluate(() => window.calls.some(c => c.command === 'dismiss_tray_menu' && c.args.generation === 10)));
    // A delayed paint acknowledgement must not steal the new session's keyboard focus.
    await page.evaluate(() => { window.holdReply('tray_menu_painted', 11); window.openMenu(11); });
    await page.waitForFunction(() => window.calls.some(c => c.command === 'tray_menu_painted' && c.args.generation === 11));
    await page.evaluate(() => window.openMenu(12));
    await page.waitForFunction(() => window.completed.includes('tray_menu_painted:12'));
    await page.keyboard.press('ArrowDown');
    assert.equal(await page.locator(':focus').innerText(), '切换至本地模型');
    await page.evaluate(() => window.releaseReply('tray_menu_painted', 11));
    await page.waitForFunction(() => window.completed.includes('tray_menu_painted:11'));
    assert.equal(await page.locator(':focus').innerText(), '切换至本地模型');
    // An old action's finally callback must not release a newer action's busy state.
    await page.evaluate(() => { window.holdReply('tray_menu_action', 13); window.openMenu(13); });
    await page.waitForFunction(() => window.completed.includes('tray_menu_painted:13'));
    await page.getByRole('menuitem', { name: '配置', exact: true }).click();
    await page.waitForFunction(() => window.calls.some(c => c.command === 'tray_menu_action' && c.args.generation === 13));
    await page.evaluate(() => { window.holdReply('tray_menu_action', 14); window.openMenu(14); });
    await page.waitForFunction(() => window.completed.includes('tray_menu_painted:14'));
    await page.getByRole('menuitemcheckbox').click();
    await page.waitForFunction(() => window.calls.some(c => c.command === 'tray_menu_action' && c.args.generation === 14));
    await page.evaluate(() => window.releaseReply('tray_menu_action', 13));
    await page.waitForFunction(() => window.completed.includes('tray_menu_action:13'));
    assert.equal(await page.locator('.tray-menu button:disabled').count(), 4);
    await page.evaluate(() => window.releaseReply('tray_menu_action', 14));
    await page.waitForFunction(() => document.querySelectorAll('.tray-menu button:disabled').length === 0);
    await page.evaluate(() => { window.holdReply('tray_menu_action', 15); window.openMenu(15); });
    await page.waitForFunction(() => window.completed.includes('tray_menu_painted:15'));
    await page.getByRole('menuitem', { name: '配置', exact: true }).click();
    await page.waitForFunction(() => window.calls.some(c => c.command === 'tray_menu_action' && c.args.generation === 15));
    await page.evaluate(() => window.releaseReply('tray_menu_action', 15, true));
    await page.waitForFunction(() => document.querySelectorAll('.tray-menu button:disabled').length === 0);
    await page.evaluate(() => window.openMenu(16));
    await page.waitForFunction(() => window.completed.includes('tray_menu_painted:16'));
    await page.keyboard.press('Escape');
    assert.ok(await page.evaluate(() => window.calls.some(c => c.command === 'dismiss_tray_menu' && c.args.generation === 16)));
    // A failed local font must release the paint gate and retain a usable menu.
    let failedFontRequests = 0;
    await page.route('**/*.woff2*', route => { failedFontRequests++; return route.abort(); });
    await page.reload();
    await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'menu');
    await page.keyboard.press('ArrowDown');
    assert.equal(await page.locator(':focus').innerText(),'划词图标');
    fonts.failedLoadFallback = await renderedFonts(page,'.tray-menu-settings > span');
    assert(failedFontRequests > 0);
    assert(fonts.failedLoadFallback.some(font => !font.isCustomFont && font.glyphCount > 0));
    await page.unroute('**/*.woff2*');
    await page.reload();
    await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'menu');
    fonts.recovered = await assertFont(page,'.tray-menu-settings > span','TranslayDisplay');
    fs.writeFileSync('tests/artifacts/font-design-menu.json',JSON.stringify(fonts,null,2));
    assert.deepEqual(errors, []);
    console.log('PASS: tray menu layout, light/dark, actual local fonts, font-load failure fallback and recovery, keyboard, disabled state, four action mappings, duplicate clicks, stale events, padding dismissal, delayed paint focus isolation, old action/new busy isolation, IPC failure and reopen. Mock IPC only; no native focus or tray-panel evidence.');
  } finally { if (browser) await browser.close(); await server.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });


