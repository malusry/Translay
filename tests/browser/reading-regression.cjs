const {chromium}=require('playwright');const assert=require('assert');const fs=require('fs');
(async()=>{const {createServer}=await import('vite');const server=await createServer({server:{host:'127.0.0.1',port:0}});await server.listen();const browser=await chromium.launch({headless:true,channel:'msedge'});try{
 const page=await browser.newPage({viewport:{width:560,height:220},deviceScaleFactor:Number(process.env.QA_DPR||1),colorScheme:process.env.QA_SCHEME||'light'});
 await page.exposeFunction('nativeFit',async height=>{await page.setViewportSize({width:page.viewportSize().width,height:Math.max(94,Math.min(648,height))});return true});
 await page.addInitScript(()=>{
  const callbacks=new Map(),events=new Map();let next=1;
  window.calls=[];window.pending=[];window.retryPending=[];window.holdRetry=false;
  window.payload={requestId:42,phase:'translated',success:true,text:'$$\\operatorname{Attention}(Q,K,V)=\\operatorname{softmax}(\\frac{QK^T}{\\sqrt{d_k}})V$$ (1)\n\n正文使用 $1/\\sqrt{d_k}$ 缩放。引用 [2]。',applicationName:'PDF test',processId:1,captureMethod:'Synthetic IPC',elapsedMs:0,selectionRect:null,errorCode:null,errorMessage:null,focusPreserved:true,clipboardRestored:true,warningCode:null,languageProfile:null,translationMode:'academic'};
  window.emitCapture=()=>{for(const [event,handler] of events){if(event==='capture-result')callbacks.get(handler)?.({event,payload:window.payload})}};
  window.__TAURI_INTERNALS__={transformCallback:fn=>{const id=next++;callbacks.set(id,fn);return id},unregisterCallback:id=>callbacks.delete(id),invoke:async(cmd,args={})=>{
   window.calls.push({cmd,args});
   if(cmd==='get_latest_capture')return window.payload;
   if(cmd==='plugin:event|listen'){events.set(args.event,args.handler);return next++}
   if(cmd==='plugin:event|unlisten')return;
   if(cmd==='fit_overlay_height')return window.nativeFit(args.logicalHeight);
   if(cmd==='explain_translation')return new Promise((resolve,reject)=>window.pending.push({args,resolve,reject}));
   if(cmd==='copy_translation'){window.copied=args.text;return}
   if(cmd==='retry_capture'&&window.holdRetry)return new Promise((resolve,reject)=>window.retryPending.push({args,resolve,reject}));
   return true;
  }};
 });
 await page.goto(server.resolvedUrls.local[0]);
 await page.getByRole('button',{name:'查看详细解释'}).waitFor();
 assert.equal(await page.locator('.translation .katex').count(),2);
 await page.getByRole('button',{name:'复制译文',exact:true}).click();
 assert((await page.evaluate(()=>window.copied)).includes('\\sqrt{d_k}'));
 await page.getByRole('button',{name:'查看详细解释'}).click();
 await page.getByRole('status').waitFor();
 await page.getByRole('button',{name:'关闭详细解释'}).click();
 await page.waitForFunction(()=>window.calls.some(x=>x.cmd==='cancel_explanation'));
 await page.waitForFunction(()=>!document.querySelector('#explanation-panel'));
 await page.getByRole('button',{name:'查看详细解释'}).click();
 await page.waitForFunction(()=>window.pending.length===2);
 await page.evaluate(()=>window.pending[0].resolve({coreExplanation:'OLD RESULT',keyConcepts:[],caveat:''}));
 assert.equal(await page.getByText('OLD RESULT').count(),0);
 await page.evaluate(()=>window.pending[1].resolve({coreExplanation:'CURRENT RESULT',keyConcepts:[],caveat:''}));
 await page.getByText('CURRENT RESULT').waitFor();
 await page.getByRole('button',{name:'关闭详细解释'}).click();
 await page.waitForFunction(()=>!document.querySelector('#explanation-panel'));
 await page.getByRole('button',{name:'查看详细解释'}).click();
 await page.getByText('CURRENT RESULT').waitFor();assert.equal(await page.evaluate(()=>window.pending.length),2);
 // Change capture while an explanation is loading, then deliver its late response.
 await page.evaluate(()=>{window.payload={...window.payload,requestId:43,text:'新的段落包含 $x_i$。'};window.emitCapture()});
 await page.getByText('新的段落包含',{exact:false}).waitFor();
 await page.getByRole('button',{name:'查看详细解释'}).click();await page.waitForFunction(()=>window.pending.length===3);
 await page.evaluate(()=>{window.payload={...window.payload,requestId:44,text:'NEXT DOCUMENT'};window.emitCapture()});
 await page.getByText('NEXT DOCUMENT').waitFor();
 await page.waitForFunction(()=>window.calls.some(x=>x.cmd==='cancel_explanation'&&x.args.requestId===43));
 await page.evaluate(()=>window.pending[2].resolve({coreExplanation:'STALE DOCUMENT',keyConcepts:[],caveat:''}));
 assert.equal(await page.getByText('STALE DOCUMENT').count(),0);
 const data=await page.evaluate(()=>({attempts:window.pending.map(x=>x.args),cancels:window.calls.filter(x=>x.cmd==='cancel_explanation').map(x=>x.args)}));
 assert(data.attempts[1].attemptId>data.attempts[0].attemptId);assert.equal(data.cancels[0].attemptId,data.attempts[0].attemptId);
 console.log('PASS full Overlay: mixed math/copy; close/reopen; old completion; cache reuse; new capture cancellation.',JSON.stringify(data));
 // Long formulas and content in narrow viewports.
 for(const width of [320,560]){
  await page.setViewportSize({width,height:320});
  await page.evaluate(()=>{window.payload={...window.payload,requestId:window.payload.requestId+1,text:'长公式：$'+('\\frac{'+'x'.repeat(100)+'}{z}')+'$。\n'+('正文内容。'.repeat(150))};window.emitCapture()});
  await page.locator('.translation .math-inline .mfrac').waitFor();
  const layout=await page.locator('.translation').evaluate(e=>({client:e.clientWidth,scroll:e.scrollWidth,inlineScroll:e.querySelector('.math-inline').scrollWidth,inlineClient:e.querySelector('.math-inline').clientWidth}));
  assert(layout.scroll<=layout.client+1);assert(layout.inlineScroll>layout.inlineClient);
  const scrolled=await page.locator('.translation .math-inline').evaluate(e=>{e.scrollLeft=100;return e.scrollLeft});assert(scrolled>0);
  console.log('PASS long fraction scrolling',width,JSON.stringify(layout));
 }
 // Daily tone notes: real component, copy isolation, bounded height, no-note compact fallback.
 for (const sample of [
  {text:'有道理。',toneNote:'口语中表示认可对方的判断。'},
  {text:'我不太确定，这是不是最合适的处理方式。我们能不能先试一个更简单的方案？',toneNote:'先委婉表达保留意见，再提出建议。'},
 ]) {
  await page.evaluate(sample=>{window.payload={...window.payload,...sample,requestId:window.payload.requestId+1,translationMode:'conversational'};window.emitCapture()},sample);
  await page.locator('.tone-note').filter({hasText:sample.toneNote}).waitFor();
  await page.waitForFunction(()=>document.querySelector('.overlay-stage')?.dataset.motionState==='settled');
  assert.equal(await page.locator('.overlay.compact').count(),0);
  await page.getByRole('button',{name:'复制译文',exact:true}).click();
  assert.equal(await page.evaluate(()=>window.copied),sample.text);
  const geometry=await page.locator('.translation').evaluate(e=>{const note=e.querySelector('.tone-note');return {font:getComputedStyle(note).fontSize,client:e.clientWidth,scroll:e.scrollWidth,bottom:note.getBoundingClientRect().bottom,containerBottom:e.getBoundingClientRect().bottom}});
  assert.equal(geometry.font,'11px');assert(geometry.scroll<=geometry.client+1);assert(geometry.bottom<=geometry.containerBottom+1);
 }
 await page.evaluate(()=>{window.payload={...window.payload,requestId:window.payload.requestId+1,text:'会议三点开始。',toneNote:null};window.emitCapture()});
 await page.locator('.overlay.compact').waitFor();assert.equal(await page.locator('.tone-note').count(),0);
 console.log('PASS daily notes: short/long, copy isolation, layout fit, 11px, no-note compact fallback.');
 // Paragraph spacing, scrolling and exact copy at narrow and normal widths.
 for (const width of [320, 460]) {
  await page.setViewportSize({width,height:320});
  const paragraphs=['阅读时先理解整个段落，不必急着逐词翻译。'.repeat(5),'遇到难句时先找主干，再检查条件与限定。'.repeat(5),'最后用自己的话复述，检查是否真正理解。'.repeat(5)];
  const text=paragraphs.join('\n\n');
  await page.evaluate(text=>{window.payload={...window.payload,requestId:window.payload.requestId+1,text,toneNote:null,translationMode:'conversational'};window.emitCapture()},text);
  await page.waitForFunction(()=>document.querySelectorAll('.translation-paragraph').length===3);
  await page.waitForFunction(()=>document.querySelector('.overlay-stage')?.dataset.motionState==='settled');
  const geometry=await page.locator('.translation').evaluate(e=>({font:getComputedStyle(e).fontSize,line:getComputedStyle(e).lineHeight,gap:getComputedStyle(e.children[1]).marginTop,client:e.clientWidth,scroll:e.scrollWidth,height:e.clientHeight,full:e.scrollHeight}));
  assert.equal(geometry.font,'14px');assert(Math.abs(parseFloat(geometry.line)-24.08)<0.1);assert.equal(geometry.gap,'10px');
  assert(geometry.scroll<=geometry.client+1);assert(geometry.full>geometry.height);
  await page.locator('.translation').evaluate(e=>{e.scrollTop=e.scrollHeight});
  assert(await page.locator('.translation').evaluate(e=>e.scrollTop>0));
  await page.getByRole('button',{name:'复制译文',exact:true}).click();
  assert.equal(await page.evaluate(()=>window.copied),text);
 }
 console.log('PASS compact-comfortable paragraphs: typography, 10px gaps, narrow layout, scrolling and exact copy.');
 // Dense provider output gets display-only grouping; clipboard stays exact.
 const dense='老练且持续存在的威胁行为者不断试探我们的防护措施，并试图绕过我们用来检测和防止滥用的技术手段。我们将继续改进防护措施，并与合作伙伴协调，以提高我们检测、阻止和防止未来滥用的能力。';
 await page.evaluate(text=>{window.payload={...window.payload,requestId:window.payload.requestId+1,text,toneNote:null};window.emitCapture()},dense);
 await page.waitForFunction(()=>document.querySelectorAll('.translation-paragraph').length===2);
 assert.equal(await page.locator('.translation').textContent(),dense);
 await page.getByRole('button',{name:'复制译文',exact:true}).click();
 assert.equal(await page.evaluate(()=>window.copied),dense);
 console.log('PASS medium unbroken provider paragraph: two reading blocks, exact text and copy.');
 // Reading continuity: explanation work must not reset the translation's scroll.
 await page.setViewportSize({width:560,height:320});
 await page.evaluate(()=>{window.payload={...window.payload,requestId:window.payload.requestId+1,translationMode:'academic',text:'阅读位置测试。'.repeat(500)};window.emitCapture()});
 await page.getByRole('button',{name:'查看详细解释'}).waitFor();
 await page.waitForTimeout(300);
 const readingTop=await page.locator('.translation').evaluate(e=>{e.scrollTop=180;return e.scrollTop});
 assert(readingTop>0);
 const pendingBefore=await page.evaluate(()=>window.pending.length);
 await page.getByRole('button',{name:'查看详细解释'}).click();
 await page.waitForFunction(n=>window.pending.length>n,pendingBefore);
 await page.evaluate(()=>window.pending.at(-1).reject('TIMEOUT'));
 await page.getByRole('button',{name:'重新生成解释'}).click();
 await page.waitForFunction(n=>window.pending.length>n+1,pendingBefore);
 await page.evaluate(()=>window.pending.at(-1).resolve({coreExplanation:'阅读位置保持。'.repeat(100),keyConcepts:[],caveat:''}));
 await page.locator('#explanation-panel.open .explanation-section').waitFor();
 assert.equal(await page.locator('.translation').evaluate(e=>e.scrollTop),readingTop);
 await page.getByRole('button',{name:'关闭详细解释'}).click();
 await page.waitForFunction(()=>!document.querySelector('#explanation-panel'));
 await page.waitForTimeout(300);
 assert.equal(await page.locator('.translation').evaluate(e=>e.scrollTop),readingTop);
 // A new passage must start at its beginning even if it also arrives translated.
 await page.evaluate(()=>{window.payload={...window.payload,requestId:window.payload.requestId+1,text:'新选文应从顶部开始。'.repeat(500)};window.emitCapture()});
 await page.waitForFunction(()=>document.querySelector('.translation').textContent.startsWith('新选文'));
  assert.equal(await page.locator('.translation').evaluate(e=>e.scrollTop),0);
  // Late text/font fitting must not anchor an already readable window again.
  await page.setViewportSize({width:560,height:280});
  const fitStart=await page.evaluate(()=>window.calls.length);
  await page.evaluate(()=>{window.payload={...window.payload,text:window.payload.text+'补充说明。'};window.emitCapture()});
  await page.waitForFunction(start=>window.calls.slice(start).some(c=>c.cmd==='fit_overlay_height'),fitStart);
  assert((await page.evaluate(start=>window.calls.slice(start).filter(c=>c.cmd==='fit_overlay_height'),fitStart)).every(c=>c.args.preservePosition===true),'Settled content fitting must retain the current native position');
 console.log('PASS reading continuity: explanation failure/retry/close retain translation scroll; new passage resets it.');
 // Retry admission is immediate, request-scoped, and recovers from IPC rejection.
 await page.evaluate(()=>{window.holdRetry=true;window.payload={...window.payload,requestId:window.payload.requestId+1,phase:'translationFailed',success:false,text:'',errorMessage:'连接超时，请重试'};window.emitCapture()});
 await page.getByRole('button',{name:'重新翻译',exact:true}).waitFor();
 await page.waitForTimeout(180);
 const failureLayout=await page.evaluate(()=>{
   const status=document.querySelector('.status'),retry=status.querySelector('.retry-button');
   const label=status.querySelector('span:not(.progress-dot)').getBoundingClientRect(),button=retry.getBoundingClientRect();
   return {sameColor:getComputedStyle(status).color===getComputedStyle(retry).color,
     aligned:Math.abs((label.top+label.bottom-button.top-button.bottom)/2)<2,
     adjacent:button.left>=label.right, label:status.textContent.trim(), height:innerHeight};
 });
 assert(failureLayout.sameColor && failureLayout.aligned && failureLayout.adjacent);
 assert(failureLayout.label.includes('翻译未完成'));
 await page.locator('.retry-button').evaluate(b=>{b.click();b.click();b.click()});
 await page.waitForFunction(()=>window.retryPending.length===1);
 assert(await page.locator('.retry-button').isDisabled());
 assert(!(await page.locator('.retry-button').isVisible()));
 assert.equal(await page.locator('.status').getAttribute('data-status'),'loading');
 assert.equal(await page.evaluate(()=>innerHeight),failureLayout.height);
 assert(await page.getByRole('button',{name:'关闭翻译'}).isVisible());
 assert.equal(await page.evaluate(()=>window.retryPending[0].args.requestId),await page.evaluate(()=>window.payload.requestId));
 await page.evaluate(()=>window.retryPending[0].reject('IPC disconnected'));
 await page.getByText('未能开始重试，请再试一次',{exact:true}).waitFor();
 assert(await page.locator('.retry-button').isEnabled());
 await page.getByRole('button',{name:'重新翻译',exact:true}).click();
 await page.waitForFunction(()=>window.retryPending.length===2);
 await page.evaluate(()=>{window.oldRetryId=window.payload.requestId;window.payload={...window.payload,requestId:window.payload.requestId+1,phase:'translating',success:false,text:'',errorMessage:null};window.emitCapture();window.retryPending[1].resolve(true)});
 await page.locator('.overlay.translating[data-retry-layout]').waitFor();
 assert.equal(await page.evaluate(()=>innerHeight),failureLayout.height);
 const retained=await page.locator('.overlay').evaluate(e=>({width:e.getBoundingClientRect().width,height:e.getBoundingClientRect().height,w:innerWidth,h:innerHeight}));
 assert(Math.abs(retained.width-retained.w)<=2 && Math.abs(retained.height-retained.h)<=2);
 await page.evaluate(()=>{window.payload={...window.payload,phase:'translated',success:true,text:'重试成功的新结果。'};window.emitCapture()});
 await page.getByText('重试成功的新结果。',{exact:true}).waitFor();
 await page.evaluate(()=>{const latest=window.payload;window.payload={...latest,requestId:window.oldRetryId,text:'不应覆盖的旧结果'};window.emitCapture();window.payload=latest;window.holdRetry=false});
 await page.waitForTimeout(150);
 assert.equal(await page.getByText('不应覆盖的旧结果').count(),0);
 console.log('PASS retry recovery: three clicks submit once; request ID retained; IPC rejection re-enables retry; late result cannot overwrite.');
 // English output from Chinese reading scenarios. Synthetic fixtures, not live model translations.
 const englishSamples=[
  {id:'english-prose',mode:'conversational',text:Array.from({length:6},(_,i)=>`Paragraph ${i+1}. Although the preliminary results suggest that the method may improve performance under the tested conditions, they do not establish that the same improvement will occur on other datasets. Further evaluation should preserve the original assumptions and report both the observed benefit and the remaining uncertainty.`).join('\n\n')},
  {id:'english-list',mode:'conversational',text:'1. Keep the original assumptions together with each conclusion so that readers can understand when the result applies and when it does not.\n2. Compare the proposed method with a suitable baseline and report uncertainty rather than presenting a single score without context.\n3. Record the limitations of the experiment before deciding whether the method is suitable for a different dataset.'},
  {id:'english-long-token',mode:'conversational',text:'Please inspect https://example.org/'+('long-path-segment-'.repeat(18))+' and keep the identifier '+('very_long_identifier_'.repeat(12))+' unchanged.'},
  {id:'english-math',mode:'academic',text:'The attention weights are computed using the scaled dot product:\n\n$$\\operatorname{Attention}(Q,K,V)=\\operatorname{softmax}\\left(\\frac{QK^{T}}{\\sqrt{d_k}}\\right)V$$ (1)\n\nThe factor $1/\\sqrt{d_k}$ limits the magnitude of the dot products. This statement describes the scaling operation; it does not imply that every model benefits equally.'},
 ];
 for(const width of [320,560]) for(const sample of englishSamples){
  await page.setViewportSize({width,height:320});
  await page.evaluate(sample=>{window.payload={...window.payload,requestId:window.payload.requestId+1,phase:'translated',success:true,text:sample.text,translationMode:sample.mode,toneNote:null,errorMessage:null};window.emitCapture()},sample);
  await page.waitForFunction(text=>document.querySelector('.translation')?.textContent.includes(text.slice(0,28)),sample.text);
  await page.waitForFunction(()=>document.querySelector('.overlay-stage')?.dataset.motionState==='settled');
  await page.waitForTimeout(260);
  const layout=await page.locator('.translation').evaluate(e=>({width:e.clientWidth,fullWidth:e.scrollWidth,height:e.clientHeight,fullHeight:e.scrollHeight}));
  assert(layout.fullWidth<=layout.width+1,`${sample.id} at ${width}: outer horizontal overflow`);
  if(sample.id==='english-prose'){
   assert.equal(await page.locator('.translation-paragraph').count(),6);
   assert(layout.fullHeight>layout.height);
   assert(await page.locator('.translation').evaluate(e=>{e.scrollTop=e.scrollHeight;return e.scrollTop>0}));
  }
  if(sample.id==='english-math')assert.equal(await page.locator('.translation .katex').count(),2);
  if(sample.id==='english-list'){
   assert.equal(await page.locator('.translation-list-item').count(),3);
   assert(await page.locator('.translation-list-item').evaluateAll(items=>items.every(item=>{
    const marker=item.querySelector('.translation-list-marker').getBoundingClientRect();
    const content=item.querySelector('.translation-list-content').getBoundingClientRect();
    return content.left>=marker.right-1&&content.width>0;
   })));
   if(width===320&&process.env.QA_READING_SCREENSHOT)await page.screenshot({path:process.env.QA_READING_SCREENSHOT});
  }
  await page.getByRole('button',{name:'复制译文',exact:true}).click();
  assert.equal(await page.evaluate(()=>window.copied),sample.text);
 }
 console.log('PASS English reading: paragraphs, numbered instructions, long URLs/identifiers, mixed math, scrolling and exact copy at 320/560px.');
 // Optional replay of actual provider outputs; no provider requests from the browser.
 if(process.env.TONE_EVAL_REPORT){
  const report=JSON.parse(fs.readFileSync(process.env.TONE_EVAL_REPORT,'utf8'));
  const samples=report.results.filter(r=>r.displayedOutput).map(r=>({...r,output:r.displayedOutput}));
  assert(samples.length>0,'No valid model outputs to replay');
  for(const width of [320,560]) for(const sample of samples){
   await page.setViewportSize({width,height:140});
   await page.evaluate(()=>{window.payload={...window.payload,requestId:window.payload.requestId+1,phase:'translating',text:'',toneNote:null,translationMode:'conversational'};window.emitCapture()});
   await page.locator('.overlay.translating').waitFor();
   await page.evaluate(output=>{window.payload={...window.payload,phase:'translated',text:output.translation,toneNote:output.toneNote};window.emitCapture()},sample.output);
   await page.locator('.overlay.translated').waitFor();
   await page.waitForFunction(()=>document.querySelector('.overlay-stage')?.dataset.motionState==='settled');
   assert.equal(await page.locator('.tone-note').count(),sample.output.toneNote?1:0,sample.id);
   const text=await page.locator('.translation').evaluate(e=>{const copy=e.cloneNode(true);copy.querySelector('.tone-note')?.remove();return Array.from(copy.querySelectorAll('.translation-paragraph'),p=>p.textContent).join('\n\n')});
   assert.equal(text.replace(/\s/g,''),sample.output.translation.replace(/\s/g,''),sample.id);
   const bounds=await page.locator('.translation').evaluate(e=>({client:e.clientWidth,scroll:e.scrollWidth}));
   assert(bounds.scroll<=bounds.client+1,`${sample.id}: horizontal overflow`);
   await page.getByRole('button',{name:'复制译文',exact:true}).click();
   assert.equal(await page.evaluate(()=>window.copied),sample.output.translation,sample.id);
  }
  // A prior response must not restore its note after a newer source has arrived.
  const text=await page.locator('.translation').innerText();
  await page.evaluate(()=>{const current=window.payload;window.payload={...current,requestId:current.requestId-1,text:'OLD TRANSLATION',toneNote:'OLD NOTE'};window.emitCapture();setTimeout(()=>{window.payload=current},100)});
  await page.waitForTimeout(200);
  assert.equal(await page.locator('.translation').innerText(),text);
  assert.equal(await page.getByText('OLD NOTE',{exact:true}).count(),0);
  // A failed response hides notes and retains the existing retry action.
  await page.evaluate(()=>{window.payload={...window.payload,requestId:window.payload.requestId+1,phase:'translationFailed',success:false,text:'',toneNote:'SHOULD NOT RENDER',errorMessage:'格式不完整，请重试'};window.emitCapture()});
  await page.getByRole('button',{name:'重新翻译',exact:true}).waitFor();
  assert.equal(await page.locator('.tone-note').count(),0);
  await page.getByRole('button',{name:'重新翻译',exact:true}).click();
  await page.waitForFunction(()=>window.calls.some(x=>x.cmd==='retry_capture'));
  console.log(`PASS real model output replay: ${samples.length} responses x 2 widths; loading/result, copy, stale note, failure/retry.`);
 }
}finally{await browser.close();await server.close()}})().catch(e=>{console.error(e);process.exitCode=1});
