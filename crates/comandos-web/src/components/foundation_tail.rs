//! Notice shelf integration and iframe pane selection. Arrival never changes focus.
pub fn valid_pane(value: &str) -> bool {
    value
        .strip_prefix('%')
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}
#[cfg(not(target_arch = "wasm32"))]
pub fn mount() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn attach() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn pane_identity_requires_exact_percent_ascii_digits() {
        for s in ["%0", "%12", "%0009"] {
            assert!(super::valid_pane(s));
        }
        for s in ["", "%", "12", "%12\n", "%١", "%12x", " %2"] {
            assert!(!super::valid_pane(s));
        }
    }
}
#[cfg(target_arch = "wasm32")]
pub use web::{attach, mount};
#[cfg(target_arch = "wasm32")]
mod web {
    use crate::components::web_support::*;
    use comandos_web_dom::port::*;
    use js_sys::{Array, Reflect, Set};
    use wasm_bindgen::{JsCast, JsValue};
    fn exported(name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
        invoke(&global(name), args)
    }
    fn tab(session: &JsValue) -> Result<JsValue, JsValue> {
        call(&global("openTerms"), "get", std::slice::from_ref(session))
    }
    fn app() -> Result<bool, JsValue> {
        exported("inApp", &[]).map(|v| truthy(&v))
    }
    fn or(a: JsValue, b: JsValue) -> JsValue {
        if truthy(&a) { a } else { b }
    }
    fn toast(error: JsValue) -> Result<(), JsValue> {
        exported("toast", &[get(&error, "message"), true.into()])?;
        Ok(())
    }
    fn select_frame(session: JsValue, pane: JsValue) -> Result<JsValue, JsValue> {
        let entry = tab(&session)?;
        let frame = if truthy(&entry) {
            get(&entry, "frame")
        } else {
            JsValue::UNDEFINED
        };
        if !truthy(&frame) {
            return Ok(JsValue::UNDEFINED);
        }
        let captured = frame.clone();
        let post = function(move |_| {
            let window = get(&captured, "contentWindow");
            if !window.is_null() && !window.is_undefined() {
                let message = object();
                set(&message, "source", &"comandos".into())?;
                set(&message, "type", &"select-pane".into())?;
                set(&message, "pane", &pane)?;
                call(
                    &window,
                    "postMessage",
                    &[message, get(&global("location"), "origin")],
                )?;
            }
            Ok(JsValue::UNDEFINED)
        });
        let ready = (|| -> Result<bool, JsValue> {
            let document = Reflect::get(&frame, &"contentDocument".into())?;
            if document.is_null()
                || document.is_undefined()
                || Reflect::get(&document, &"readyState".into())? != JsValue::from_str("complete")
            {
                return Ok(false);
            }
            let window = Reflect::get(&frame, &"contentWindow".into())?;
            let location = Reflect::get(&window, &"location".into())?;
            Ok(Reflect::get(&location, &"href".into())? != JsValue::from_str("about:blank"))
        })()
        .unwrap_or(false);
        if ready {
            invoke(&post, &[])?;
        } else {
            let options = object();
            set(&options, "once", &true.into())?;
            call(&frame, "addEventListener", &["load".into(), post, options])?;
        }
        Ok(JsValue::UNDEFINED)
    }
    fn source(n: JsValue) -> Result<JsValue, JsValue> {
        let session = get(&n, "sessionKey");
        let label = or(
            get(&tab(&session)?, "label"),
            or(get(&n, "project"), session.clone()),
        );
        let raw_pane = get(&n, "paneId");
        let pane = if super::valid_pane(&string(&or(raw_pane.clone(), "".into()))) {
            Some(raw_pane)
        } else {
            None
        };
        if app()? {
            exported("openInApp", &[session.clone(), "claude".into(), label])?;
            if let Some(pane) = pane {
                let body = object();
                set(&body, "session", &session)?;
                set(&body, "action", &"list".into())?;
                let response = exported("api", &["/terminal-panes".into(), body])?;
                wasm_bindgen_futures::spawn_local(async move {
                    let result = async {
                        let response = wait(Ok(response)).await?;
                        let panes = or(get(&response, "panes"), Array::new().into());
                        let selected = Array::from(&panes)
                            .iter()
                            .find(|p| get(p, "id") == pane)
                            .ok_or_else(|| {
                                JsValue::from(js_sys::Error::new(
                                    "Ese panel ya no existe. El aviso sigue en la franja.",
                                ))
                            })?;
                        let body = object();
                        set(&body, "session", &session)?;
                        set(&body, "action", &"select".into())?;
                        set(&body, "pane", &get(&selected, "id"))?;
                        set(&body, "identity", &get(&selected, "identity"))?;
                        wait(exported("api", &["/terminal-panes".into(), body])).await?;
                        Ok::<(), JsValue>(())
                    }
                    .await;
                    if let Err(error) = result {
                        let _ = toast(error);
                    }
                });
            }
        } else if truthy(&exported("appEnabled", &[])?) {
            exported("openTerm", &[session.clone(), label])?;
            if let Some(pane) = pane {
                select_frame(session, pane)?;
            }
        } else {
            let body = object();
            set(&body, "session", &session)?;
            let response = exported("api", &["/focus".into(), body])?;
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(error) = wait(Ok(response)).await {
                    let _ = toast(error);
                }
            });
        }
        Ok(JsValue::UNDEFINED)
    }
    pub fn mount() -> Result<(), JsValue> {
        set(
            &js_sys::global(),
            "selectPaneInFrame",
            &function(|args| select_frame(args.get(0), args.get(1))),
        )
    }
    pub fn attach() -> Result<(), JsValue> {
        let notices = global("ComandosNotices");
        let panel = global("ONLY_PANEL");
        if !truthy(&notices) || (truthy(&panel) && panel != JsValue::from_str("notices")) {
            return Ok(());
        }
        let shelf = panel == JsValue::from_str("notices");
        let options = object();
        set(&options, "api", &global("api"))?;
        set(&options, "deviceId", &or(global("WS_DEVICE"), "".into()))?;
        set(
            &options,
            "host",
            &if shelf {
                get(&doc(), "body")
            } else {
                id("panes")
            },
        )?;
        set(
            &options,
            "onBadge",
            &function(|args| {
                let badge = query(&doc(), "#notif-badge");
                if truthy(&badge) {
                    let n = args.get(0);
                    set(&badge, "textContent", &string(&n).into())?;
                    classes(&badge, "hidden", !truthy(&n));
                }
                Ok(JsValue::UNDEFINED)
            }),
        )?;
        set(
            &options,
            "isSessionLive",
            &function(|args| {
                let session = args.get(0);
                if !truthy(&session) {
                    return Ok(false.into());
                }
                let live = get(&state(), "liveSess");
                let collection = if live.is_instance_of::<Set>() {
                    live
                } else {
                    global("openTerms")
                };
                call(&collection, "has", &[session])
            }),
        )?;
        set(
            &options,
            "showFloat",
            // The sidebar is a file workspace. Notification history and native
            // desktop delivery keep their existing settings and dedicated UI.
            &function(|_| Ok(false.into())),
        )?;
        let captured = notices.clone();
        set(
            &options,
            "describe",
            &function(move |args| {
                let n = args.get(0);
                if truthy(&call(&captured, "isNews", std::slice::from_ref(&n))?) {
                    return Ok("Noticias".into());
                }
                let parts = Array::new();
                parts.push(&or(
                    get(&n, "project"),
                    or(get(&n, "projectKey"), "General".into()),
                ));
                let session = get(&n, "sessionKey");
                let entry = if truthy(&session) {
                    tab(&session)?
                } else {
                    JsValue::NULL
                };
                let name = or(get(&entry, "label"), session.clone());
                if truthy(&name) && name != parts.get(0) {
                    parts.push(&name);
                }
                let pane = get(&n, "paneId");
                let list = or(get(&state(), "list"), Array::new().into());
                if let Some(item) = Array::from(&list).iter().find(|x| {
                    get(x, "session") == session && (!truthy(&pane) || get(x, "pane") == pane)
                }) {
                    let agent = get(&item, "agent");
                    if truthy(&agent) {
                        parts.push(&agent);
                    }
                }
                if truthy(&pane) {
                    let label: JsValue = js_sys::JsString::from("pane ").concat(&pane).into();
                    parts.push(&label);
                }
                call(&parts.into(), "join", &[" · ".into()])
            }),
        )?;
        set(
            &options,
            "openSource",
            &function(|args| source(args.get(0))),
        )?;
        set(
            &options,
            "openNews",
            &function(|args| {
                let edition = get(&args.get(0), "editionId");
                let edition = if truthy(&edition) {
                    edition
                } else {
                    JsValue::UNDEFINED
                };
                let news = global("ComandosNews");
                if truthy(&news) && get(&news, "open").is_function() {
                    call(&news, "open", &[edition])?;
                } else {
                    let reader = global("NewsReader");
                    let instance = get(&reader, "instance");
                    if truthy(&reader) && truthy(&instance) {
                        call(&instance, "open", &[edition])?;
                    } else {
                        let button = id("btn-news");
                        if !button.is_null() && !button.is_undefined() {
                            call(&button, "click", &[])?;
                        }
                    }
                }
                Ok(JsValue::UNDEFINED)
            }),
        )?;
        set(
            &options,
            "onError",
            &function(|args| {
                let err = args.get(0);
                let message = if truthy(&err) {
                    get(&err, "message")
                } else {
                    JsValue::UNDEFINED
                };
                exported("toast", &[or(message, string(&err).into()), true.into()])?;
                Ok(JsValue::UNDEFINED)
            }),
        )?;
        call(&notices, "install", &[options])?;
        if shelf && truthy(&get(&notices, "instance")) {
            call(&get(&notices, "instance"), "toggleStrip", &[true.into()])?;
            let node = id("notices");
            if !node.is_null() && !node.is_undefined() {
                call(
                    &node,
                    "addEventListener",
                    &[
                        "click".into(),
                        function(|args| {
                            let e = args.get(0);
                            let target = get(&e, "target");
                            if truthy(&get(&target, "closest"))
                                && truthy(&call(
                                    &target,
                                    "closest",
                                    &["[data-nt-act=\"close\"]".into()],
                                )?)
                            {
                                call(&e, "stopImmediatePropagation", &[])?;
                                call(&e, "preventDefault", &[])?;
                                if app()? {
                                    let message = object();
                                    set(&message, "headerAction", &"notices".into())?;
                                    call(
                                        &get(&get(&global("webkit"), "messageHandlers"), "centro"),
                                        "postMessage",
                                        &[js_sys::JSON::stringify(&message)?.into()],
                                    )?;
                                }
                            }
                            Ok(JsValue::UNDEFINED)
                        }),
                        true.into(),
                    ],
                )?;
            }
        }
        listen(
            &js_sys::global(),
            "message",
            function(move |args| {
                let e = args.get(0);
                let d = get(&e, "data");
                if get(&e, "origin") != get(&global("location"), "origin")
                    || !truthy(&d)
                    || get(&d, "source") != JsValue::from_str("comandos-term")
                    || get(&d, "type") != JsValue::from_str("user-interaction")
                {
                    return Ok(JsValue::UNDEFINED);
                }
                let entries = call(&global("openTerms"), "values", &[])?;
                if !Array::from(&entries).iter().any(|o| {
                    let frame = get(&o, "frame");
                    truthy(&frame) && get(&frame, "contentWindow") == get(&e, "source")
                }) {
                    return Ok(JsValue::UNDEFINED);
                }
                let instance = get(&notices, "instance");
                if instance.is_null() || instance.is_undefined() {
                    return Ok(JsValue::UNDEFINED);
                }
                let presence = get(&instance, "presence");
                if !presence.is_null() && !presence.is_undefined() {
                    let gesture = object();
                    set(&gesture, "isTrusted", &true.into())?;
                    call(&presence, "onGesture", &[gesture])?;
                }
                Ok(JsValue::UNDEFINED)
            }),
        );
        Ok(())
    }
}
#[cfg(all(test, target_arch = "wasm32"))]
#[allow(clippy::unwrap_used)]
mod wasm_tests {
    use super::*;
    use comandos_web_dom::port::function;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_test::*;
    #[wasm_bindgen(inline_js = r#"
import vm from 'node:vm';
export function tail_fixture(){
 globalThis.window=globalThis;globalThis.tailEvents={};globalThis.tailCalls=[];globalThis.tailToasts=[];globalThis.tailInstalls=[];globalThis.tailGestures=[];globalThis.tailPopups=[];
 globalThis.addEventListener=(n,f)=>{(tailEvents[n]??=[]).push(f)};
 globalThis.tailNative=false;globalThis.tailWeb=true;globalThis.ONLY_PANEL='';globalThis.WS_DEVICE='device-fixture';globalThis.DESKTOP_POPUPS=true;globalThis.SYSTEM_POPUP_CATS=new Set(['permission']);
 globalThis.inApp=()=>tailNative;globalThis.appEnabled=()=>tailWeb;globalThis.location={origin:'https://fixture.invalid'};
 const node=()=>({events:{},textContent:'',classList:{toggle(c,on){this[c]=on}},addEventListener(n,f,o){(this.events[n]??=[]).push({f,o})},click(){tailCalls.push(['button-news'])}});
 globalThis.tailNodes={panes:node(),notices:node(),'notif-badge':node(),'btn-news':node()};globalThis.document={body:node(),getElementById:k=>tailNodes[k]??null,querySelector:s=>tailNodes[s.slice(1)]??null};
 globalThis.S={liveSess:new Set(['live']),list:[{session:'live',pane:'%2',agent:'claude'},{session:'live',pane:'%3',agent:'other'}]};globalThis.__comandosState=()=>S;
 globalThis.openTerms=new Map();globalThis.tailFrame=(ready=true,href='https://fixture.invalid/term')=>({contentDocument:{readyState:ready?'complete':'loading'},contentWindow:{location:{href},postMessage(...a){tailCalls.push(['post',...a])}},events:{},addEventListener(n,f,o){(this.events[n]??=[]).push({f,o})}});
 openTerms.set('live',{label:'Terminal',frame:tailFrame()});openTerms.set('orphan',{label:'Old'});
 globalThis.openInApp=(...a)=>tailCalls.push(['native',...a]);globalThis.openTerm=(...a)=>tailCalls.push(['term',...a]);globalThis.toast=(...a)=>tailToasts.push(a);
 globalThis.tailPanes=[{id:'%2',identity:{pid:42,birth:17}}];globalThis.tailFailure='';globalThis.api=(path,body)=>{tailCalls.push(['api',path,body]);return tailFailure===path?Promise.reject(Error('route denied')):Promise.resolve({panes:tailPanes})};
 globalThis.webkit={messageHandlers:{centro:{postMessage:m=>tailPopups.push(m)}}};
 globalThis.ComandosNews=undefined;globalThis.NewsReader=undefined;
 globalThis.ComandosNotices={isNews:n=>n.category==='news',install(o){tailInstalls.push(o);this.options=o;this.instance={toggleStrip:n=>tailCalls.push(['strip',n]),presence:{onGesture:g=>tailGestures.push(g)}}}};
}
export async function tail_contracts(source,attach){
 const eq=(a,b,m)=>{if(JSON.stringify(a)!==JSON.stringify(b))throw Error(m+': '+JSON.stringify(a)+' != '+JSON.stringify(b))};const ok=(x,m)=>{if(!x)throw Error(m)};const drain=async()=>{for(let i=0;i<24;i++)await Promise.resolve()};
 let N=ComandosNotices,o=N.options;ok(o.api===api,'API reference');eq(o.deviceId,'device-fixture','device');ok(o.host===tailNodes.panes,'normal host');
 const originalText=source.slice(source.indexOf('// N2: franja'),source.indexOf('</script>',source.indexOf('// N2: franja')));
 const context={window:null,document,S,openTerms,location,ONLY_PANEL:'',WS_DEVICE,inApp,DESKTOP_POPUPS,SYSTEM_POPUP_CATS,api,toast,openInApp,appEnabled,openTerm,$:s=>tailNodes[s.slice(1)],Set,addEventListener(){},ComandosNotices:{isNews:N.isNews,install(x){this.options=x}}};context.window=context;vm.runInNewContext(originalText,context);const ref=context.ComandosNotices.options;
 for(const n of [{category:'news'},{},{projectKey:'Key',sessionKey:'none'},{project:'Terminal',sessionKey:'live',paneId:'%2'},{project:'Project',sessionKey:'live',paneId:'%3'},{project:'😀\ud800',sessionKey:'live',paneId:'%999'},{sessionKey:'orphan',paneId:'%3'}])eq(o.describe(n),ref.describe(n),'original describe');
 for(const s of ['',null,'live','orphan','missing'])eq(o.isSessionLive(s),ref.isSessionLive(s),'live set predicate');S.liveSess=null;for(const s of ['live','orphan','missing'])eq(o.isSessionLive(s),ref.isSessionLive(s),'live map fallback');S.liveSess=new Set(['live']);
 o.onBadge(4);eq(tailNodes['notif-badge'].textContent,'4','badge');ok(!tailNodes['notif-badge'].classList.hidden,'badge visible');o.onBadge(0);ok(tailNodes['notif-badge'].classList.hidden,'badge zero hidden');
 for(const native of [false,true])for(const pop of [false,true])for(const category of ['permission','other']){tailNative=native;DESKTOP_POPUPS=pop;context.DESKTOP_POPUPS=pop;eq(o.showFloat({category}),false,'sidebar does not float notices');}tailNative=false;DESKTOP_POPUPS=true;
 selectPaneInFrame('missing','%1');eq(tailCalls,[],'missing frame silent');selectPaneInFrame('live','%2');eq(tailCalls.pop(),['post',{source:'comandos',type:'select-pane',pane:'%2'},location.origin],'ready iframe selection');
 for(const kind of ['loading','blank','security']){const f=tailFrame(kind!=='loading',kind==='blank'?'about:blank':undefined);if(kind==='security')Object.defineProperty(f.contentWindow,'location',{get(){throw Error('cross origin')}});openTerms.set('pending',{frame:f});selectPaneInFrame('pending','%9');eq(f.events.load.length,1,'wait for '+kind);eq(f.events.load[0].o,{once:true},'once load '+kind);eq(tailCalls,[],'no premature post '+kind);f.events.load[0].f();eq(tailCalls.pop()[1].pane,'%9','deferred post '+kind);}
 tailNative=true;o.openSource({sessionKey:'live',project:'Project',paneId:'%2'});eq(tailCalls.slice(0,2),[['native','live','claude','Terminal'],['api','/terminal-panes',{session:'live',action:'list'}]],'native order synchronously');await drain();eq(tailCalls.pop(),['api','/terminal-panes',{session:'live',action:'select',pane:'%2',identity:{pid:42,birth:17}}],'native identity select');tailCalls=[];
 tailPanes=[];o.openSource({sessionKey:'live',paneId:'%2'});await drain();eq(tailToasts.pop(),['Ese panel ya no existe. El aviso sigue en la franja.',true],'missing pane failure');ok(!tailCalls.some(x=>x[2]?.action==='select'),'no absent pane selection');tailCalls=[];
 tailFailure='/terminal-panes';o.openSource({sessionKey:'live',paneId:'%2'});await drain();eq(tailToasts.pop(),['route denied',true],'native route error');tailFailure='';tailCalls=[];
 for(const pane of ['', '%', '%12x','%12\n']){o.openSource({sessionKey:'live',paneId:pane});await drain();eq(tailCalls.length,1,'invalid pane only opens session');tailCalls=[];}
 tailNative=false;tailWeb=true;o.openSource({sessionKey:'live',paneId:'%2'});eq(tailCalls,[['term','live','Terminal'],['post',{source:'comandos',type:'select-pane',pane:'%2'},location.origin]],'web local-only pane');tailCalls=[];
 tailWeb=false;tailFailure='/focus';o.openSource({sessionKey:'live',paneId:'%2'});await drain();eq(tailCalls,[['api','/focus',{session:'live'}]],'local focus only');eq(tailToasts.pop(),['route denied',true],'focus error');tailCalls=[];tailFailure='';
 ComandosNews={open(id){ok(this===ComandosNews,'news receiver');tailCalls.push(['news',id])}};o.openNews({editionId:'edition'});eq(tailCalls.pop(),['news','edition'],'namespace news');ComandosNews={open:42};NewsReader={instance:{open(id){ok(this===NewsReader.instance,'reader receiver');tailCalls.push(['reader',id])}}};o.openNews({editionId:''});eq(tailCalls.pop(),['reader',undefined],'reader fallback undefined id');NewsReader=null;o.openNews({});eq(tailCalls.pop(),['button-news'],'news button fallback');o.onError(Error('notice failure'));eq(tailToasts.pop(),['notice failure',true],'error message');o.onError('plain error');eq(tailToasts.pop(),['plain error',true],'string error');
 const message=tailEvents.message[0],frame=openTerms.get('live').frame;const event={origin:location.origin,data:{source:'comandos-term',type:'user-interaction'},source:frame.contentWindow};for(const bad of [{...event,origin:'https://wrong.invalid'},{...event,source:{}},{...event,data:null},{...event,data:{source:'wrong',type:'user-interaction'}},{...event,data:{source:'comandos-term',type:'wrong'}}])message(bad);eq(tailGestures,[],'origin and known frame guard');message(event);eq(tailGestures,[{isTrusted:true}],'trusted embedded gesture');N.instance.presence=null;message(event);N.instance=null;message(event);
 ONLY_PANEL='notices';tailNative=true;tailInstalls=[];tailCalls=[];attach();o=N.options;ok(o.host===document.body,'shelf body host');eq(tailCalls,[['strip',true]],'shelf always open');const close=tailNodes.notices.events.click[0];eq(close.o,true,'capture close');let stopped=0,prevented=0;close.f({target:{closest:s=>s==='[data-nt-act="close"]'},stopImmediatePropagation(){stopped++},preventDefault(){prevented++}});eq([stopped,prevented],[1,1],'native shelf close stops delegation');eq(tailPopups,[JSON.stringify({headerAction:'notices'})],'native header action');tailNative=false;close.f({target:{closest:()=>true},stopImmediatePropagation(){},preventDefault(){}});eq(tailPopups.length,1,'browser close no native message');
 const count=tailInstalls.length;ONLY_PANEL='other';attach();eq(tailInstalls.length,count,'other panel skips');ONLY_PANEL='';ComandosNotices=null;attach();eq(tailInstalls.length,count,'missing optional module skips');
}
"#)]
    extern "C" {
        fn tail_fixture();
        #[wasm_bindgen(catch)]
        async fn tail_contracts(source: &str, attach: JsValue) -> Result<JsValue, JsValue>;
    }
    #[wasm_bindgen_test(async)]
    async fn tail_preserves_notice_callbacks_native_identity_and_origin_frame_guards() {
        tail_fixture();
        mount().unwrap();
        attach().unwrap();
        let attach_callback = function(|_| {
            attach()?;
            Ok(JsValue::UNDEFINED)
        });
        tail_contracts(include_str!("../../../../dash/index.html"), attach_callback)
            .await
            .unwrap();
    }
}
