// Isolated local pages + real Windows UIA. Never attach to personal browser tabs.
const {chromium}=require('playwright');
const {spawn}=require('node:child_process');
const http=require('node:http');
const fs=require('node:fs');
const path=require('node:path');
const root=path.resolve(__dirname,'../..');
const report=[];
const activateWindow=process.argv.includes('--foreground');
(async()=>{
 const server=http.createServer((req,res)=>{res.setHeader('Content-Type','text/html; charset=utf-8');res.end('<!doctype html><title>Same paper title</title><main><p id="first">'+(req.url.startsWith('/paper-b')?'Paper B discusses additive attention and recurrent networks.':'Paper A discusses dot product attention and transformers.')+'</p><p id="second">A second paragraph provides a different selection in the same document.</p></main>');});
 await new Promise(r=>server.listen(0,'127.0.0.1',r));
 const base=`http://127.0.0.1:${server.address().port}`;
 try{
  for(const channel of ['msedge','chrome']){
   let browser,probe;
   const records=[];
   try{
    browser=await chromium.launch({channel,headless:false,args:['--force-renderer-accessibility']});
    const context=await browser.newContext();
    const page=await context.newPage();
    const cdp=await browser.newBrowserCDPSession();
    const info=await cdp.send('SystemInfo.getProcessInfo');
    const pid=info.processInfo.find(p=>p.type==='browser')?.id;
    if(!pid)throw Error('Owned browser process ID unavailable');
    let buffer='',ready=false,exited=false;
    probe=spawn(path.join(root,'src-tauri/target/debug/translay.exe'),['--document-probe','--probe-pipe','--probe-pid='+pid],{cwd:root,windowsHide:true,stdio:['pipe','pipe','pipe']});
    probe.on('exit',()=>{exited=true});
    probe.stderr.on('data',()=>{ready=true});
    probe.stdout.on('data',chunk=>{buffer+=chunk;let n;while((n=buffer.indexOf('\n'))>=0){const line=buffer.slice(0,n).trim();buffer=buffer.slice(n+1);if(line.startsWith('{'))records.push(JSON.parse(line));}});
    async function until(fn){const start=Date.now();while(!fn()){if(exited||Date.now()-start>6000)throw Error('Probe unavailable or sample timed out');await new Promise(r=>setTimeout(r,40));}}
    await until(()=>ready);
    for(const [route,selector,label] of [['/paper-a','#first','a-first'],[null,'#second','a-second'],['/paper-b','#first','b-same-title'],['/paper-a','#first','a-return']]){
     if(route)await page.goto(base+route);
     await page.bringToFront();
     await page.locator(selector).click();
     await page.evaluate(selector=>{const range=document.createRange();range.selectNodeContents(document.querySelector(selector));getSelection().removeAllRanges();getSelection().addRange(range)},selector);
     // Allow the native accessibility provider to publish the new active tab/selection.
     await new Promise(r=>setTimeout(r,350));
     const count=records.length;
     probe.stdin.write(JSON.stringify({sample:true,activateWindow,expectedText:await page.evaluate(()=>getSelection().toString()),expectedUrl:page.url()})+'\n');
     await until(()=>records.length>count);
     records[count].scenario=label;
    }
    async function samplePage(target,label,delay=350){
     await target.bringToFront();
     await target.locator('#first').click();
     await target.evaluate(()=>{const range=document.createRange();range.selectNodeContents(document.querySelector('#first'));getSelection().removeAllRanges();getSelection().addRange(range)});
     // Allow the native accessibility provider to publish the new active tab/selection.
     await new Promise(r=>setTimeout(r,delay));
     const count=records.length;
     probe.stdin.write(JSON.stringify({sample:true,activateWindow,expectedText:await target.evaluate(()=>getSelection().toString()),expectedUrl:target.url()})+'\n');
     await until(()=>records.length>count);
     records[count].scenario=label;
    }
    const tab=await page.context().newPage();
    await tab.goto(base+'/paper-b');
    async function windowFor(target){
     const session=await context.newCDPSession(target);
     try{return (await session.send('Browser.getWindowForTarget')).windowId;}
     finally{await session.detach();}
    }
    const tabsShareBrowserWindow=await windowFor(page)===await windowFor(tab);
    await samplePage(tab,'b-new-tab');
    await samplePage(page,'a-tab-return');
    await page.evaluate(()=>{document.querySelector('#first').textContent='An entirely different article replaces the current text without changing its URL.'});
    await samplePage(page,'a-same-url-replaced-content');
    await page.evaluate(()=>history.pushState({},'', '#another-document'));
    await samplePage(page,'a-hash-route');
    await samplePage(tab,'b-long-settle',2000);
    await page.evaluate(()=>getSelection().removeAllRanges());
    await samplePage(tab,'b-other-selection-cleared');
    await samplePage(page,'a-with-b-selection-retained');
    await tab.evaluate(()=>getSelection().removeAllRanges());
    await samplePage(page,'a-other-selection-cleared');
    await tab.close();
    // Independent single-document cases: no retained selection in a second tab.
    await page.goto(base+'/paper-a');
    await samplePage(page,'isolated-original');
    await page.evaluate(()=>{document.querySelector('#first').textContent='An entirely different article replaces the current text without changing its URL.'});
    await samplePage(page,'isolated-same-url-replacement');
    await page.evaluate(()=>history.pushState({},'', '#another-document'));
    await samplePage(page,'isolated-hash-route');
    await page.reload();
    await samplePage(page,'isolated-reload');
    await page.evaluate(()=>{document.body.innerHTML='<main><p id="first">A newly mounted article replaces the entire page body at the same address.</p></main>'});
    await samplePage(page,'isolated-body-remount');
    probe.stdin.write('stop\n');
    await until(()=>exited);
    const tokens=records.slice(0,8).map(r=>r.evidence?.urlToken);
    const isolated=records.slice(12,16);
    const isolatedVerified=isolated.length===4&&isolated.every(r=>r.selectionMatchesExpected===true&&r.documentMatchesExpected===true&&r.evidence?.urlToken);
    const runtime=records.slice(12,17).map(r=>r.evidence?.runtimeToken);
    const lifecycleVerified=runtime.length===5&&runtime.every(Boolean)&&records.slice(12,17).every(r=>r.selectionMatchesExpected===true&&r.documentMatchesExpected===true);
    const lifecycleObservations=lifecycleVerified?{
     textReplacementChangesNode:runtime[0]!==runtime[1],
     hashRouteChangesNode:runtime[1]!==runtime[2],
     reloadChangesNode:runtime[2]!==runtime[3],
     bodyRemountChangesNode:runtime[3]!==runtime[4]
    }:null;
    const focusObservations={
     foregroundSamples:records.filter(r=>r.isForeground===true).length,
     samplesWithFocusedDocument:records.filter(r=>r.documentSignals?.some(d=>d.containsGlobalFocus===true)).length,
     samplesWithDocumentKeyboardFocus:records.filter(r=>r.documentSignals?.some(d=>d.hasKeyboardFocus===true)).length
    };
    report.push({generatedAt:new Date().toISOString(),browser:channel,tabsShareBrowserWindow,lifecycleObservations,focusObservations,records,isolatedChecks:{
     lifecycleEvidenceCaptured:lifecycleVerified,
     requestedForegroundVerified:!activateWindow||records.every(r=>r.isForeground===true&&r.selectionMatchesExpected===true&&r.documentMatchesExpected===true),
     selectionsAndDocumentsMatch:isolatedVerified,
     hashRouteDistinguished:isolatedVerified?isolated[1].evidence.urlToken!==isolated[2].evidence.urlToken:null,
     reloadPreservesUrl:isolatedVerified?isolated[2].evidence.urlToken===isolated[3].evidence.urlToken:null
    },candidateChecks:tokens.every(Boolean)?{sameDocument:tokens[0]===tokens[1],sameTitleDifferentUrl:tokens[0]!==tokens[2],returnToDocument:tokens[0]===tokens[3],newTabDifferentDocument:tokens[0]!==tokens[4]&&tokens[2]===tokens[4],returnToOriginalTab:tokens[0]===tokens[5],hashRouteDifferent:tokens[5]!==tokens[7]}:null,selectionSafetyChecks:{
     unambiguousSelectionsMatch:[0,1,2,3,9,11].every(i=>records[i]?.selectionMatchesExpected===true && records[i]?.documentMatchesExpected===true),
     retainedSelectionsResolvedOrRejected:[4,5,6,7,8,10].every(i=>{
      const r=records[i];
      return r?.code==='PROBE_AMBIGUOUS_SELECTION'||(r?.isForeground===true&&r.selectionMatchesExpected===true&&r.documentMatchesExpected===true&&r.documentSignals.filter(d=>d.chosenByGlobalFocus===true).length===1);
     })
    },limitations:{sameUrlReplacementDetected:isolatedVerified?isolated[0].evidence.urlToken!==isolated[1].evidence.urlToken:null},terminologyEnabled:false});
   }catch(error){report.push({generatedAt:new Date().toISOString(),browser:channel,records,error:error.message,terminologyEnabled:false});}
   finally{if(probe&&!probe.killed)probe.kill();if(browser)await browser.close();}
  }
 }finally{await new Promise(r=>server.close(r));}
 fs.mkdirSync(path.join(root,'tests/artifacts'),{recursive:true});
 fs.writeFileSync(path.join(root,'tests/artifacts',activateWindow?'document-probe-foreground.json':'document-probe-browsers.json'),JSON.stringify(report,null,2));
 console.log(JSON.stringify(report.map(r=>({browser:r.browser,error:r.error,tabsShareBrowserWindow:r.tabsShareBrowserWindow,lifecycle:r.lifecycleObservations,focus:r.focusObservations,isolatedChecks:r.isolatedChecks,limitations:r.limitations,safety:r.selectionSafetyChecks})),null,2));
 if(report.some(r=>!r.selectionSafetyChecks||!Object.values(r.selectionSafetyChecks).every(Boolean)||!r.isolatedChecks||!Object.values(r.isolatedChecks).every(Boolean)))process.exitCode=2;
})().catch(e=>{console.error(e);process.exitCode=1});
