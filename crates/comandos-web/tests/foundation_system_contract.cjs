const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const {fixture}=require('./theme_preferences_contract.cjs');
let active;
function setup(){
 const f=fixture(),e=f.e;const fresh=()=>fixture().nodes.get('#btn-theme');
 for(const id of ['vol','vol-top','btn-mute','sw-voice','remote','servers','usage','sovereignty','pomo-panel','btn-settings','btn-remote','btn-pomo','btn-sov'])f.nodes.set('#'+id,fresh());
 f.config=[fresh(),fresh()];f.config[0].dataset.key='DESKTOP_NOTIFY';f.config[1].dataset.key='SOUND_ENABLED';
 f.lang=[fresh(),fresh()];f.lang[0].dataset.lang='auto';f.lang[1].dataset.lang='en';
 const queryAll=e.document.querySelectorAll;e.document.querySelectorAll=s=>s==='.cfg[data-key]'?f.config:s==='.lang-btn'?f.lang:queryAll(s);
 e.ComandosNotices={instance:{render:()=>f.calls.push('render-notices')}};e.CENTER_PANELS={settings:'#settings',remote:'#remote',servers:'#servers',sovereignty:'#sovereignty',usage:'#usage',pomo:'#pomo-panel'};e.ONLY_PANEL='';
 e.inApp=()=>true;e.location.search='?anwin=1';e.URLSearchParams=URLSearchParams;e.Date=Date;f.messages=[];e.webkit={messageHandlers:{centro:{postMessage:s=>f.messages.push(JSON.parse(s))}}};
 f.store=new Map();e.localStorage={setItem:(k,v)=>f.store.set(k,v)};e.window=e;f.observers=[];e.MutationObserver=class{constructor(callback){this.callback=callback;}observe(el,opts){f.observers.push({el,opts,callback:this.callback});}};
 for(const [id,panel]of [['btn-settings','settings'],['btn-remote','remote'],['btn-pomo','pomo-panel'],['btn-sov','sovereignty']])f.nodes.get('#'+id).click=()=>{f.calls.push(['click',id]);f.nodes.get('#'+panel).classList.add('open');};
 e.openAnalytics=tab=>f.calls.push(['analytics',tab]);e.toggleSshManager=()=>f.calls.push('servers');e.activateMtab=(scope,tab)=>f.calls.push(['mtab',tab]);
 f.activeTab=fresh();f.activeTab.dataset.mtab='appearance';f.nodes.get('#settings').querySelector=s=>s==='.mtab.active'?f.activeTab:null;
 return f;
}
const tick=()=>new Promise(setImmediate);
async function exercise(ctx,f){
 assert.equal(ctx.desktopPopups(),false);assert.deepEqual([...ctx.popupCategories()],['done','attention','usage']);
 const load=ctx.loadConf();assert.deepEqual(JSON.parse(JSON.stringify(f.calls.at(-1))),['api','/conf']);f.pending.shift().resolve({DESKTOP_NOTIFY:'0',SOUND_ENABLED:'0',VOLUME:'0',SPEAK_DONE:'0',SPEAK_ATTENTION:'0',CC_LANG:'en'});await load;
 assert.equal(ctx.desktopPopups(),false);assert.equal(f.nodes.get('#vol').value,60);assert.equal(f.nodes.get('#btn-mute').dataset.on,'0');assert.equal(f.nodes.get('#sw-voice').attrs['aria-checked'],'false');assert.equal(f.lang[1].classList.contains('on'),true);
 const failed=ctx.loadConf();f.pending.shift().reject(Error('conf failed'));await failed;assert.equal(f.notices.length,0);
 f.config[0].dispatch('click');assert.deepEqual(JSON.parse(JSON.stringify(f.calls.at(-1))),['api','/conf-set',{key:'DESKTOP_NOTIFY',value:'1'}]);f.pending.shift().resolve({});await tick();assert.equal(ctx.desktopPopups(),true);assert.equal(f.config[0].attrs['aria-checked'],'true');
 f.config[1].dispatch('click');f.pending.shift().reject(Error('change denied'));await tick();assert.equal(f.config[1].classList.contains('on'),false);assert.deepEqual(f.notices.at(-1),['change denied',true]);
 f.nodes.get('#btn-mute').dispatch('click');assert.deepEqual(JSON.parse(JSON.stringify(f.calls.at(-1))),['api','/conf-set',{key:'SOUND_ENABLED',value:'1'}]);f.pending.shift().resolve({});await tick();assert.deepEqual(JSON.parse(JSON.stringify(f.calls.at(-1))),['api','/conf']);f.pending.shift().resolve({SOUND_ENABLED:'1',VOLUME:30});await tick();assert.equal(f.nodes.get('#btn-mute').dataset.on,'1');assert.equal(f.nodes.get('#vol-top').value,30);
 f.nodes.get('#vol-top').value=45;f.nodes.get('#vol-top').dispatch('change');assert.deepEqual(JSON.parse(JSON.stringify(f.calls.at(-1))),['api','/conf-set',{key:'VOLUME',value:'45'}]);f.nodes.get('#vol-top').value=46;f.pending.shift().resolve({});await tick();assert.deepEqual(f.notices.at(-1),['Volume 46% (system-wide)']);f.pending.shift().resolve({VOLUME:46});await tick();
 assert.equal(f.observers.length,6);for(const o of f.observers)assert.deepEqual(JSON.parse(JSON.stringify(o.opts)),{attributes:true,attributeFilter:['class']});
 ctx.openCenterPanel({panel:'settings',tab:'appearance'});assert.deepEqual(JSON.parse(JSON.stringify(f.calls.at(-1))),['mtab','appearance']);assert.equal(f.observers.length,7);
 const center=f.observers.at(-1);f.nodes.get('#settings').classList.remove('open');center.callback();assert.deepEqual(f.messages.at(-1),{chainModal:'close'});
 const settings=f.observers.find(o=>o.el===f.nodes.get('#settings'));f.nodes.get('#settings').classList.add('open');settings.callback();assert.deepEqual(f.messages.at(-1),{headerAction:'chains'});assert.equal(f.nodes.get('#settings').classList.contains('open'),false);const stored=JSON.parse(f.store.get('cc-center-panel'));assert.equal(stored.panel,'settings');assert.equal(stored.tab,'appearance');assert.ok(stored.at>0);
 const usage=f.observers.find(o=>o.el===f.nodes.get('#usage'));f.nodes.get('#usage').classList.add('open');usage.callback();assert.deepEqual(f.messages.at(-1),{headerAction:'analytics'});assert.equal(f.nodes.get('#usage').classList.contains('open'),false);
 f.nodes.get('#remote').classList.add('open');const before=f.messages.length;ctx.localStorage.setItem=()=>{throw Error('blocked storage');};f.observers.find(o=>o.el===f.nodes.get('#remote')).callback();assert.equal(f.messages.length,before);assert.equal(f.nodes.get('#remote').classList.contains('open'),true);
 ctx.openCenterPanel({panel:'usage',tab:'accounts'});assert.deepEqual(JSON.parse(JSON.stringify(f.calls.at(-1))),['analytics','accounts']);ctx.openCenterPanel({panel:'servers'});assert.equal(f.calls.at(-1),'servers');
 return {calls:f.calls,notices:f.notices,messages:f.messages,volume:f.nodes.get('#vol').value,desktop:ctx.desktopPopups(),config:f.config.map(b=>b.attrs),stored:{panel:stored.panel,tab:stored.tab}};
}
exports.reference=async root=>{const f=setup(),src=fs.readFileSync(root+'/dash/index.html','utf8');vm.runInNewContext(src.slice(src.indexOf('// ---------- notificaciones del sistema ----------'),src.indexOf('// ---------- UI general ----------'))+'\nthis.desktopPopups=()=>DESKTOP_POPUPS;this.popupCategories=()=>SYSTEM_POPUP_CATS;',f.e);return exercise(f.e,f);};
exports.install=()=>{active=setup();Object.assign(globalThis,active.e);globalThis.window=globalThis;};
exports.native=async()=>{globalThis.desktopPopups=()=>globalThis.DESKTOP_POPUPS;globalThis.popupCategories=()=>globalThis.SYSTEM_POPUP_CATS;return exercise(globalThis,active);};
