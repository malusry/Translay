// Composes the production startup DOM over controlled backgrounds, without native IPC.
const {chromium}=require('playwright');
const fs=require('node:fs/promises');
const path=require('node:path');
const compareEdges=process.argv.includes('--edges');
(async()=>{
 const {preview}=await import('vite');
 const server=await preview({preview:{host:'127.0.0.1',port:5196,strictPort:true}});
 let browser;
 try{
  browser=await chromium.launch({headless:true,channel:'msedge'});
  const page=await browser.newPage({viewport:{width:900,height:compareEdges?390:205},deviceScaleFactor:1});
  await page.addInitScript(()=>{window.__TAURI_INTERNALS__={transformCallback:()=>1,invoke:async cmd=>cmd==='startup_status'?{generation:1,phase:'loading',visible:true}:1};});
  await page.goto(server.resolvedUrls.local[0]+'startup.html');
  await page.locator('.startup-signature.playing').waitFor();
  await page.waitForTimeout(2000);
  await page.evaluate(compare=>{
   const original=document.querySelector('.startup-signature').cloneNode(true);
   const grid=document.createElement('div');grid.id='background-review';
   for(const edged of compare?[false,true]:[false])for(const scene of ['light','dark','text']){
    const panel=document.createElement('section');panel.className=`scene ${scene} ${edged?'edged':''}`;
    const title=document.createElement('header');title.textContent=`${edged?'轻微衬边':'当前版本'} · ${{light:'浅色',dark:'深色',text:'密集文字'}[scene]}`;
    const sample=document.createElement('div');sample.className='sample';
    if(scene==='text'){const paper=document.createElement('div');paper.className='paper';paper.textContent=('阅读需要清晰的层次。Good design supports focused reading. 重要信息应当清晰，细节保持安静。 ').repeat(8);sample.append(paper);}
    const signature=original.cloneNode(true);signature.className='startup-signature';signature.querySelector('.startup-label').textContent='已就绪';sample.append(signature);panel.append(title,sample);grid.append(panel);
   }
   document.body.append(grid);document.querySelector('#root').style.display='none';
  },compareEdges);
  await page.addStyleTag({content:`
   body{background:#f4f3ef}#background-review{display:grid;grid-template-columns:repeat(3,1fr);gap:14px;padding:14px;font:13px "Segoe UI","Microsoft YaHei",sans-serif}
   header{height:30px;color:#424948}.sample{height:145px;position:relative;overflow:hidden;border-radius:10px;background:#fff}.dark .sample{background:#20252a}.paper{position:absolute;inset:0;font:15px/1.65 "Microsoft YaHei",sans-serif;color:#333}
   #background-review .startup-signature,#background-review .startup-signature *{animation:none!important}.startup-sheen,.startup-word-sheen{display:none}
   .edged .startup-lettering img,.edged .startup-label{filter:drop-shadow(0 0 .7px rgba(248,250,246,.85)) drop-shadow(0 0 1px rgba(248,250,246,.45))}
  `});
  await fs.mkdir(path.join('tests','artifacts'),{recursive:true});
  await page.screenshot({path:path.join('tests','artifacts',compareEdges?'startup-edge-comparison.png':'startup-background-review.png')});
  console.log('Rendered production DOM: light, dark and dense-text backgrounds.');
 }finally{if(browser)await browser.close();await new Promise(r=>server.httpServer.close(r));}
})().catch(e=>{console.error(e);process.exitCode=1});
