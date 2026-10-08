import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import {runBehavior} from './behavior.mjs';
const root=fileURLToPath(new URL('../../../../',import.meta.url));
const require=createRequire(import.meta.url);
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'comandos-original-oracle-'));
globalThis.fetch=async url=>{
  if(url!=='/tests/fixtures/workspace_layout.json')throw new Error('Unexpected fixture fetch '+url);
  return {json:async()=>JSON.parse(fs.readFileSync(path.join(root,url),'utf8'))};
};
try {
  for(const [source,name] of [['device-drafts','ComandosDeviceDrafts'],['push-settings','PushSettings'],['ui-sounds','ComandosUISounds'],['workspace-layout','WorkspaceLayout'],['quick-terminal','ComandosQuickTerminal'],['session-config','SessionConfig']]){
    const target=path.join(temp,source+'.cjs');
    fs.copyFileSync(path.join(root,'dash',source+'.js'),target);
    globalThis[name]=require(target);
  }
  const value=JSON.stringify(await runBehavior(),null,2)+'\n';
  if(process.argv[2]==='--write')fs.writeFileSync(fileURLToPath(new URL('./oracle.json',import.meta.url)),value);
  else if(value!==fs.readFileSync(fileURLToPath(new URL('./oracle.json',import.meta.url)),'utf8'))throw new Error('Original oracle changed');
  console.log('Original JavaScript oracle: 44 groups matched');
} finally {fs.rmSync(temp,{recursive:true,force:true});}
