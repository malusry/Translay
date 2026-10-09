const assert = require('node:assert/strict');
const { chromium } = require('playwright');
const { assertFont, renderedFonts } = require('./font-evidence.cjs');
const fs = require('node:fs');

function settingsMock() {
  const config = {
    backend: 'local', mode: 'conversational', reasoningEnabled: false,
    local: {baseUrl:'http://127.0.0.1:11434/v1',model:''},
    api: {baseUrl:'https://example.invalid/v1',model:''},
    localModels:{},apiModels:{},timeoutSeconds:60,hasApiKey:false,apiKeyHint:null,
  };
  const probe = window.__settingsModeProbe = { calls: [], failNext: false, deferNext: false, finishPending: null };
  const saveProbe = window.__settingsSaveProbe = { calls: [], failNext: false, deferNext: false, finishPending: null, connectionCalls: 0 };
  const connectionProbe = window.__settingsConnectionProbe = { calls: [], pending: new Map(), completed: [], hidden: false };
  const callbacks = new Map(), listeners = new Map();
  let callbackId = 0, listenerId = 0;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_event, id) => listeners.delete(id) };
  window.settingsMockListenerCount = () => listeners.size;
  window.finishConnection = (index, reply) => {
    const pending = connectionProbe.pending.get(index);
    if (!pending) throw new Error(`No pending connection: ${index}`);
    connectionProbe.pending.delete(index);
    connectionProbe.completed.push(index);
    reply.error === undefined ? pending.resolve(reply) : pending.reject(reply.error);
  };
  window.changeActiveModel = (changes) => {
    Object.assign(config, changes);
    for (const listener of listeners.values()) if (listener.event === 'model-backend-changed') callbacks.get(listener.handler)?.({payload:structuredClone(config)});
  };
  window.__TAURI_INTERNALS__ = {
    transformCallback: fn => { callbacks.set(++callbackId, fn); return callbackId; }, unregisterCallback: id => callbacks.delete(id),
    invoke: async (cmd, args) => {
      if (cmd === 'plugin:event|listen') { listeners.set(++listenerId, args); return listenerId; }
      if (cmd === 'plugin:event|unlisten') { listeners.delete(args.eventId); return; }
      if (cmd === 'get_model_config') return structuredClone(config);
      if (cmd === 'get_model_api_key_status') return {hasApiKey:false,apiKeyHint:null};
      if (cmd === 'activate_model_backend') { config.backend = args.backend; return structuredClone(config); }
      if (cmd === 'hide_settings_window') { connectionProbe.hidden = true; return; }
      if (cmd === 'test_model_connection') {
        saveProbe.connectionCalls++;
        const index = connectionProbe.calls.push(structuredClone(args.input)) - 1;
        return new Promise((resolve, reject) => connectionProbe.pending.set(index, {resolve, reject}));
      }
      if (cmd === 'save_model_provider_config') {
        saveProbe.calls.push(structuredClone(args.input));
        if (saveProbe.deferNext) {
          saveProbe.deferNext = false;
          await new Promise(resolve => { saveProbe.finishPending = resolve; });
          saveProbe.finishPending = null;
        }
        if (saveProbe.failNext) {
          saveProbe.failNext = false;
          throw new Error('Isolated provider-save failure');
        }
        const input = args.input;
        config[input.backend] = {...input[input.backend]};
        config.timeoutSeconds = input.timeoutSeconds;
        return structuredClone(config);
      }
      if (cmd === 'save_translation_preferences') {
        probe.calls.push({mode:args.mode,reasoningEnabled:args.reasoningEnabled});
        if (probe.deferNext) {
          probe.deferNext = false;
          await new Promise(resolve => { probe.finishPending = resolve; });
          probe.finishPending = null;
        }
        if (probe.failNext) {
          probe.failNext = false;
          throw new Error('Isolated mode-save failure');
        }
        Object.assign(config, args);
      }
      return null;
    },
  };
}

async function connectionIsolationEvidence(page) {
  const checks = [];
  const check = (name, actual, expected) => {
    checks.push({name,actual,expected,passed:JSON.stringify(actual) === JSON.stringify(expected)});
    assert.deepEqual(actual, expected, name);
  };
  const reset = async () => {
    await page.reload();
    await page.waitForFunction(() => document.querySelector('.provider-test-button')?.disabled === false);
  };
  const snapshot = () => page.evaluate(() => ({
    kind:document.querySelector('.provider-test-button').className.split(' ').at(-1),
    message:document.querySelector('.connection-feedback')?.textContent || '',
    active:document.querySelector('.active-model-status').className.split(' ').at(-1),
    disabled:document.querySelector('.provider-test-button').disabled,
  }));
  const finish = (index, reply) => page.evaluate(async ({index,reply}) => {
    window.finishConnection(index, reply);
    await new Promise(resolve => requestAnimationFrame(resolve));
  }, {index,reply});
  const button = page.locator('.provider-test-button');
  await reset();
  await button.evaluate(button => { for (let index=0;index<12;index++) button.click(); });
  const duplicates = await page.evaluate(() => window.__settingsConnectionProbe.calls.length);
  check('rapid repeated clicks start one request', duplicates, 1);
  check('pending request drives working state', (await snapshot()).kind, 'working');
  for (let index=0;index<duplicates;index++) await finish(index,{success:true,message:'virtual response',elapsedMs:111});
  check('success retains elapsed time', (await snapshot()).message, '连接成功 · 111 ms');
  await button.press('Enter');
  await finish(duplicates,{success:false,message:'服务返回 HTTP 401：虚拟 API Key 已失效，请检查服务端权限。',elapsedMs:15});
  check('failure permits retry', (await snapshot()).disabled, false);
  await button.press('Space');
  await finish(duplicates+1,{success:true,message:'virtual response',elapsedMs:222});
  check('keyboard retry after failure', (await snapshot()).kind, 'success');

  for (const [selector,value,label] of [['#model-name','virtual-model-b','model'],['#base-url','http://virtual.invalid/v1','address'],['#timeout-seconds','90','timeout']]) {
    await reset(); await button.click(); await page.locator(selector).fill(value);
    await finish(0,{success:false,message:'迟到的旧配置错误',elapsedMs:900});
    check(`${label} edit rejects old feedback`, (await snapshot()).kind, 'idle');
  }
  await reset(); await button.click();
  await page.locator('#model-name').fill('virtual-model-b'); await button.click();
  await finish(1,{success:true,message:'new response',elapsedMs:222});
  await finish(0,{error:'迟到的旧请求异常'});
  check('older rejection cannot replace newer result', (await snapshot()).message, '连接成功 · 222 ms');

  await reset(); await button.click();
  await page.locator('#model-name').fill('virtual-model-b'); await button.click();
  await finish(0,{success:true,message:'old response',elapsedMs:900});
  check('obsolete completion leaves new request working', (await snapshot()).kind, 'working');
  await button.evaluate(button => { for (let index=0;index<12;index++) button.click(); });
  check('obsolete finally cannot release new request guard', await page.evaluate(() => window.__settingsConnectionProbe.calls.length), 2);
  await finish(1,{success:false,message:'新请求自己的失败原因',elapsedMs:15});
  check('new request failure remains readable', (await snapshot()).message, '连接失败 · 新请求自己的失败原因');

  await reset(); await button.click();
  await page.locator('#model-name').fill('virtual-model-b'); await button.click();
  await finish(1,{error:'新请求自己的异常'});
  await finish(0,{success:true,message:'old response',elapsedMs:900});
  check('older success cannot replace newer failure', (await snapshot()).message, '连接失败 · 新请求自己的异常');

  await reset(); await button.click();
  await page.locator('#model-name').fill('virtual-model-b'); await page.locator('#model-name').fill('');
  await finish(0,{success:true,message:'old response',elapsedMs:900});
  check('edit then restore still invalidates old request', (await snapshot()).kind, 'idle');

  await reset(); await button.click();
  await page.getByRole('button',{name:'切换到在线 API',exact:true}).click();
  await page.waitForFunction(() => document.querySelector('.source-tabs').classList.contains('api') && !document.querySelector('.source-tabs').classList.contains('switching'));
  await finish(0,{success:false,message:'本地模型的迟到错误',elapsedMs:900});
  check('backend switch rejects old feedback', (await snapshot()).kind, 'idle');
  check('local result cannot change API health', (await snapshot()).active, 'idle');

  await reset(); await button.click();
  await page.locator('.provider-lmstudio').click();
  await finish(0,{success:false,message:'旧提供商错误',elapsedMs:900});
  check('provider preset switch rejects old feedback', (await snapshot()).kind, 'idle');

  await reset(); await button.click();
  await page.evaluate(() => window.changeActiveModel({backend:'api'}));
  await finish(0,{success:false,message:'旧活动后端错误',elapsedMs:900});
  check('external backend event isolates active health', (await snapshot()).active, 'idle');
  check('external backend event clears connection feedback', (await snapshot()).kind, 'idle');

  await reset(); await button.click();
  await page.evaluate(() => window.changeActiveModel({local:{baseUrl:'http://127.0.0.1:11434/v1',model:'virtual-active-model-b'}}));
  await finish(0,{success:true,message:'old model response',elapsedMs:900});
  check('external model change isolates model health', (await snapshot()).active, 'idle');
  check('external model change rejects old draft result', (await snapshot()).kind, 'idle');

  await reset(); await page.locator('#model-name').fill('virtual-draft-model'); await button.click();
  await finish(0,{success:true,message:'draft response',elapsedMs:111});
  check('unsaved draft test cannot update another active model', (await snapshot()).active, 'idle');
  await page.locator('.provider-save-button').click();
  await page.waitForFunction(() => document.querySelector('.provider-save-button').classList.contains('confirmed'));
  check('saving the tested draft transfers its matching health', (await snapshot()).active, 'success');

  for (const [selector,label] of [['.study-mode','mode'],['.switch-button','reasoning']]) {
    await reset(); await button.click();
    await page.getByRole('button',{name:'翻译',exact:true}).click(); await page.locator(selector).click();
    await page.getByRole('button',{name:'模型',exact:true}).click();
    await finish(0,{success:true,message:'old preferences response',elapsedMs:900});
    check(`${label} change rejects old feedback`, (await snapshot()).kind, 'idle');
  }

  await reset(); await page.getByRole('button',{name:'切换到在线 API',exact:true}).click();
  await page.locator('#api-key').fill('virtual-key-a'); await button.click();
  await page.locator('#api-key').fill('virtual-key-b');
  await finish(0,{success:true,message:'old credential response',elapsedMs:900});
  check('new key to new key rejects same-signature result', (await snapshot()).kind, 'idle');

  await reset(); await page.getByRole('button',{name:'切换到在线 API',exact:true}).click();
  await page.locator('#api-key').fill('virtual-key-a'); await button.click();
  await page.locator('.provider-deepseek').click();
  await finish(0,{success:true,message:'old provider response',elapsedMs:900});
  check('API provider switch rejects old feedback', (await snapshot()).kind, 'idle');

  await reset(); await page.getByRole('button',{name:'切换到在线 API',exact:true}).click();
  await page.locator('#api-key').fill('virtual-key-a'); await button.click();
  await page.getByRole('button',{name:'关闭',exact:true}).click();
  await finish(0,{success:true,message:'old key response',elapsedMs:900});
  check('hide clearing an unsaved key invalidates its test', (await snapshot()).kind, 'idle');

  await reset(); await button.click(); await page.keyboard.press('Escape');
  await finish(0,{success:false,message:'保留视图自己的失败原因',elapsedMs:15});
  check('Escape hide retains unchanged-input failure', (await snapshot()).message, '连接失败 · 保留视图自己的失败原因');

  await reset(); await button.click(); await page.getByRole('button',{name:'最小化',exact:true}).click();
  await page.getByRole('button',{name:'翻译',exact:true}).click();
  await finish(0,{success:true,message:'retained response',elapsedMs:444});
  await page.getByRole('button',{name:'模型',exact:true}).click();
  check('minimize and section change retain an unchanged-input test', (await snapshot()).message, '连接成功 · 444 ms');

  await reset(); await button.click(); await page.getByRole('button',{name:'关闭',exact:true}).click();
  check('close uses existing hide command', await page.evaluate(() => window.__settingsConnectionProbe.hidden), true);
  await finish(0,{success:true,message:'hidden response',elapsedMs:333});
  await page.evaluate(() => { window.__settingsConnectionProbe.hidden = false; });
  check('hidden retained view keeps unchanged-input result on reopen', (await snapshot()).message, '连接成功 · 333 ms');
  return {checks,scope:'Real Settings handlers, controlled mock IPC and virtual inputs. Hide/reopen here models retained React state, not native window visibility. No real models or credentials.'};
}

async function connectionVisualEvidence(page, capture = false) {
  await page.getByRole('button',{name:'模型',exact:true}).click();
  const button = page.locator('.provider-test-button');
  const settle = () => button.evaluate(async button => {
    await new Promise(resolve => requestAnimationFrame(resolve));
    await Promise.all(button.getAnimations({subtree:true}).filter(animation => animation instanceof CSSTransition).map(animation => animation.finished.catch(() => {})));
  });
  const measure = () => button.evaluate(button => {
    const bounds = button.getBoundingClientRect(), icon = button.querySelector('.connection-status-icon'), iconBounds = icon.getBoundingClientRect();
    const border = button.querySelector('.connection-button-border'), circle = border.querySelector('circle');
    const feedback = document.querySelector('.connection-feedback'), text = feedback.getBoundingClientRect(), css = getComputedStyle(button);
    return {kind:button.className.split(' ').at(-1),disabled:button.disabled,busy:button.getAttribute('aria-busy'),describedBy:button.getAttribute('aria-describedby'),size:[bounds.width,bounds.height],position:[bounds.x,bounds.y],iconSize:[iconBounds.width,iconBounds.height],iconCenterOffset:[iconBounds.x+iconBounds.width/2-bounds.x-bounds.width/2,iconBounds.y+iconBounds.height/2-bounds.y-bounds.height/2],paths:[...icon.querySelectorAll('path')].map(path => path.getAttribute('d')),spinner:icon.querySelector('.connection-test-spinner')?getComputedStyle(icon.querySelector('circle')).animationName:null,radius:css.borderRadius,shadow:css.boxShadow,fill:css.backgroundImage,color:css.color,transition:css.transitionDuration,outline:css.outlineWidth,borderSize:[border.getBoundingClientRect().width,border.getBoundingClientRect().height],borderShape:['cx','cy','r'].map(name => Number(circle.getAttribute(name))),borderThickness:getComputedStyle(circle).strokeWidth,borderOpacity:Number(getComputedStyle(border).opacity),borderTransition:getComputedStyle(border).transitionDuration,borderStops:[...border.querySelectorAll('stop')].map(stop => getComputedStyle(stop).stopColor),decorative:border.getAttribute('aria-hidden'),pointerEvents:getComputedStyle(border).pointerEvents,feedback:{text:feedback.textContent,title:feedback.title,id:feedback.id,role:feedback.getAttribute('role'),height:text.height,width:text.width,ellipsis:getComputedStyle(feedback).textOverflow,truncated:feedback.scrollWidth>feedback.clientWidth,inside:text.right<=bounds.left-7.9},saveBounds:document.querySelector('.provider-save-button').getBoundingClientRect().toJSON(),overflow:document.querySelector('.settings-shell').scrollWidth>document.querySelector('.settings-shell').clientWidth};
  });
  const assertLayout = state => {
    assert.deepEqual(state.size,[32,32]);assert.deepEqual(state.iconSize,[16,16]);assert.equal(state.radius,'50%');assert.equal(state.shadow,'none');
    assert.ok(state.iconCenterOffset.every(offset => Math.abs(offset)<.03));assert.deepEqual(state.borderSize,[32,32]);assert.deepEqual(state.borderShape,[16,16,15.5]);assert.equal(state.borderThickness,'1px');
    assert.equal(state.decorative,'true');assert.equal(state.pointerEvents,'none');assert.equal(state.borderStops[0],'rgb(80, 123, 120)');assert.equal(state.borderStops[2],'rgb(112, 96, 141)');assert.equal(state.borderStops[3],state.borderStops[2]);assert.match(state.fill,/linear-gradient/);
    assert.equal(state.feedback.height,16);assert.ok(state.feedback.width<=240);assert.equal(state.feedback.inside,true);assert.equal(state.overflow,false);assert.deepEqual([state.saveBounds.width,state.saveBounds.height],[74,28]);
    assert.ok(state.saveBounds.left-state.position[0]-32>=7.9);
  };
  await settle();const initial=await measure();assertLayout(initial);assert.equal(initial.disabled,false);
  await button.hover();await settle();const hover=await measure();assertLayout(hover);assert.equal(hover.borderOpacity,1);
  const start = async key => {
    const index = await page.evaluate(() => window.__settingsConnectionProbe.calls.length);
    await button.focus();await button.press(key);
    await page.waitForFunction(() => document.querySelector('.provider-test-button').classList.contains('working'));
    return index;
  };
  const finish = (index,reply) => page.evaluate(({index,reply}) => window.finishConnection(index,reply),{index,reply});
  let index=await start('Space');await settle();const working=await measure();assertLayout(working);assert.equal(working.disabled,true);assert.equal(working.busy,'true');assert.equal(working.feedback.text,'正在连接…');
  await button.evaluate(button => { for(let count=0;count<12;count++) button.click(); });
  assert.equal(await page.evaluate(() => window.__settingsConnectionProbe.calls.length),index+1);
  if(capture && await page.evaluate(() => devicePixelRatio===1)) await page.screenshot({path:'tests/artifacts/settings-connection-working-20261009.png'});
  await finish(index,{success:true,message:'virtual response',elapsedMs:326});
  await page.waitForFunction(() => document.querySelector('.provider-test-button').classList.contains('success'));
  await settle();const success=await measure();assertLayout(success);assert.equal(success.busy,'false');assert.equal(success.disabled,false);assert.equal(success.feedback.text,'连接成功 · 326 ms');
  if(capture){
    await button.evaluate(button => button.blur());await page.mouse.move(660,550);
    const scale=await page.evaluate(() => devicePixelRatio);
    await page.locator('.provider-actions').screenshot({path:`tests/artifacts/settings-connection-actions-${scale}-20261009.png`});
    if(scale===1) await page.screenshot({path:'tests/artifacts/settings-connection-success-20261009.png'});
  }
  index=await start('Enter');
  const detail='服务返回 HTTP 401：虚拟 API Key 已失效，请检查模型访问权限。\nrequest_id=virtual-request; '+ '原始服务端诊断。'.repeat(60);
  await finish(index,{success:false,message:detail,elapsedMs:15});
  await page.waitForFunction(() => document.querySelector('.provider-test-button').classList.contains('error'));
  await settle();const failed=await measure();assertLayout(failed);assert.equal(failed.disabled,false);assert.equal(failed.feedback.truncated,true);assert.equal(failed.feedback.ellipsis,'ellipsis');assert.equal(failed.feedback.title,`连接失败 · ${detail}`);assert.equal(failed.feedback.text,failed.feedback.title);assert.equal(failed.describedBy,failed.feedback.id);assert.equal(failed.feedback.role,'status');assert.notDeepEqual(failed.paths,success.paths);
  await button.focus();const focused=await measure();assert.equal(focused.outline,'2px');
  if(capture && await page.evaluate(() => devicePixelRatio===1)) await page.screenshot({path:'tests/artifacts/settings-connection-error-20261009.png'});
  index=await start('Enter');await finish(index,{error:'ECONNREFUSED'});
  await page.waitForFunction(() => document.querySelector('.connection-feedback')?.textContent.includes('确认服务已启动'));
  const rejected=await measure();assertLayout(rejected);assert.match(rejected.feedback.title,/ECONNREFUSED/);
  await page.emulateMedia({reducedMotion:'reduce'});index=await start('Enter');
  const reduced=await measure();assertLayout(reduced);assert.equal(reduced.spinner,'none');assert.equal(reduced.transition,'0s');assert.equal(reduced.borderTransition,'0s');
  await finish(index,{success:true,message:'retry response',elapsedMs:75});
  await page.waitForFunction(() => document.querySelector('.provider-test-button').classList.contains('success'));
  const retried=await measure();assertLayout(retried);
  for(const state of [hover,working,success,failed,rejected,reduced,retried]) {
    assert.deepEqual(state.position,initial.position,'feedback moved the connection button');
    assert.deepEqual(state.saveBounds,initial.saveBounds,'feedback moved the save button');
  }
  await page.emulateMedia({reducedMotion:'no-preference'});
  return {initial,hover,working,success,failed,focused,rejected,reduced,retried,scope:'Production React/CSS with controlled mock IPC, long diagnostics in native title and accessible description. Device scale is browser emulation; no model or credential writes.'};
}

async function connectionUnmountEvidence(browser) {
  const { createServer } = await import('vite');
  const fixtureId = '\0settings-unmount-fixture';
  const server = await createServer({server:{host:'127.0.0.1',port:0},plugins:[{
    name:'settings-unmount-fixture',
    configureServer(server) {
      server.middlewares.use('/settings-unmount.html', (_request,response) => {
        response.setHeader('Content-Type','text/html');
        response.end('<!doctype html><html><body><div id="root"></div><script type="module" src="/@id/settings-unmount-fixture"></script></body></html>');
      });
    },
    resolveId(id) { if(id === 'settings-unmount-fixture') return fixtureId; },
    load(id) {
      if(id !== fixtureId) return;
      return `import React from 'react';
        import {createRoot} from 'react-dom/client';
        import {Settings} from '/src/settings/Settings.tsx';
        import '/src/shared/base.css';
        let root;
        window.mountSettingsFixture=()=>{root=createRoot(document.getElementById('root'));root.render(React.createElement(React.StrictMode,null,React.createElement(Settings)));};
        window.unmountSettingsFixture=()=>root.unmount();
        window.mountSettingsFixture();`;
    },
  }]});
  let page;
  const errors=[];
  try {
    await server.listen();
    page=await browser.newPage({viewport:{width:680,height:570}});
    page.on('pageerror',error => errors.push(error.message));
    page.on('console',message => { if(message.type() === 'error') errors.push(message.text()); });
    await page.route('**/favicon.ico',route => route.fulfill({status:204}));
    await page.addInitScript(settingsMock);
    await page.goto(server.resolvedUrls.local[0]+'settings-unmount.html');
    const ready=() => page.waitForFunction(() => document.querySelector('.provider-test-button')?.disabled === false && window.settingsMockListenerCount() === 1);
    const unmount=async () => {
      await page.evaluate(() => window.unmountSettingsFixture());
      await page.waitForFunction(() => document.getElementById('root').childElementCount === 0 && window.settingsMockListenerCount() === 0);
    };
    await ready();await page.locator('.provider-test-button').click();await unmount();
    await page.evaluate(() => window.finishConnection(0,{error:'unmounted virtual failure'}));
    assert.equal(await page.locator('#root').evaluate(root => root.childElementCount),0);
    await page.evaluate(() => window.mountSettingsFixture());await ready();
    assert.equal(await page.locator('.connection-feedback').innerText(),'');
    await page.locator('.provider-test-button').click();await unmount();
    await page.evaluate(() => window.mountSettingsFixture());await ready();await page.locator('.provider-test-button').click();
    await page.evaluate(() => window.finishConnection(2,{success:true,message:'new mounted response',elapsedMs:123}));
    await page.waitForFunction(() => document.querySelector('.connection-feedback')?.textContent === '连接成功 · 123 ms');
    await page.evaluate(async () => {window.finishConnection(1,{success:false,message:'old mounted failure',elapsedMs:900});await new Promise(resolve => requestAnimationFrame(resolve));});
    assert.equal(await page.locator('.connection-feedback').innerText(),'连接成功 · 123 ms');
    await unmount();assert.equal(await page.evaluate(() => window.__settingsConnectionProbe.pending.size),0);
    assert.deepEqual(errors,[]);
    return {unmountedReplyIgnored:true,remountStartsIdle:true,oldInstanceCannotOverwriteNew:true,listenerCleanup:true,pendingRequests:0,errors,scope:'Actual React StrictMode mount/unmount via the existing Vite browser harness, source Settings component and mock IPC. Production rendering and native hide/reopen are separate checks.'};
  } catch(error) {
    if(page) console.error('Unmount fixture diagnostics:',JSON.stringify({errors,state:await page.evaluate(() => ({root:document.getElementById('root')?.innerHTML,listeners:window.settingsMockListenerCount?.(),fixtureAvailable:typeof window.mountSettingsFixture}))}));
    throw error;
  } finally {
    if(page) await page.close();
    await server.close();
  }
}

async function providerSaveEvidence(page, capture = false) {
  await page.getByRole('button',{name:'模型',exact:true}).click();
  const button=page.locator('.provider-save-button'),input=page.locator('#model-name');
  await input.waitFor();
  await page.evaluate(()=>{window.__settingsSaveProbe.calls=[];window.__settingsSaveProbe.connectionCalls=0;});
  const settle=()=>button.evaluate(async button=>{
    await new Promise(resolve=>requestAnimationFrame(resolve));
    await Promise.all(button.getAnimations({subtree:true}).filter(animation=>animation instanceof CSSTransition).map(animation=>animation.finished.catch(()=>{})));
  });
  const measure=()=>button.evaluate(button=>{
    const bounds=button.getBoundingClientRect(),label=button.querySelector('.save-button-label'),text=label.getBoundingClientRect(),svg=button.querySelector('.save-status-icon'),icon=svg?.getBoundingClientRect();
    const border=button.querySelector('.save-button-border'),rect=border.querySelector('rect'),edge=getComputedStyle(border),outline=getComputedStyle(rect);
    const style=getComputedStyle(button),labelStyle=getComputedStyle(label);
    const previous=button.previousElementSibling.getBoundingClientRect();
    return {className:button.className,label:label.textContent,disabled:button.disabled,busy:button.getAttribute('aria-busy'),size:[bounds.width,bounds.height],fontSize:labelStyle.fontSize,fontWeight:labelStyle.fontWeight,labelFamily:labelStyle.fontFamily,labelSpacing:labelStyle.letterSpacing,labelColor:labelStyle.color,iconColor:svg?getComputedStyle(svg).color:null,iconSize:icon?[icon.width,icon.height]:null,centerDifference:icon?Math.abs(icon.y+icon.height/2-text.y-text.height/2):null,contentInside:text.left>=bounds.left+1&&text.right<=bounds.right-1&&text.top>=bounds.top&&text.bottom<=bounds.bottom&&(!icon||(icon.left>=bounds.left+1&&icon.right<=bounds.right-1)),gapFromTestButton:bounds.left-previous.right,opacity:style.opacity,borderThickness:outline.strokeWidth,borderShape:['x','y','width','height','rx'].map(name=>Number(rect.getAttribute(name))),borderStroke:outline.stroke,borderFill:outline.fill,borderDecorative:border.getAttribute('aria-hidden'),borderPointerEvents:edge.pointerEvents,borderSize:[border.getBoundingClientRect().width,border.getBoundingClientRect().height],borderStops:[...border.querySelectorAll('stop')].map(stop=>({offset:stop.getAttribute('offset'),color:getComputedStyle(stop).stopColor})),borderOpacity:Number(edge.opacity),fill:style.backgroundImage,fillColor:style.backgroundColor,radius:style.borderRadius,transform:style.transform,focusOutline:style.outlineWidth,transition:style.transitionDuration,borderTransition:edge.transitionDuration,spinnerAnimation:svg?.querySelector('.save-button-spinner')?getComputedStyle(svg.querySelector('.save-button-spinner')).animationName:null,actionPadding:getComputedStyle(button.parentElement).paddingTop,overflow:document.querySelector('.settings-shell').scrollWidth>document.querySelector('.settings-shell').clientWidth};
  });
  const assertLayout=state=>{
    assert.deepEqual(state.size,[74,28]);assert.equal(state.radius,'7px');assert.equal(state.fontSize,'12px');assert.equal(state.fontWeight,'400');assert.equal(state.contentInside,true);assert.equal(state.overflow,false);assert.equal(state.opacity,'1');assert.equal(state.actionPadding,'12px');assert.ok(state.gapFromTestButton>=7.9);
    assert.equal(state.borderThickness,'1px');assert.deepEqual(state.borderShape,[.5,.5,73,27,6.5]);assert.deepEqual(state.borderSize,state.size);assert.match(state.borderStroke,/settings-save-border-gradient/);assert.equal(state.borderFill,'none');assert.equal(state.borderDecorative,'true');assert.equal(state.borderPointerEvents,'none');assert.deepEqual(state.borderStops.map(stop=>stop.offset),['0','0.42','0.76','1']);assert.equal(state.borderStops[0].color,'rgb(80, 123, 120)');assert.equal(state.borderStops[2].color,'rgb(112, 96, 141)');assert.equal(state.borderStops[3].color,state.borderStops[2].color);
    assert.equal((state.fill.match(/radial-gradient/g)||[]).length,2);assert.match(state.fill,/linear-gradient/);assert.equal(state.fillColor,'rgba(0, 0, 0, 0)');
    if(state.iconSize){assert.deepEqual(state.iconSize,[16,16]);assert.ok(state.centerDifference<.03);}
  };
  const initial=await measure();assertLayout(initial);assert.equal(initial.label,'保存');assert.equal(initial.disabled,true);
  await input.fill('preview-local-a');await settle();
  const dirty=await measure();assertLayout(dirty);assert.equal(dirty.disabled,false);assert.equal(dirty.borderOpacity,.92);
  const normalFonts=await assertFont(page,'.save-button-label','TranslayInterface');
  await button.hover();await settle();const hover=await measure();assertLayout(hover);assert.ok(hover.borderOpacity>dirty.borderOpacity);
  await page.evaluate(()=>{window.__settingsSaveProbe.deferNext=true;});
  await button.focus();
  await page.keyboard.down('Space');const pressed=await measure();assertLayout(pressed);assert.equal(pressed.focusOutline,'2px');await page.keyboard.up('Space');
  await page.waitForFunction(()=>document.querySelector('.provider-save-button').classList.contains('saving'));
  await settle();const saving=await measure();assertLayout(saving);assert.equal(saving.label,'保存中');assert.equal(saving.disabled,true);assert.equal(saving.busy,'true');
  await button.evaluate(button=>{for(let i=0;i<12;i++)button.click();});
  assert.equal(await page.evaluate(()=>window.__settingsSaveProbe.calls.length),1,'pending save accepted duplicate clicks');
  if(capture && await page.evaluate(()=>devicePixelRatio===1))await page.screenshot({path:'tests/artifacts/settings-save-refine-saving-20261009.png'});
  await page.evaluate(()=>window.__settingsSaveProbe.finishPending());
  await page.waitForFunction(()=>document.querySelector('.provider-save-button').classList.contains('confirmed'));
  await settle();const confirmed=await measure();assertLayout(confirmed);assert.equal(confirmed.label,'已保存');assert.equal(confirmed.disabled,true);assert.equal(confirmed.busy,'false');assert.equal(confirmed.labelSpacing,'0.36px');assert.equal(confirmed.labelColor,'rgb(83, 110, 118)');assert.equal(confirmed.iconColor,confirmed.labelColor);
  const confirmedFonts=await renderedFonts(page,'.save-button-label');
  assert.equal(confirmedFonts.filter(font=>font.isCustomFont&&font.familyName==='TranslayReading').reduce((sum,font)=>sum+font.glyphCount,0),3,'saved label mixed fonts or missed a glyph');
  assert.equal(confirmedFonts.filter(font=>!font.isCustomFont).length,0);
  if(capture){
    await button.evaluate(button=>button.blur());await page.mouse.move(660,550);
    const scale=await page.evaluate(()=>devicePixelRatio);
    await button.screenshot({path:`tests/artifacts/settings-save-refine-corner-${scale}-20261009.png`});
    if(scale===1)await page.screenshot({path:'tests/artifacts/settings-save-refine-confirmed-20261009.png'});
  }
  await input.fill('preview-local-b');await settle();const editedDuringConfirmation=await measure();assertLayout(editedDuringConfirmation);assert.equal(editedDuringConfirmation.disabled,false);assert.equal(await input.inputValue(),'preview-local-b');
  // Existing product behavior keeps the success hint until its 1500ms timer ends,
  // even when the user starts a new draft. Styling must not change that lifecycle.
  await page.waitForFunction(()=>document.querySelector('.provider-save-button').classList.contains('dirty'));
  await settle();const edited=await measure();assertLayout(edited);assert.equal(edited.label,'保存');assert.equal(edited.disabled,false);assert.match(edited.labelFamily,/TranslayInterface/);
  await page.evaluate(()=>{window.__settingsSaveProbe.failNext=true;});await button.press('Enter');
  await page.waitForFunction(()=>document.querySelector('.settings-status.error')?.textContent.includes('Isolated provider-save failure'));
  await settle();const failed=await measure();assertLayout(failed);assert.equal(failed.label,'保存');assert.equal(failed.disabled,false);assert.equal(await input.inputValue(),'preview-local-b');
  await button.press('Enter');await page.waitForFunction(()=>document.querySelector('.provider-save-button').classList.contains('confirmed'));await settle();const retried=await measure();assertLayout(retried);assert.equal(retried.label,'已保存');
  await input.fill('preview-local-c');await page.emulateMedia({reducedMotion:'reduce'});
  await page.evaluate(()=>{window.__settingsSaveProbe.deferNext=true;});await button.press('Enter');
  await page.waitForFunction(()=>document.querySelector('.provider-save-button').classList.contains('saving'));
  const reduced=await measure();assertLayout(reduced);assert.equal(reduced.spinnerAnimation,'none');assert.equal(reduced.transition,'0s');assert.equal(reduced.borderTransition,'0s');
  await page.evaluate(()=>window.__settingsSaveProbe.finishPending());await page.waitForFunction(()=>document.querySelector('.provider-save-button').classList.contains('confirmed'));
  await page.waitForFunction(()=>document.querySelector('.provider-save-button').classList.contains('idle'));
  const expired=await measure();assertLayout(expired);assert.equal(expired.label,'保存');assert.equal(expired.disabled,true);assert.match(expired.labelFamily,/TranslayInterface/);
  const calls=await page.evaluate(()=>window.__settingsSaveProbe.calls.map(input=>({backend:input.backend,model:input[input.backend].model,apiKey:input.apiKey})));
  assert.deepEqual(calls.map(call=>call.model),['preview-local-a','preview-local-b','preview-local-b','preview-local-c']);assert.ok(calls.every(call=>call.apiKey===null));assert.equal(await page.evaluate(()=>window.__settingsSaveProbe.connectionCalls),0);
  await page.emulateMedia({reducedMotion:'no-preference'});
  return {initial,dirty,normalFonts,hover,pressed,saving,confirmed,confirmedFonts,editedDuringConfirmation,edited,failed,retried,reduced,expired,calls,scope:'Real React save handler with simulated IPC and virtual models; no model connection or actual settings/credentials writes. Existing 1500ms confirmation persists while starting a new draft, then restores the ordinary save label.'};
}

async function modeMotionEvidence(page) {
  const choices = page.locator('.style-options');
  const evidence = await choices.evaluate(async root => {
    await document.fonts.ready;
    const buttons = [...root.querySelectorAll('button')];
    const measure = button => {
      const marker = button.querySelector('.choice-indicator').getBoundingClientRect();
      const dot = button.querySelector('.choice-dot').getBoundingClientRect();
      const heading = button.querySelector('.mode-choice-heading').getBoundingClientRect();
      const title = button.querySelector('strong').getBoundingClientRect();
      const description = button.querySelector('small').getBoundingClientRect();
      const wash = button.querySelector('.mode-choice-wash').getBoundingClientRect();
      const style = getComputedStyle(button.querySelector('.mode-choice-wash'));
      const center = rectangle => [rectangle.x + rectangle.width / 2, rectangle.y + rectangle.height / 2];
      const [mx, my] = center(marker), [dx, dy] = center(dot), [wx, wy] = center(wash);
      const cardStyle = getComputedStyle(button);
      const card = button.getBoundingClientRect();
      return {className:button.className,card:card.toJSON(),text:button.querySelector('.mode-choice-copy').getBoundingClientRect().toJSON(),heading:heading.toJSON(),title:title.toJSON(),description:description.toJSON(),titleCenterOffset:title.y+title.height/2-my,descriptionLeftOffset:description.x-title.x,descriptionGap:description.y-heading.bottom,dotOffset:[dx-mx,dy-my],washCenterError:Math.hypot(wx-mx,wy-my),washSize:[wash.width,wash.height],opacity:Number(style.opacity),borderRadius:cardStyle.borderRadius,overflow:cardStyle.overflow,transform:cardStyle.transform};
    };
    const initialAnimations = root.getAnimations({subtree:true}).length;
    if (buttons[0].getAttribute('aria-pressed') !== 'true') {
      buttons[0].click();
      await new Promise(resolve => requestAnimationFrame(resolve));
      await Promise.all(root.getAnimations({subtree:true}).map(animation => animation.finished.catch(() => {})));
      await new Promise(resolve => requestAnimationFrame(resolve));
    }
    const initial = buttons.map(measure);
    buttons[1].click();
    await new Promise(resolve => requestAnimationFrame(resolve));
    const animations = root.getAnimations({subtree:true});
    animations.forEach(animation => animation.pause());
    const frames = [0,60,180,300].map(time => {
      animations.forEach(animation => { animation.currentTime = time; });
      return {time,markers:buttons.map(measure)};
    });
    animations.forEach(animation => animation.finish());
    const settled = buttons.map(measure);
    buttons[1].click();
    await new Promise(resolve => requestAnimationFrame(resolve));
    const sameSelectionAnimations = root.getAnimations({subtree:true}).filter(animation => animation.playState !== 'finished').length;
    // Reverse a partially completed transition through the real React handler.
    buttons[0].click();
    await new Promise(resolve => requestAnimationFrame(resolve));
    root.getAnimations({subtree:true}).forEach(animation => { animation.pause(); animation.currentTime = 70; });
    const beforeReverse = buttons.map(measure);
    buttons[1].click();
    await new Promise(resolve => requestAnimationFrame(resolve));
    const reversedAnimations = root.getAnimations({subtree:true});
    reversedAnimations.forEach(animation => { animation.pause(); animation.currentTime = 0; });
    const afterReverse = buttons.map(measure);
    reversedAnimations.forEach(animation => animation.finish());
    return {initial,initialAnimations,frames,settled,sameSelectionAnimations,beforeReverse,afterReverse};
  });
  assert.equal(evidence.initialAnimations,0,'opening Translation must not replay the selected animation');
  assert.equal(evidence.sameSelectionAnimations,0,'same selection must not replay the animation');
  const snapshots = [evidence.initial,...evidence.frames.map(frame => frame.markers),evidence.settled,evidence.beforeReverse,evidence.afterReverse];
  for (const markers of snapshots) {
    markers.forEach((marker,index) => {
      assert.deepEqual(marker.card,evidence.initial[index].card,'card moved during selection');
      assert.deepEqual(marker.text,evidence.initial[index].text,'text moved during selection');
      assert.deepEqual(marker.heading,evidence.initial[index].heading,'heading moved during selection');
      assert.deepEqual(marker.description,evidence.initial[index].description,'description moved during selection');
      assert.equal(marker.transform,'none');
      assert.equal(marker.borderRadius,'12px');assert.equal(marker.overflow,'hidden');
      assert.ok(marker.dotOffset.every(offset => Math.abs(offset)<.03),'dot and ring are not concentric');
      assert.ok(Math.abs(marker.titleCenterOffset)<.03,'mode title and ring do not share a center line');
      assert.ok(Math.abs(marker.descriptionLeftOffset)<.03,'description is not aligned below the title');
      assert.ok(Math.abs(marker.descriptionGap-5)<.03,'description spacing changed');
      assert.ok(marker.washCenterError < .06,'wash did not start at the ring center');
      assert.ok(Math.abs(marker.washSize[0]-marker.washSize[1]) < .03,'wash is not circular');
    });
    assert.ok(Math.abs(markers[0].title.y-markers[1].title.y)<.03,'daily and study headings are not level');
  }
  const incoming = evidence.frames.map(frame => frame.markers[1]);
  for (let index=1;index<incoming.length;index++) {
    assert.ok(incoming[index].washSize[0] > incoming[index-1].washSize[0],'wash did not expand');
    assert.ok(incoming[index].opacity > incoming[index-1].opacity,'tint did not deepen');
  }
  assert.ok(Math.abs(incoming.at(-1).opacity-.65)<.001);
  assert.ok(incoming[1].washSize[0] < incoming.at(-1).washSize[0]/2,'wash instantly covered the card');
  evidence.afterReverse.forEach((marker,index) => {
    assert.ok(Math.abs(marker.washSize[0]-evidence.beforeReverse[index].washSize[0]) < .1,'reversal jumped to a new size');
    assert.ok(Math.abs(marker.opacity-evidence.beforeReverse[index].opacity) < .002,'reversal flashed');
  });
  return evidence;
}

async function modeSaveEvidence(page) {
  const daily = page.locator('.daily-mode'), study = page.locator('.study-mode');
  await page.evaluate(() => { window.__settingsModeProbe.deferNext = true; });
  await daily.click();
  await page.waitForFunction(() => window.__settingsModeProbe.finishPending !== null);
  assert.equal(await daily.getAttribute('aria-pressed'),'true');
  assert.equal(await study.isDisabled(),true);
  const callsWhileSaving = await page.evaluate(() => window.__settingsModeProbe.calls.length);
  await study.evaluate(button => { for (let index=0;index<12;index++) button.click(); });
  assert.equal(await page.evaluate(() => window.__settingsModeProbe.calls.length),callsWhileSaving,'duplicate save while pending');
  await page.evaluate(() => window.__settingsModeProbe.finishPending());
  await page.waitForFunction(() => !document.querySelector('.daily-mode').disabled);
  await page.evaluate(() => { window.__settingsModeProbe.failNext = true; });
  await study.click();
  await page.waitForFunction(() => document.querySelector('.settings-status.error')?.textContent.includes('Isolated mode-save failure'));
  assert.equal(await daily.getAttribute('aria-pressed'),'true','save failure did not restore the previous mode');
  assert.equal(await study.isDisabled(),false,'save failure left controls disabled');
  await study.focus();await study.press('Enter');
  await page.waitForFunction(() => document.querySelector('.study-mode')?.getAttribute('aria-pressed') === 'true' && !document.querySelector('.study-mode').disabled);
  const calls = await page.evaluate(() => window.__settingsModeProbe.calls);
  assert.ok(calls.every(call => call.reasoningEnabled === false));
  await page.emulateMedia({reducedMotion:'reduce'});
  await daily.click();
  const reduced = await page.locator('.style-options').evaluate(root => ({
    durations:[...root.querySelectorAll('button,.choice-ring,.choice-dot,.mode-choice-wash')].map(element => getComputedStyle(element).transitionDuration),
    animations:root.getAnimations({subtree:true}).length,
  }));
  assert.ok(reduced.durations.every(duration => duration === '0s'));
  assert.equal(reduced.animations,0);
  await page.emulateMedia({reducedMotion:'no-preference'});
  return {calls,pendingDuplicateIgnored:true,failureRestored:true,retryWithKeyboardPassed:true,reducedMotion:reduced};
}

(async () => {
  const { preview } = await import('vite');
  const server = await preview({ preview: { host: '127.0.0.1', port: 0 } });
  let browser;
  try {
    browser = await chromium.launch({ headless: true, channel: 'msedge' });
    const page = await browser.newPage({ viewport: { width: 960, height: 720 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
    await page.route('**/*', async route => {
      if (new URL(route.request().url()).pathname === '/favicon.ico') {
        await route.fulfill({status:204});
      } else if (route.request().resourceType() === 'document') {
        const response = await route.fetch();
        await route.fulfill({response,headers:{...response.headers(),'content-security-policy':"default-src 'self'; style-src 'self' 'unsafe-inline'"}});
      } else {
        await route.continue();
      }
    });
    await page.addInitScript(settingsMock);
    await page.goto(server.resolvedUrls.local[0] + '?view=settings');
    await page.locator('.brand-wordmark').waitFor();
    if(process.env.QA_SETTINGS_CHECK === 'unmount') {
      const evidence=await connectionUnmountEvidence(browser);
      fs.writeFileSync('tests/artifacts/settings-connection-unmount-20261009.json',JSON.stringify(evidence,null,2));
      console.log('PASS: isolated actual Settings StrictMode unmount/remount, old completions and listener cleanup.');
      return;
    }
    if(process.env.QA_SETTINGS_CHECK === 'connection') {
      const evidence=await connectionIsolationEvidence(page);
      console.log(JSON.stringify(evidence,null,2));
      return;
    }
    await page.waitForFunction(() => {
      const img = document.querySelector('.brand-wordmark');
      return img?.complete && img.naturalWidth > 0;
    });
    const info = await page.locator('.brand-wordmark').evaluate(img => ({src:img.src,width:img.clientWidth,height:img.clientHeight}));
    assert.match(info.src, /\/assets\/wordmark-.*\.svg$/);
    assert.equal(info.width,96);assert.equal(info.height,26);
    await page.getByRole('button', {name:'翻译',exact:true}).click();
    const fonts = {
      title: await assertFont(page, '.settings-header h1', 'TranslayDisplay'),
      navigation: await assertFont(page, '.translation-nav', 'TranslayDisplay'),
      daily: await assertFont(page, '.daily-mode strong', 'TranslayDisplay'),
      study: await assertFont(page, '.study-mode strong', 'TranslayReading'),
      guide: await assertFont(page, '.settings-heading.translation + p', 'TranslayReading'),
      studyNote: await assertFont(page, '.study-mode small', 'TranslayReading'),
      dailyNote: await assertFont(page, '.daily-mode small', 'TranslayInterface'),
      section: await assertFont(page, '#translation-mode-label', 'TranslayInterface'),
    };
    const layouts = [];
    for (const colorScheme of ['light','dark']) {
      await page.emulateMedia({ colorScheme });
      for (const size of [{width:680,height:570},{width:960,height:720}]) {
        await page.setViewportSize(size);
        const layout = await page.locator('.settings-shell').evaluate(shell => ({
          overflow: shell.scrollWidth > shell.clientWidth,
          notes: [...shell.querySelectorAll('.style-options small')].map(el => {
            const r = el.getBoundingClientRect(), parent = el.closest('button').getBoundingClientRect();
            return {fontSize:getComputedStyle(el).fontSize,inside:r.left >= parent.left && r.right <= parent.right && r.bottom <= parent.bottom};
          }),
        }));
        assert.equal(layout.overflow,false);
        layout.notes.forEach(note => assert(note.inside,'mode note escapes its existing card'));
        assert.equal(layout.notes[1].fontSize,'12px');
        layouts.push({colorScheme,...size,...layout});
      }
    }
    await page.emulateMedia({colorScheme:'light'});
    const modeMotion = [{colorScheme:'light',scale:1,width:960,height:720,evidence:await modeMotionEvidence(page)}];
    const modeSave = await modeSaveEvidence(page);
    await page.setViewportSize({width:680,height:570});
    await page.mouse.move(660,550);
    await page.screenshot({path:'tests/artifacts/settings-mode-alignment-translation-20261009.png'});
    await page.setViewportSize({width:960,height:720});
    const connectionIsolation = await connectionIsolationEvidence(page);
    const providerSave=[{colorScheme:'light',scale:1,width:960,height:720,evidence:await providerSaveEvidence(page)}];
    const connectionVisual=[{colorScheme:'light',scale:1,width:960,height:720,evidence:await connectionVisualEvidence(page)}];
    for (const colorScheme of ['light','dark']) for (const scale of [1,1.25,1.5,2]) {
      const scaledPage = await browser.newPage({viewport:{width:680,height:570},deviceScaleFactor:scale,colorScheme});
      scaledPage.on('pageerror', error => errors.push(error.message));
      await scaledPage.addInitScript(settingsMock);
      try {
        await scaledPage.goto(server.resolvedUrls.local[0]+'?view=settings');
        for (const size of [{width:680,height:570},{width:960,height:720}]) {
          if(colorScheme==='light'&&scale===1&&size.width===960) continue;
          await scaledPage.setViewportSize(size);
          await scaledPage.getByRole('button',{name:'模型',exact:true}).click();
          await scaledPage.getByRole('button',{name:'翻译',exact:true}).click();
          modeMotion.push({colorScheme,scale,...size,evidence:await modeMotionEvidence(scaledPage)});
          providerSave.push({colorScheme,scale,...size,evidence:await providerSaveEvidence(scaledPage,colorScheme==='light'&&size.width===680)});
          connectionVisual.push({colorScheme,scale,...size,evidence:await connectionVisualEvidence(scaledPage,colorScheme==='light'&&size.width===680)});
        }
      } finally { await scaledPage.close(); }
    }
    await page.getByRole('button', {name:'模型',exact:true}).click();
    await page.locator('#model-name').waitFor();
    fonts.modelTitle = await assertFont(page, '.settings-header h1', 'TranslayDisplay');
    // Arbitrary model names retain a system glyph fallback beyond the UI subset.
    await page.evaluate(() => {
      const sample = document.createElement('span');
      sample.id = 'font-fallback-sample'; sample.textContent = '雲';
      document.querySelector('.settings-title').append(sample);
    });
    await page.locator('#font-fallback-sample').evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
    fonts.fallback = await renderedFonts(page, '#font-fallback-sample');
    assert(fonts.fallback.some(font => !font.isCustomFont && font.glyphCount > 0));
    await page.locator('#font-fallback-sample').evaluate(el => el.remove());
    await page.setViewportSize({width:680,height:570});
    const captureSettings = async path => {
      await page.mouse.move(660,550);
      await page.locator('.settings-nav').evaluate(async nav => {
        await new Promise(resolve => requestAnimationFrame(resolve));
        await Promise.all(nav.getAnimations({subtree:true}).map(animation => animation.finished.catch(() => {})));
      });
      await page.screenshot({path});
    };
    await captureSettings('tests/artifacts/font-design-settings-model.png');
    await page.getByRole('button', {name:'翻译',exact:true}).click();
    await captureSettings('tests/artifacts/font-design-settings-translation.png');
    const connectionUnmount = await connectionUnmountEvidence(browser);
    assert.deepEqual(errors,[]);
    fs.writeFileSync('tests/artifacts/font-design-settings.json',JSON.stringify({fonts,layouts,errors},null,2));
    fs.writeFileSync('tests/artifacts/settings-mode-alignment-browser-20261009.json',JSON.stringify({source:'Production React and CSS in isolated headless Edge; simulated IPC only; no Windows window or actual settings writes.',modeMotion,modeSave,errors},null,2));
    fs.writeFileSync('tests/artifacts/settings-save-refine-browser-20261009.json',JSON.stringify({source:'Production React, local fonts and CSS under application CSP in isolated Edge. Color-scheme preferences tested; the existing settings page retains its light product palette.',providerSave,errors},null,2));
    fs.writeFileSync('tests/artifacts/settings-connection-browser-20261009.json',JSON.stringify({connectionIsolation,connectionVisual,connectionUnmount,providerSave,modeMotion,modeSave,errors},null,2));
    console.log('PASS: production settings brand and three actual local fonts under app CSP; existing wordmark dimensions; native 680x570 and 960 layouts, 12px study note, model page and arbitrary-glyph fallback. Mock IPC only.');
    console.log('PASS: concentric ring/dot and centered title row with descriptions aligned below titles; circular soft wash with progressive tint and fixed card/text geometry in light/dark at 100/125/150/200% device scales; initial/same-selection no replay; continuous reversal; pending save guard, failure rollback and keyboard retry; reduced motion.');
    console.log('PASS: compact 74x28 save button in idle/dirty/saving/confirmed/failed/retry states; transparent fill and continuous 1px sage-purple SVG outline with a deeper purple corner; actual WenKai glyphs for all three saved characters; centered 16px icon and 12px label; keyboard save, duplicate-click guard, draft retention, original confirmation expiry and reduced motion. Mock IPC only.');
    console.log('PASS: connection request guard, input/key invalidation, both completion orders, obsolete-finally isolation, backend/provider/active-model health isolation, retained hide/reopen and actual StrictMode unmount/remount. Controlled mock IPC only.');
    console.log('PASS: circular 32x32 connection button with centered 16px state icons, continuous sage-purple outline and no white inset/shadow; fixed feedback/button geometry, full long diagnostics in title and accessible description, success timing, failure/rejection retry, keyboard and reduced motion at 100/125/150/200% browser device scale. Save and mode regression retained.');
  } finally {
    if (browser) await browser.close();
    await new Promise(resolve => server.httpServer.close(resolve));
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
