const {spawnSync}=require('node:child_process');
const fs=require('node:fs');const path=require('node:path');
const root=path.resolve(__dirname,'..');
process.chdir(root);
const flags=process.argv.slice(2);
const corners=['bottom-right','bottom-left','top-right','top-left'];
const corner=flags.find(a=>a.startsWith('--corner='))?.slice(9)||'bottom-right';
const sample=flags.find(a=>a.startsWith('--sample='))?.slice(9)||'long';
const manual=flags.includes('--manual'),all=flags.includes('--all');
if(!corners.includes(corner)||!['short','long','math'].includes(sample)||manual&&all||
   flags.some(a=>!['--manual','--all'].includes(a)&&!a.startsWith('--corner=')&&!a.startsWith('--sample='))){
 console.error('Usage: npm run acceptance -- [--corner=bottom-right] [--sample=long|short|math] [--manual|--all]');process.exit(2);
}
const build=spawnSync('cmd.exe',['/d','/s','/c','npx tauri build --debug --no-bundle'],{cwd:root,stdio:'inherit'});
if(build.status!==0)process.exit(build.status||1);
const cache=path.resolve(root,'tests/artifacts/acceptance-webview');
let failed=false;
try {
 for(const selected of all?corners:[corner]){
  const args=['--acceptance='+selected,'--acceptance-sample='+sample];
  if(!manual)args.push('--acceptance-auto');
  const result=spawnSync(path.join(root,'src-tauri/target/debug/translay.exe'),args,
   {cwd:root,stdio:'inherit',...(manual?{}:{timeout:35000})});
  if(result.error)console.error(result.error.message);
  if(result.status!==0)failed=true;
 }
}finally{
 // Only the isolated test profile is disposable; never delete normal app data.
 if(cache!==path.join(root,'tests','artifacts','acceptance-webview'))throw new Error('Unsafe cleanup path');
 try{
  // WebView child processes may release profile handles after the app exits.
  for(let attempt=0;;attempt++){
   try{fs.rmSync(cache,{recursive:true,force:true});break;}
   catch(error){
    if(attempt>=19)throw error;
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,500);
   }
  }
 }
 catch(error){console.error('Test WebView cache still in use:',error.message);failed=true;}
}
process.exitCode=failed?1:0;
