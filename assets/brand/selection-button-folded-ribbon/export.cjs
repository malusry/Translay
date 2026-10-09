const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { chromium } = require('playwright');
(async () => {
 const root = __dirname;
 const sizes = [20,24,28,32,40,48,56,64,128,256];
 const svg = fs.readFileSync(path.join(root,'translay-folded-ribbon.svg'),'utf8');
 const browser = await chromium.launch({headless:true,channel:'msedge'});
 try {
  const page = await browser.newPage({deviceScaleFactor:1});
  fs.mkdirSync(path.join(root,'png'),{recursive:true});
  for (const size of sizes) {
   await page.setViewportSize({width:size,height:size});
   await page.setContent(`<style>html,body{margin:0;background:transparent}svg{display:block;width:${size}px;height:${size}px}</style>${svg}`);
   await page.screenshot({path:path.join(root,'png',`${size}.png`),omitBackground:true});
  }
  const uri='data:image/svg+xml;base64,'+Buffer.from(svg).toString('base64');
  const row=(dark)=>`<section style="background:${dark?'#20252e':'#f6f7f5'};color:${dark?'#e5e7eb':'#34404c'}"><h2>${dark?'Dark':'Light'} / actual CSS sizes</h2><div class="row">${[20,24,28,32,40,64].map(n=>`<div class="sample"><div class="icon"><img src="${uri}" width="${n}" height="${n}"></div><span>${n} px</span></div>`).join('')}</div></section>`;
  await page.setViewportSize({width:960,height:420});
  await page.setContent(`<style>*{box-sizing:border-box}body{margin:0;font-family:Arial}section{height:210px;padding:24px 36px}h2{font-size:14px;font-weight:500;margin:0 0 20px}.row{display:flex;justify-content:space-between}.sample{text-align:center;width:112px}.icon{height:90px;display:flex;align-items:center;justify-content:center}span{font-size:12px}img{display:block}</style>${row(false)}${row(true)}`);
  await page.locator('img').evaluateAll(imgs=>Promise.all(imgs.map(i=>i.decode())));
  await page.screenshot({path:path.join(root,'size-preview.png')});
  const files=['translay-folded-ribbon.svg','approved-comparison.png','size-preview.png',...sizes.map(n=>`png/${n}.png`)];
  const gradients=[...svg.matchAll(/<linearGradient\b([^>]*)>([\s\S]*?)<\/linearGradient>/g)].map(m=>({attributes:m[1].trim(),stops:[...m[2].matchAll(/<stop\b([^>]*)\/?\s*>/g)].map(s=>s[1].trim())}));
  fs.writeFileSync(path.join(root,'manifest.json'),JSON.stringify({status:'integrated-selection-button',master:'translay-folded-ribbon.svg',viewBox:'0 0 64 64',sizes,gradients,files:files.map(file=>({file,sha256:crypto.createHash('sha256').update(fs.readFileSync(path.join(root,file))).digest('hex')}))},null,2)+'\n');
  console.log('Exported '+sizes.length+' transparent PNGs, preview and manifest.');
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1});
