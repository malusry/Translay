const assert=require('node:assert/strict');
const {chromium}=require('playwright');
(async()=>{
 const {preview}=await import('vite');
 const server=await preview({preview:{host:'127.0.0.1',port:5195,strictPort:true}});
 let browser;
 try{
  browser=await chromium.launch({headless:true,channel:'msedge'});
  const page=await browser.newPage({viewport:{width:260,height:120}});
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(()=>{
   let next=1;const callbacks=new Map(),events=new Map();
   window.payload={generation:1,phase:'loading',visible:!new URLSearchParams(location.search).has('silent')};window.finished=[];window.violations=[];
   let hold=new URLSearchParams(location.search).has('hold');
   document.addEventListener('securitypolicyviolation',e=>window.violations.push(e.blockedURI));
   window.repeat=()=>{for(const [event,id] of events)if(event==='startup-repeat')callbacks.get(id)?.({event,payload:null});};
   window.__TAURI_INTERNALS__={transformCallback:f=>{const id=next++;callbacks.set(id,f);return id;},unregisterCallback:id=>callbacks.delete(id),invoke:async(cmd,args={})=>{
    if(cmd==='plugin:event|listen'){events.set(args.event,args.handler);return next++;}
    if(cmd==='startup_status'){
     if(hold)await new Promise(resolve=>{window.releaseStartup=()=>{hold=false;resolve();};});
     return structuredClone(window.payload);
    }
    if(cmd==='finish_startup'){
     if(Number(getComputedStyle(document.querySelector('.startup-backdrop')).opacity)>.01)throw new Error('Backdrop clipped before fade completion');
     const ink=getComputedStyle(document.querySelector('.startup-ink'));
     if(matchMedia('(prefers-reduced-motion: reduce)').matches ? Number(ink.opacity)>.01 : parseFloat(ink.maskPosition)!==-195)throw new Error('Ink hidden before exit completes');
    }
    if(cmd==='finish_startup'){window.finished.push(args.generation);if(args.generation===window.payload.generation)window.payload.visible=false;return;}
   }};

  });
  let releaseMask,maskRequested;
  const maskGate=new Promise(resolve=>releaseMask=resolve);
  const maskSeen=new Promise(resolve=>maskRequested=resolve);
  await page.route('**/*',async route=>{
   if(route.request().url().includes('/ribbon-mask-')){maskRequested();await maskGate;return route.continue();}
   if(!route.request().isNavigationRequest())return route.continue();
   const response=await route.fetch();
   await route.fulfill({response,headers:{...response.headers(),'content-security-policy':"default-src 'self'; style-src 'self' 'unsafe-inline'"}});
  });
  const scripts=[];page.on('request',r=>{if(r.resourceType()==='script')scripts.push(r.url());});
  await page.goto(server.resolvedUrls.local[0]+'startup.html?present=1&hold=1',{waitUntil:'domcontentloaded'});
  await maskSeen;
  assert.equal(await page.locator('.startup-signature.playing').count(),0);
  await page.evaluate(()=>{window.originalStartupMark=document.querySelector('.startup-mark img');});
  releaseMask();
  await page.locator('.startup-signature.playing').waitFor();
  assert(await page.evaluate(()=>window.originalStartupMark===document.querySelector('.startup-mark img')));
  console.log('PASS: first sweep waits for its mask and retains the decoded image element.');
  await page.waitForFunction(()=>typeof window.releaseStartup==='function');
  assert.equal(await page.locator('.startup-label').innerText(),'');
  await page.evaluate(()=>window.releaseStartup());
  console.log('PASS: startup paints while native initialization reply is blocked.');
  // Seek the real CSS animation to verify reveal/light alignment without timing races.
  const reveal=await page.locator('.startup-lettering').evaluate(e=>{
   const a=e.getAnimations().find(a=>a.animationName==='startup-light-front');
   a.pause();a.currentTime=250;
   const initial=parseFloat(getComputedStyle(e).getPropertyValue('--startup-reveal-x'));
   a.currentTime=900;
   const x=parseFloat(getComputedStyle(e).getPropertyValue('--startup-reveal-x'));
   const light=new DOMMatrix(getComputedStyle(e.querySelector('.startup-word-sheen'),'::after').transform).m41;
   a.currentTime=1600;a.play();return{initial,x,light};
  });
  assert.equal(reveal.initial,-18);assert(reveal.x>0&&reveal.x<112);
  assert(Math.abs(reveal.light-(reveal.x-20))<.02);
  assert.equal(await page.locator('.startup-ink .startup-label').count(),0);
  await page.waitForTimeout(2100);
  assert.equal(await page.locator('.startup-label').innerText(),'正在启动');
  assert.deepEqual(await page.evaluate(()=>window.finished),[]);
  const geometry=await page.locator('.startup-signature').evaluate(e=>{const r=e.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};});
  assert.equal(geometry.x,130);assert.equal(geometry.y,60);
  const backdrop=await page.locator('.startup-backdrop').evaluate(e=>{const r=e.getBoundingClientRect();return{width:r.width,height:r.height,x:r.x+r.width/2,y:r.y+r.height/2,radius:getComputedStyle(e).borderRadius};});
  assert.deepEqual(backdrop,{width:204,height:88,x:130,y:66,radius:'44px'});
  await page.evaluate(()=>window.payload.phase='ready');
  await page.getByText('已就绪',{exact:true}).waitFor();
  await page.locator('.startup-signature.leaving').waitFor();
  const exit=await page.locator('.startup-ink').evaluate(e=>{
   const a=e.getAnimations().find(a=>a.animationName==='startup-reverse-erase');
   return a.effect.getTiming();
  });
  assert.equal(exit.duration,850);assert.equal(exit.delay,140);
  await page.waitForFunction(()=>window.finished.includes(1));
  console.log('PASS: slow startup waits for real readiness; fixed visual center; fade completes before finish.');
  // Fast startup still completes the single sweep; switching states never moves the mark.
  await page.evaluate(()=>{window.payload={generation:2,phase:'setup',visible:true};window.repeat();});
  await page.locator('.startup-signature.playing').waitFor();
  await page.waitForTimeout(800);
  assert.equal(await page.locator('.startup-label').innerText(),'');
  await page.getByText('待配置',{exact:true}).waitFor();
  assert.deepEqual(await page.locator('.startup-signature').evaluate(e=>{const r=e.getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2};}),geometry);
  await page.waitForFunction(()=>window.finished.includes(2));
  console.log('PASS: unconfigured startup uses its own status and finishes exactly once.');
  await page.emulateMedia({reducedMotion:'reduce',colorScheme:'dark'});
  await page.evaluate(()=>{window.payload={generation:3,phase:'already',visible:true};window.repeat();window.repeat();});
  await page.getByText('已在运行',{exact:true}).waitFor();
  assert.equal(await page.locator('.startup-sheen').evaluate(e=>getComputedStyle(e).display),'none');
  await page.waitForFunction(()=>window.finished.includes(3));
  assert.deepEqual(await page.evaluate(()=>window.finished),[1,2,3]);
  await page.emulateMedia({reducedMotion:'no-preference'});
  await page.evaluate(()=>{window.payload={generation:4,phase:'already',visible:true};window.repeat();});
  await page.getByText('已在运行',{exact:true}).waitFor();
  assert.equal(await page.locator('.startup-lettering').evaluate(e=>parseFloat(getComputedStyle(e).getPropertyValue('--startup-reveal-x'))),150);
  await page.waitForFunction(()=>window.finished.includes(4));
  assert.deepEqual(await page.evaluate(()=>window.finished),[1,2,3,4]);
  assert(await page.evaluate(()=>window.originalStartupMark===document.querySelector('.startup-mark img')));
  const imageInfo=await page.locator('img').evaluateAll(imgs=>imgs.map(img=>({loaded:img.complete&&img.naturalWidth>0,url:img.src})));
  assert(imageInfo.every(i=>i.loaded&&!i.url.startsWith('data:')));
  assert.deepEqual(await page.evaluate(()=>window.violations),[]);assert.deepEqual(errors,[]);
  const fs=require('node:fs');const path=require('node:path');
  const scriptBytes=scripts.reduce((n,url)=>n+fs.statSync(path.join('dist',new URL(url).pathname)).size,0);
  assert(scriptBytes<20000,`Startup scripts unexpectedly large: ${scriptBytes}`);
  console.log(`PASS: isolated startup scripts total ${scriptBytes} bytes (no React runtime).`);
  await page.goto(server.resolvedUrls.local[0]+'startup.html?silent=1');
  await page.locator('.startup-signature.waiting').waitFor();
  await page.waitForTimeout(500);
  assert.equal(await page.locator('.startup-signature.playing').count(),0);
  assert.deepEqual(await page.evaluate(()=>window.finished),[]);
  console.log('PASS: background startup stays invisible.');
  console.log('PASS: duplicate launch is brief, reduced motion hides sweep, production assets load under app CSP.');
 }finally{if(browser)await browser.close();await new Promise(r=>server.httpServer.close(r));}
})().catch(e=>{console.error(e);process.exitCode=1});
