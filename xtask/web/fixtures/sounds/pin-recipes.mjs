import fs from 'node:fs';import crypto from 'node:crypto';
const root=new URL('../../../../',import.meta.url);const source=fs.readFileSync(new URL('assets/uisfx/uisfx-0.4.0.js',root),'utf8');
const uisfx=await import('data:text/javascript;base64,'+Buffer.from(source).toString('base64'));
const recipes=Object.fromEntries(uisfx.packNames.map(pack=>[pack,Object.fromEntries(uisfx.cueNames.filter(c=>!uisfx.getCue(c).loop).map(cue=>[cue,uisfx.createRecipe(pack,cue)]))]));
const out=new URL('crates/comandos-web-dom/src/audio_recipes.json',root);const body=JSON.stringify({source_sha256:crypto.createHash('sha256').update(source).digest('hex'),packs:recipes});
if(process.argv.includes('--check')){if(fs.readFileSync(out,'utf8')!==body+'\n')throw new Error('Pinned UISFX recipes drifted');}else fs.writeFileSync(out,body+'\n');
