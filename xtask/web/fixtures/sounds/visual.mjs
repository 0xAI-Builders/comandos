// Each crop is produced by an action through the selected original/WASM API.
// This fixture owns only test inputs and presentation, never component behavior.
export async function renderVisual() {
  const main=document.querySelector('#result');main.replaceChildren();window.fixtureApiCalls=[];
  const panel=(id,title,value)=>{const s=document.createElement('section');s.id=id;const h=document.createElement('h2');h.textContent=title;const p=document.createElement('pre');p.style.cssText='white-space:pre-wrap;overflow-wrap:anywhere';p.textContent=JSON.stringify(value,null,2);s.append(h,p);main.append(s);window.fixtureApiCalls.push(id);return s;};
  const store=new Map();const storage={getItem:k=>store.get(k)??null,setItem:(k,v)=>store.set(k,String(v)),removeItem:k=>store.delete(k)};
  const quick=ComandosQuickTerminal.createQuickTerminal({api:async(path,body)=>({tabId:'term-q1',label:'T-1',requestId:body.requestId}),openTerm(){},storage,makeId:()=> 'id-1'});const opened=await quick.open();
  panel('quick-terminal','Terminal rápida',{opened,pendingRequestId:quick.pendingRequestId,busy:quick.busy});
  let text='';const draft=ComandosDeviceDrafts.createDrafts({key:'k',load:async()=>({drafts:{k:{text:'git status',selStart:3}}}),save:async()=>{},read:()=>text,write:v=>text=v});
  panel('device-drafts','Borrador de este dispositivo',{state:await draft.restore(),text,line:ComandosDeviceDrafts.lineAt('uno\ndos\ntres',5)});
  const registry={harnesses:{codex:{accounts:[{alias:'work',selectable:true}]}},motors:{codex:{models:[{id:'gpt-6-astra',name:'GPT-6 Astra',efforts:['high','ultra'],defaultEffort:'high'}]}},matrix:[{harness:'codex',motor:'codex',selectable:true}]};
  let config=SessionConfig.draft({agent:'codex',model:'gpt-6-astra',effort:'high',account:'work'});config=SessionConfig.update(registry,config,'effort','ultra');
  panel('session-config','Configuración de sesión',{config,validation:SessionConfig.validate(registry,config),models:SessionConfig.fieldOptions(registry,config,'model')});
  const layout={groups:[{id:'a',tree:{type:'tab',tabId:'one'}},{id:'b',tree:{type:'tab',tabId:'two'}}]};
  panel('workspace-layout','Disposición del workspace',WorkspaceLayout.moveTab(layout,'one','two','left'));
  const env={isSecureContext:false,storage,navigator:{},Notification:null,PushManager:null};const controller=PushSettings.createController(env);const state=await controller.current();
  // Avoid id push-settings here: actual Rust attach already owns that production id.
  panel('push-fixture','Avisos push',state);
  const sounds=ComandosUISounds.createUISounds({storage,win:{navigator:{}},doc:{visibilityState:'visible',addEventListener(){},removeEventListener(){}},loadEngine:async()=>null});sounds.setEnabled(true);sounds.setVolume('0.7');
  const section=panel('sound-settings','Sonido',{enabled:sounds.isEnabled(),volume:sounds.getVolume(),cues:sounds.cues()});const button=document.createElement('button');button.textContent=sounds.isEnabled()?'Sonidos activados':'Sonidos desactivados';button.setAttribute('aria-pressed',String(sounds.isEnabled()));button.onclick=()=>{sounds.setEnabled(!sounds.isEnabled());button.textContent=sounds.isEnabled()?'Sonidos activados':'Sonidos desactivados';};section.append(button);await sounds.dispose();
  window.fixtureVisualReady=true;
}
