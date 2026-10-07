// Full native page CSS versus original theme query, catalog and apply function.
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),cp=require('node:child_process');
const source=fs.readFileSync(path.resolve(__dirname,'../../../dash/term.html'),'utf8');
const normalization=source.match(/const theme\s*=.*?;/)[0];
const catalog='const THEMES = {'+source.split('const THEMES = {')[1].split('\n  };')[0]+'\n};';
const apply='function applyTerminalTheme(name) {'+source.split('function applyTerminalTheme(name) {')[1].split('\n  }')[0]+'\n}';
const names=['noche','dia','calido','termius','bruno','superglass','neon','contraste','ubuntu'];
const cases=[...names.flatMap(n=>[n.toUpperCase(),n.replace(/^./,c=>c.toUpperCase())]),'', 'unknown', 'DİA', 'ΣΟΣ', 'Σ\u0301', 'ＮＯＣＨＥ', '😀', '\ud800'];
for(const input of cases){
  const style={setProperty(k,v){this[k]=v;}},body={style:{}},shell={style:{}};
  const ctx={params:new URLSearchParams({theme:input}),document:{documentElement:{style},body,getElementById:()=>shell},term:{options:{}},refreshLigatures(){}};
  vm.runInNewContext(normalization+catalog+"let activeTheme=THEMES[theme]?theme:'noche';"+apply+'applyTerminalTheme(activeTheme);',ctx);
  const expected=Object.fromEntries(Object.entries(style).filter(([,v])=>typeof v==='string'));
  const result=cp.spawnSync(process.execPath,[path.join(__dirname,'terminal_page_node.cjs'),process.argv[2],'native'],{encoding:'utf8',env:{...process.env,COMANDOS_TERM_FIXTURE_THEME:input,COMANDOS_TERM_EXPECTED_STYLE:JSON.stringify(expected)}});
  assert.equal(result.status,0,JSON.stringify(input)+'\n'+result.stdout+'\n'+result.stderr);
}
console.log('Actual complete native terminal theme CSS equals original for '+cases.length+' uppercase/mixed/unknown/Unicode queries');
