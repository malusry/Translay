// Run with sharp on NODE_PATH; only emits the Windows resources used by this app.
const sharp = require('sharp');
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const source = path.join(root, 'assets/brand/folded-ribbon');
const out = path.join(root, 'src-tauri/icons');
async function renderTray(name) {
  const png = await sharp(path.join(source, `${name}.svg`)).resize(32,32).png().toBuffer();
  fs.writeFileSync(path.join(out, `${name}-32x32.png`), png);
  const raw = await sharp(png).ensureAlpha().raw().toBuffer();
  if (raw.length !== 32*32*4) throw new Error('Unexpected RGBA dimensions');
  fs.writeFileSync(path.join(out, `${name}-32x32.rgba`), raw);
}
async function renderTrayAnimation(theme) {
  const folded = fs.readFileSync(path.join(source, `tray-collapsed-${theme}.svg`), 'utf8');
  const frames = [];
  // Reuse exact endpoints. Append four slightly tighter poses for double-click
  // charge; ordinary fold frames retain their existing byte layout.
  for (let frame = 1; frame <= 24; frame++) {
    if (frame === 20) continue;
    const progress = frame / 20;
    const svg = folded
      .replace('rotate(12) scale(.96)', `rotate(${12*progress}) scale(${1-.04*progress})`)
      .replace('rotate(-32) scale(.96)', `rotate(${-32*progress}) scale(${1-.04*progress})`);
    if (svg === folded || svg.includes('scale(.96)')) throw new Error('Missing fold transform');
    const raw = await sharp(Buffer.from(svg)).resize(32,32).ensureAlpha().raw().toBuffer();
    if (raw.length !== 32*32*4) throw new Error('Unexpected animation frame dimensions');
    frames.push(raw);
  }
  fs.writeFileSync(path.join(out, `tray-fold-${theme}-32x32.rgba`), Buffer.concat(frames));
}
(async () => {
  if (process.argv.includes('--tray-animation-only')) {
    for (const theme of ['light','dark']) await renderTrayAnimation(theme);
    console.log('Rendered only tray fold intermediates; existing endpoints and brand resources preserved.');
    return;
  }
  if (process.argv.includes('--tray-states-only')) {
    for (const theme of ['light','dark']) await renderTray(`tray-collapsed-${theme}`);
    console.log('Rendered only collapsed tray PNG/RGBA; existing brand resources preserved.');
    return;
  }
  for (const [name, size] of [['32x32',32],['40x40',40],['48x48',48],['64x64',64],['128x128',128],['128x128@2x',256],['icon',512]]) {
    await sharp(path.join(source,'mark.svg')).resize(size,size).png().toFile(path.join(out,name+'.png'));
  }
  for (const theme of ['light','dark']) {
    await renderTray(`tray-${theme}`);
    await renderTray(`tray-collapsed-${theme}`);
    await renderTrayAnimation(theme);
  }
  // Windows ICO supports PNG-compressed entries, retaining alpha and gradients.
  const sizes=[16,20,24,32,40,48,64,128,256];
  const images=[];
  for (const size of sizes) images.push(await sharp(path.join(source,'mark.svg')).resize(size,size).png().toBuffer());
  const header=Buffer.alloc(6+sizes.length*16);header.writeUInt16LE(1,2);header.writeUInt16LE(sizes.length,4);
  let offset=header.length;
  sizes.forEach((size,i)=>{const p=6+i*16;header[p]=size===256?0:size;header[p+1]=header[p];header.writeUInt16LE(1,p+4);header.writeUInt16LE(32,p+6);header.writeUInt32LE(images[i].length,p+8);header.writeUInt32LE(offset,p+12);offset+=images[i].length;});
  fs.writeFileSync(path.join(out,'icon.ico'),Buffer.concat([header,...images]));
  console.log('Rendered Windows icon family and light/dark tray RGBA.');
})().catch(error=>{console.error(error);process.exitCode=1;});
