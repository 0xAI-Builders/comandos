// Original coordinator reference versus actual Rust/WASM. Node fixtures only.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
function fixture() {
  const nodes = new Map(), calls=[], messages=[], notices=[], pending=[], timers=[], bindings={};
  function node() {
    const classes=new Set(), listeners={};
    return {dataset:{},style:{setProperty(k,v){this[k]=v;}},innerHTML:'',value:'',attrs:{},
      classList:{toggle(k,on){if(on===undefined)on=!classes.has(k);on?classes.add(k):classes.delete(k);return !!on;},contains:k=>classes.has(k),remove:k=>classes.delete(k),add:k=>classes.add(k)},
      setAttribute(k,v){this.attrs[k]=String(v);},getAttribute(k){return this.attrs[k]??null;},
      querySelectorAll(){return [];},querySelector(){return null;},closest(){return null;},
      addEventListener(k,f){(listeners[k]??=[]).push(f);},dispatch(k,extra={}){for(const f of listeners[k]??[])f({target:this,currentTarget:this,stopPropagation(){},preventDefault(){},...extra});},listenerCount:k=>(listeners[k]??[]).length,focus(){this.focused=true;}};
  }
  for(const key of ['#btn-theme','#theme-gallery','#button-style-gallery','#tab-rows','#tabbar','#pf-font-family','#pf-font-size','#pf-font-size-v','#pf-padding','#pf-padding-v','#pf-opacity','#pf-opacity-v','#sw-cursor-blink','#sw-ligatures','#font-preview','#settings'])nodes.set(key,node());
  const range=node();range.min='0';range.max='0';range.value='25';
  const modal=node();modal.dataset.mtab='appearance';modal.closest=()=>({querySelectorAll:s=>s==='.mtab'?[modal]:[]});
  const shape=node();shape.dataset.shape='block';
  const npos=node();npos.dataset.npos='free';
  const body=node(), root=node(), meta=node();
  const listeners=[];
  const doc={readyState:'loading',documentElement:root,body,getElementById:n=>nodes.get('#'+n)??null,
    querySelector:s=>s==='meta[name="theme-color"]'?meta:nodes.get(s)??null,
    querySelectorAll:s=>s==='input[type=range].solid'?[range]:s==='.cur-shape'?[shape]:s==='.npos'?[npos]:s==='.mtab'?[modal]:[],addEventListener(k,f){if(k==='DOMContentLoaded')listeners.push(f);}};
  const frame={dataset:{},contentWindow:{postMessage:(...a)=>messages.push(a)}};
  const compat={dataset:{compat:'1'},contentWindow:{postMessage(){throw Error('compat iframe touched');}}};
  const e={document:doc,location:{origin:'https://private.invalid'},openTerms:new Map([['a',{frame}],['b',{frame:compat}],['null',{frame:null}]]),favoriteVersion:4,favoriteReadAt:0,tabRowsHold:{n:7,h:100},
    $:s=>doc.querySelector(s),svg:(name,size)=>`<svg data-icon="${name}" data-size="${size}"></svg>`,tf:(es,en)=>en,
    mdEsc:s=>(s||'').replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;'),attrEsc:s=>(s||'').replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;'),
    __comandosIcon:(name,size)=>`<svg data-icon="${name}" data-size="${size}"></svg>`,
    styleTermFrame:()=>calls.push('style-frame'),inApp:()=>false,toast:(...a)=>notices.push(a),ulog:(...a)=>calls.push(['ulog',...a]),
    api:(...a)=>{calls.push(['api',...a]);return new Promise((resolve,reject)=>pending.push({resolve,reject}));},
    applyFavorites:v=>calls.push(['favorites',v]),updateTabNavigation:()=>calls.push('nav'),dispatchEvent:e=>calls.push(['event',e.type]),Event:class{constructor(type){this.type=type;}},
    setTimeout:(f,ms)=>{timers.push([f,ms]);return timers.length;},setInterval:(f,ms)=>{timers.push([f,ms]);return timers.length;},
  };e.window=e;
  return {e,nodes,range,modal,shape,npos,root,meta,body,calls,messages,notices,pending,timers,listeners,bindings};
}
let active;
function install() {active=fixture();Object.assign(globalThis,active.e);globalThis.window=globalThis;return active;}
async function exercise(ctx, f) {
  ctx.applyTheme('ubuntu',false);assert.equal(f.root.dataset.theme,'ubuntu');assert.equal(f.root.style.colorScheme,'dark');assert.equal(f.meta.attrs.content,'#300A24');
  ctx.applyTheme('missing',false);assert.equal(f.root.dataset.theme,undefined);assert.equal(ctx.curThemeValue(), 'noche');
  ctx.applyButtonStyle('pixel',false);assert.equal(f.root.dataset.btnStyle,'pixel');
  const request=ctx.selectTheme('dia');assert.equal(f.pending.length,1);assert.equal(f.root.dataset.theme,'dia');f.pending.shift().reject(Error('denied'));await request;assert.equal(ctx.curThemeValue(),'noche');assert.deepEqual(f.notices.at(-1),['denied',true]);
  const style=ctx.selectButtonStyle('arcade');f.pending.shift().resolve({});await style;assert.equal(ctx.curButtonsValue(),'arcade');
  ctx.hydrateTerminalPrefs({font_family:'A<&"',font_size:0,terminal_padding:0,terminal_opacity:0,cursor_blink:false,ligatures:false,fonts:[{family:'B',label:'B<>',a11y:true}]});
  assert.equal(f.nodes.get('#pf-font-size').value,13);assert.equal(f.nodes.get('#pf-padding').value,0);assert.equal(f.nodes.get('#pf-opacity').value,0);
  assert.equal(f.npos.listenerCount('click'),1);ctx.hydrateTerminalPrefs({});assert.equal(f.npos.listenerCount('click'),1);
  ctx.applyTabsLayout('rows');assert.equal(f.body.classList.contains('tabs-rows'),true);assert.equal(f.nodes.get('#tab-rows').attrs['aria-pressed'],'true');
  ctx.activateMtab({querySelectorAll:s=>s==='.mtab'?[f.modal]:[]},'appearance');assert.equal(f.modal.classList.contains('active'),true);
  ctx.wireMtabs();ctx.wireMtabs();assert.equal(f.modal.listenerCount('click'),1);
  ctx.updateRangeFill(f.range);assert.equal(f.range.style['--fill'],'25%');f.range.min='10';f.range.max='20';f.range.value='15';ctx.updateRangeFill(f.range);assert.equal(f.range.style['--fill'],'50%');
  ctx.wireRangeFills();ctx.wireRangeFills();assert.equal(f.range.listenerCount('input'),1);
  f.nodes.get('#pf-font-family').value='Ubuntu Sans Mono';f.nodes.get('#pf-font-size').value=17;ctx.updateFontPreview();assert.equal(f.nodes.get('#font-preview').style.fontSize,'17px');
  const prefs=ctx.loadPrefs();assert.equal(f.pending.length,1);ctx.favoriteVersion=5;f.pending.shift().resolve({favorites:['stale'],theme:'neon',button_style:'tecla',tabs_layout:'row'});await prefs;
  assert.equal(f.calls.some(v=>Array.isArray(v)&&v[0]==='favorites'),false);assert.equal(ctx.curThemeValue(),'neon');
  const save=ctx.setPref({font_size:18},'saved');f.pending.shift().reject(Error('save failed'));await save;assert.deepEqual(f.notices.at(-1),['save failed',true]);
  return {theme:ctx.curThemeValue(),buttons:ctx.curButtonsValue(),messages:f.messages,notices:f.notices,
    calls:f.calls,root:f.root.dataset,meta:f.meta.attrs,button:f.nodes.get('#btn-theme').innerHTML,
    themes:f.nodes.get('#theme-gallery').innerHTML,styles:f.nodes.get('#button-style-gallery').innerHTML,
    range:f.range.style,font:f.nodes.get('#font-preview').style,rows:f.nodes.get('#tab-rows').attrs};
}
exports.reference=async root=>{
  const f=fixture(),src=fs.readFileSync(root+'/dash/index.html','utf8');
  vm.runInNewContext(src.slice(src.indexOf('// ---------- tema ----------'),src.indexOf('// ---------- remoto ----------'))+'\nthis.curThemeValue=()=>curTheme;this.curButtonsValue=()=>curButtonStyle;',f.e);
  return exercise(f.e,f);
};
exports.install=install;
exports.native=async()=>{globalThis.curThemeValue=()=>globalThis.curTheme;globalThis.curButtonsValue=()=>globalThis.curButtonStyle;return exercise(globalThis,active);};

exports.fixture=fixture;
