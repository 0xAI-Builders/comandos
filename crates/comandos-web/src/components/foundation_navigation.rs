//! Complete session switcher and icon/tooltip coordinator regions.
use serde_json::{Value, json};

pub fn icons() -> Value {
    serde_json::from_str(include_str!("foundation_icons.json")).unwrap_or_else(|_| json!({}))
}
pub fn icon(name: &str, size: &str) -> String {
    icons().get(name).and_then(Value::as_str).map(|svg|format!("<span class=\"ico\" style=\"width:{size}px;height:{size}px;display:inline-flex;align-items:center;justify-content:center;vertical-align:-2px\">{svg}</span>")).unwrap_or_default()
}
/// Original for-of code points, indexOf UTF-16 offsets and p=i+1 rule.
pub fn score_units(query: &[u16], target: &[u16]) -> Option<f64> {
    let mut score = 0usize;
    let mut position = 0usize;
    let mut q = query.iter().peekable();
    while let Some(first) = q.next() {
        let mut needle = vec![*first];
        if (0xd800..=0xdbff).contains(first)
            && q.peek()
                .is_some_and(|next| (0xdc00..=0xdfff).contains(*next))
            && let Some(second) = q.next()
        {
            needle.push(*second);
        }
        let tail = target.get(position..)?;
        let found = tail.windows(needle.len()).position(|part| part == needle)?;
        score += found;
        position += found + 1;
    }
    Some(score as f64 + target.len() as f64 * 0.01)
}
pub fn score(query: &str, target: &str) -> Option<f64> {
    score_units(
        &query.to_lowercase().encode_utf16().collect::<Vec<_>>(),
        &target.to_lowercase().encode_utf16().collect::<Vec<_>>(),
    )
}
#[cfg(not(target_arch = "wasm32"))]
pub fn mount_switcher() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn attach_switcher() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn mount_icons() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn attach_icons() -> Result<(), wasm_bindgen::JsValue> {
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fuzzy_score_preserves_utf16_offsets_codepoints_and_misses() {
        assert_eq!(score("", "Ab"), Some(0.02));
        assert_eq!(score("ac", "A b C"), Some(3.05));
        assert_eq!(score("😀b", "a😀b"), Some(2.04));
        assert_eq!(score("z", "ab"), None);
        assert_eq!(score_units(&[0xd800], &[0xd800, 0x61]), Some(0.02));
    }
    #[test]
    fn compiled_icons_preserve_original_wrappers_and_unknown_fallback() {
        assert_eq!(icon("unknown", "14"), "");
        assert!(icon("xai", "15").contains("viewBox=\"0 0 1000 1000\""));
        assert!(icon("chevron-left", "18").contains("width:18px;height:18px"));
        assert_eq!(icons().get("grok"), icons().get("xai"));
    }
}
#[cfg(target_arch = "wasm32")]
pub use web::{attach_icons, attach_switcher, mount_icons, mount_switcher};
#[cfg(target_arch = "wasm32")]
mod web {
    use super::{icons, score_units};
    use crate::components::web_support::*;
    use comandos_web_dom::port::*;
    use js_sys::{Array, Function, JsString, Promise, Reflect};
    use serde_json::json;
    use std::cmp::Ordering;
    use wasm_bindgen::{JsCast, JsValue};
    fn publish(
        name: &str,
        f: impl Fn(Array) -> Result<JsValue, JsValue> + 'static,
    ) -> Result<(), JsValue> {
        set(&js_sys::global(), name, &function(f))
    }
    fn exported(name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
        invoke(&global(name), args)
    }
    fn state() -> JsValue {
        invoke(&global("__comandosState"), &[]).unwrap_or_else(|_| global("S"))
    }
    fn field(v: &JsValue, name: &str) -> String {
        string(&get(v, name))
    }
    fn js_score(query: &JsValue, target: &JsValue) -> Option<f64> {
        let q = JsString::from(query.clone()).to_lower_case();
        let target = JsString::from(target.clone()).to_lower_case();
        score_units(
            &q.iter().collect::<Vec<_>>(),
            &target.iter().collect::<Vec<_>>(),
        )
    }
    fn quick_visible(it: &JsValue) -> bool {
        if !truthy(it) || get(it, "alive") == JsValue::FALSE || truthy(&get(it, "zombie")) {
            return false;
        }
        let status = get(it, "status");
        let idle = !truthy(&status) || status.as_string().as_deref() == Some("idle");
        !field(it, "session").starts_with("term-")
            || truthy(&get(it, "tabbed"))
            || truthy(&get(it, "agent"))
            || !idle
    }
    fn load_recover() -> JsValue {
        let requested = exported("api", &["/tab-history".into()]);
        promise(async move {
            let recover = wait(requested)
                .await
                .unwrap_or_else(|_| Array::new().into());
            set(&state(), "recover", &recover)?;
            Ok(JsValue::UNDEFINED)
        })
    }
    struct Row {
        value: JsValue,
        priority: f64,
        score: f64,
    }
    fn fill() -> Result<(), JsValue> {
        let query: JsValue = JsString::from(get(&id("sw-in"), "value")).trim().into();
        let seen = Reflect::construct(&global("Set").dyn_into::<Function>()?, &Array::new())?;
        let mut rows = Vec::new();
        for it in Array::from(&if truthy(&get(&state(), "list")) {
            get(&state(), "list")
        } else {
            Array::new().into()
        })
        .iter()
        {
            let session = get(&it, "session");
            if !quick_visible(&it) || truthy(&call(&seen, "has", std::slice::from_ref(&session))?) {
                continue;
            }
            call(&seen, "add", &[session])?;
            let target: JsValue =
                format!("{} {}", field(&it, "project"), field(&it, "session")).into();
            let score = if truthy(&query) {
                js_score(&query, &target)
            } else {
                Some(0.0)
            };
            let Some(score) = score else {
                continue;
            };
            let priority = get(&global("SW_ORD"), &field(&it, "status"));
            let priority = if priority.is_null() || priority.is_undefined() {
                3.0
            } else {
                number(&priority)
            };
            let value = object();
            set(&value, "it", &it)?;
            set(&value, "sc", &score.into())?;
            set(&value, "pr", &priority.into())?;
            rows.push(Row {
                value,
                priority,
                score,
            });
        }
        for rec in Array::from(&if truthy(&get(&state(), "recover")) {
            get(&state(), "recover")
        } else {
            Array::new().into()
        })
        .iter()
        {
            let session = get(&rec, "session");
            if !truthy(&rec) || !truthy(&session) || truthy(&call(&seen, "has", &[session])?) {
                continue;
            }
            let cwd = get(&rec, "cwd");
            let target: JsValue = format!(
                "{} {} {}",
                field(&rec, "label"),
                field(&rec, "session"),
                if truthy(&cwd) {
                    string(&cwd)
                } else {
                    String::new()
                }
            )
            .into();
            let score = if truthy(&query) {
                js_score(&query, &target)
            } else {
                Some(0.0)
            };
            let Some(score) = score else {
                continue;
            };
            let value = object();
            set(&value, "rec", &rec)?;
            set(&value, "sc", &score.into())?;
            set(&value, "pr", &4.into())?;
            rows.push(Row {
                value,
                priority: 4.0,
                score,
            });
        }
        rows.sort_by(|a, b| {
            let diff = a.priority - b.priority;
            let diff = if diff == 0.0 || diff.is_nan() {
                a.score - b.score
            } else {
                diff
            };
            diff.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
        });
        // The empty query is the "all sessions" view; do not hide sessions
        // beyond the quick-search result limit.
        if truthy(&query) {
            rows.truncate(12);
        }
        let items = Array::new();
        for row in &rows {
            items.push(&row.value);
        }
        set(&js_sys::global(), "swItems", &items.into())?;
        set(&js_sys::global(), "swSel", &0.into())?;
        let list = id("sw-list");
        set(&list, "innerHTML", &"".into())?;
        for (i, row) in rows.into_iter().enumerate() {
            let d = call(&doc(), "createElement", &["div".into()])?;
            set(
                &d,
                "className",
                &if i == 0 { "sw-row sel" } else { "sw-row" }.into(),
            )?;
            set(
                &d,
                "innerHTML",
                &"<span class=\"dot\"></span><span></span><span class=\"hint\"></span>".into(),
            )?;
            let children = get(&d, "children");
            let rec = get(&row.value, "rec");
            let it = get(&row.value, "it");
            let (color, label, hint) = if truthy(&rec) {
                let label = get(&rec, "label");
                (
                    "var(--faint)",
                    format!(
                        "↺ {}",
                        string(&if truthy(&label) {
                            label
                        } else {
                            get(&rec, "session")
                        })
                    )
                    .into(),
                    exported("t", &["recuperar".into()])?,
                )
            } else {
                let status = field(&it, "status");
                let color = match status.as_str() {
                    "waiting" => "var(--waiting)",
                    "working" => "var(--working)",
                    "done" => "var(--done)",
                    _ => "var(--faint)",
                };
                let hint = if status == "waiting" {
                    "espera TU respuesta".into()
                } else {
                    let value = get(&it, "status");
                    if truthy(&value) { value } else { "".into() }
                };
                (color, get(&it, "project"), exported("t", &[hint])?)
            };
            set(
                &get(&get(&children, "0"), "style"),
                "background",
                &color.into(),
            )?;
            set(&get(&children, "1"), "textContent", &label)?;
            set(&get(&children, "2"), "textContent", &hint)?;
            listen(
                &d,
                "click",
                function(move |_| {
                    exported("swGo", &[(i as f64).into()])?;
                    Ok(JsValue::UNDEFINED)
                }),
            );
            call(&list, "appendChild", &[d])?;
        }
        Ok(())
    }
    fn close() {
        classes(&id("sw-ov"), "hidden", true);
    }
    fn go(index: JsValue) -> Result<JsValue, JsValue> {
        let x = get(&global("swItems"), &string(&index));
        if !truthy(&x) {
            return Ok(Promise::resolve(&JsValue::UNDEFINED).into());
        }
        close();
        let rec = get(&x, "rec");
        let recover = truthy(&rec);
        let requested = if recover {
            exported("api", &["/recover-tab".into(), rec])
        } else {
            exported("openSession", &[get(&x, "it"), "claude".into()])
        };
        Ok(promise(async move {
            let action = async {
                let r = wait(requested).await?;
                if recover {
                    exported("openTerm", &[get(&r, "session"), get(&r, "label")])?;
                    let label = field(&r, "label");
                    let text = exported(
                        "tf",
                        &[
                            format!("{label} recuperada").into(),
                            format!("{label} recovered").into(),
                        ],
                    )?;
                    exported("toast", &[text])?;
                } else {
                    exported("toast", &[r])?;
                }
                Ok::<(), JsValue>(())
            }
            .await;
            if let Err(err) = action {
                exported("toast", &[get(&err, "message"), true.into()])?;
            }
            Ok(JsValue::UNDEFINED)
        }))
    }
    pub fn mount_switcher() -> Result<(), JsValue> {
        set(&js_sys::global(), "swItems", &Array::new().into())?;
        set(&js_sys::global(), "swSel", &0.into())?;
        set(
            &js_sys::global(),
            "SW_ORD",
            &from_json(&json!({"waiting":0,"done":1,"working":2}))?,
        )?;
        publish("swScore", |a| {
            Ok(js_score(&a.get(0), &a.get(1))
                .map(JsValue::from)
                .unwrap_or(JsValue::NULL))
        })?;
        publish("quickVisible", |a| Ok(quick_visible(&a.get(0)).into()))?;
        publish("loadRecover", |_| Ok(load_recover()))?;
        publish("swFill", |_| {
            fill()?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("swClose", |_| {
            close();
            Ok(JsValue::UNDEFINED)
        })?;
        publish("swGo", |a| {
            Ok(go(a.get(0)).unwrap_or_else(|e| Promise::reject(&e).into()))
        })?;
        publish("swOpen", |_| {
            classes(&id("sw-ov"), "hidden", false);
            set(&id("sw-in"), "value", &"".into())?;
            fill()?;
            call(&load_recover(), "then", &[global("swFill")])?;
            call(&id("sw-in"), "focus", &[])?;
            Ok(JsValue::UNDEFINED)
        })
    }
    pub fn attach_switcher() -> Result<(), JsValue> {
        listen(
            &id("sw-ov"),
            "click",
            function(|a| {
                if js_sys::Object::is(&get(&a.get(0), "target"), &id("sw-ov")) {
                    close();
                }
                Ok(JsValue::UNDEFINED)
            }),
        );
        listen(
            &doc(),
            "keydown",
            function(|a| {
                let e = a.get(0);
                if truthy(&exported("openSwitcherFromKey", std::slice::from_ref(&e))?) {
                    return Ok(JsValue::UNDEFINED);
                }
                if truthy(&call(
                    &get(&id("sw-ov"), "classList"),
                    "contains",
                    &["hidden".into()],
                )?) {
                    return Ok(JsValue::UNDEFINED);
                }
                let key = field(&e, "key");
                if key == "Escape" {
                    close();
                } else if key == "Enter" {
                    call(&e, "preventDefault", &[])?;
                    exported("swGo", &[global("swSel")])?;
                } else if key == "ArrowDown" || key == "ArrowUp" {
                    call(&e, "preventDefault", &[])?;
                    let next = (number(&global("swSel"))
                        + if key == "ArrowDown" { 1.0 } else { -1.0 })
                    .min(number(&get(&global("swItems"), "length")) - 1.0)
                    .max(0.0);
                    set(&js_sys::global(), "swSel", &next.into())?;
                    for (i, row) in all(&doc(), ".sw-row").iter().enumerate() {
                        classes(row, "sel", i as f64 == next);
                    }
                }
                Ok(JsValue::UNDEFINED)
            }),
        );
        listen(&id("sw-in"), "input", global("swFill"));
        listen(
            &id("sw-ov"),
            "click",
            function(|a| {
                if field(&get(&a.get(0), "target"), "id") == "sw-ov" {
                    close();
                }
                Ok(JsValue::UNDEFINED)
            }),
        );
        listen(&id("btn-switch"), "click", global("swOpen"));
        Ok(())
    }
    fn hydrate_icons(root: JsValue) -> Result<(), JsValue> {
        for el in all(&root, "[data-icon]") {
            let name = get(&get(&el, "dataset"), "icon");
            let icon = get(&global("ICON"), &string(&name));
            if !truthy(&icon) {
                continue;
            }
            let size = get(&get(&el, "dataset"), "size");
            let size = call(
                &js_sys::global(),
                "parseInt",
                &[if truthy(&size) { size } else { "14".into() }, 10.into()],
            )?;
            set(&el, "innerHTML", &icon)?;
            let style = get(&el, "style");
            let px = format!("{}px", string(&size));
            for key in ["width", "height"] {
                if !truthy(&get(&style, key)) {
                    set(&style, key, &px.clone().into())?;
                }
            }
        }
        Ok(())
    }
    fn hydrate_tooltips(root: JsValue) -> Result<(), JsValue> {
        for button in all(&root, "button") {
            if truthy(&call(&button, "hasAttribute", &["title".into()])?) {
                continue;
            }
            let aria = call(&button, "getAttribute", &["aria-label".into()])?;
            let data = get(&get(&button, "dataset"), "tooltip");
            let text = get(&button, "textContent");
            let value = [aria, data, text]
                .into_iter()
                .find(truthy)
                .unwrap_or_else(|| "".into());
            let label = JsString::from(value)
                .replace_by_pattern(&js_sys::RegExp::new("\\s+", "g"), " ")
                .trim();
            if truthy(&label.clone().into()) {
                set(&button, "title", &label.slice(0, 180).into())?;
            }
        }
        Ok(())
    }
    pub fn mount_icons() -> Result<(), JsValue> {
        set(&js_sys::global(), "ICON", &from_json(&icons())?)?;
        let svg = function(|a| {
            let name = a.get(0);
            let size = if a.get(1).is_undefined() {
                14.into()
            } else {
                a.get(1)
            };
            let icon = get(&global("ICON"), &string(&name));
            Ok(if truthy(&icon) {
                format!("<span class=\"ico\" style=\"width:{}px;height:{}px;display:inline-flex;align-items:center;justify-content:center;vertical-align:-2px\">{}</span>",string(&size),string(&size),string(&icon)).into()
            } else {
                "".into()
            })
        });
        set(&js_sys::global(), "svg", &svg)?;
        set(&js_sys::global(), "__comandosIcon", &svg)?;
        publish("hydrateIcons", |a| {
            hydrate_icons(if a.get(0).is_undefined() {
                doc()
            } else {
                a.get(0)
            })?;
            Ok(JsValue::UNDEFINED)
        })?;
        publish("hydrateButtonTooltips", |a| {
            hydrate_tooltips(if a.get(0).is_undefined() {
                doc()
            } else {
                a.get(0)
            })?;
            Ok(JsValue::UNDEFINED)
        })?;
        let callback = function(|a| {
            for record in Array::from(&a.get(0)).iter() {
                for node in Array::from(&get(&record, "addedNodes")).iter() {
                    if number(&get(&node, "nodeType")) == 1.0 {
                        hydrate_tooltips(node)?;
                    }
                }
            }
            Ok(JsValue::UNDEFINED)
        });
        let observer = Reflect::construct(
            &global("MutationObserver").dyn_into::<Function>()?,
            &Array::of1(&callback),
        )?;
        set(&js_sys::global(), "tooltipObserver", &observer)
    }
    pub fn attach_icons() -> Result<(), JsValue> {
        hydrate_icons(doc())?;
        hydrate_tooltips(doc())?;
        call(
            &global("tooltipObserver"),
            "observe",
            &[
                get(&doc(), "body"),
                from_json(&json!({"childList":true,"subtree":true}))?,
            ],
        )?;
        Ok(())
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
#[allow(clippy::unwrap_used)]
mod wasm_tests {
    use super::*;
    use comandos_web_dom::port::*;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_test::*;
    #[wasm_bindgen(inline_js = r#"
import vm from 'node:vm';
export function navigation_fixture(){
 globalThis.events={};globalThis.observed=[];globalThis.MutationObserver=class{constructor(f){this.f=f}observe(...a){observed.push(a)}};
 const node=(id='')=>{
 const el={id,dataset:{},style:{},attrs:{},textContent:'',value:'',events:{},children:[],classList:new Set(),addEventListener(n,f){(this.events[n]??=[]).push(f)},appendChild(c){this.children.push(c);return c},focus(){this.focused=true},hasAttribute(k){return k in this.attrs},getAttribute(k){return this.attrs[k]??null},querySelectorAll(s){return s==='button'?this.buttons??[]:s==='[data-icon]'?this.icons??[]:[]}};
 el.classList.contains=el.classList.has;el.classList.toggle=(k,on)=>on?el.classList.add(k):el.classList.delete(k);Object.defineProperty(el,'innerHTML',{get(){return this._html??''},set(v){this._html=v;this.children=v?[node(),node(),node()]:[]}});return el;
 };
 globalThis.node=node;globalThis.nodes={};for(const id of ['sw-ov','sw-in','sw-list','btn-switch'])nodes[id]=node(id);nodes['sw-ov'].classList.add('hidden');
 globalThis.document={body:node('body'),buttons:[],icons:[],querySelector:s=>nodes[s.slice(1)]??null,getElementById:k=>nodes[k]??null,createElement:()=>node(),querySelectorAll(s){if(s==='.sw-row')return nodes['sw-list'].children;if(s==='button')return this.buttons;if(s==='[data-icon]')return this.icons;return []},addEventListener(n,f){(events[n]??=[]).push(f)}};
 globalThis.S={list:[],recover:[]};globalThis.__comandosState=()=>S;globalThis.pending=[];globalThis.requests=[];globalThis.toasts=[];globalThis.termOpens=[];
 globalThis.api=(path,body)=>{requests.push({path,body});return new Promise((resolve,reject)=>pending.push({resolve,reject}))};globalThis.t=x=>x;globalThis.tf=(es,en)=>es;globalThis.toast=(...a)=>toasts.push(a);globalThis.openTerm=(...a)=>termOpens.push(a);globalThis.openSession=(...a)=>Promise.resolve('opened '+a[0].session);globalThis.openSwitcherFromKey=()=>false;
}
export async function navigation_contracts(source){
 const eq=(a,b,msg)=>{if(JSON.stringify(a)!==JSON.stringify(b))throw Error(msg+': '+JSON.stringify(a)+' != '+JSON.stringify(b));};const ok=(v,m)=>{if(!v)throw Error(m)};
 const cut=(a,b)=>source.slice(source.indexOf(a),source.indexOf(b,source.indexOf(a)));
 const original=vm.runInNewContext(cut('function swScore','const SW_ORD')+cut('function quickVisible','function swFill')+cut('const ICON =','// Resuelve elementos')+'\n({swScore,quickVisible,ICON,svg})');
 for(const q of ['','ab','😀b','a😀','z','İ','Σ','ΟΣ','\ud800'])for(const s of ['a b','😀ab','a😀b','missing','İS','ΟΣΣ','\ud800x'])eq(swScore(q,s),original.swScore(q,s),'original score '+JSON.stringify([q,s]));
 const cases=[null,{}, {alive:false},{zombie:true},{session:'term-1'},{session:'term-1',tabbed:true},{session:'term-1',agent:'codex'},{session:'term-1',status:'working'},{session:'ordinary'},{session:'term-1',status:'',tabbed:false}];for(const it of cases)eq(quickVisible(it),original.quickVisible(it),'original quickVisible');
 for(const [name,raw] of Object.entries(original.ICON)){eq(ICON[name],raw,'original SVG data '+name);for(const size of [undefined,0,15,'19',null])eq(svg(name,size),original.svg(name,size),'original svg '+name)}eq(svg('absent'),'', 'unknown icon');ok(svg===__comandosIcon,'single icon authority');
 const make=(session,status,extra={})=>({session,project:session,status,...extra});S.list=[make('working','working'),make('waiting','waiting'),make('done','done'),make('waiting','working'),make('dead','idle',{alive:false}),make('term-orphan','idle'),make('term-agent','idle',{agent:'codex'})];S.recover=[{session:'waiting',label:'already live'},{session:'old',label:'Old',cwd:'/private'},{session:'old',label:'duplicate history'}];swFill();eq(swItems.map(x=>x.it?.session??x.rec.session),['waiting','done','working','term-agent','old','old'],'priority/dedup/history');eq(nodes['sw-list'].children[0].children[1].textContent,'waiting','live label uses text');eq(nodes['sw-list'].children[0].children[2].textContent,'espera TU respuesta','waiting hint');eq(nodes['sw-list'].children[4].children[1].textContent,'↺ Old','recovery label');
 nodes['sw-in'].value='Old';swFill();eq(swItems.length,2,'search history');let p=swGo(0);eq(requests.slice(-1)[0].path,'/recover-tab','recover route');pending.shift().resolve({session:'restored',label:'Restored'});await p;eq(termOpens.slice(-1)[0],['restored','Restored'],'restored terminal');eq(toasts.slice(-1)[0],['Restored recuperada'],'recovery toast');
 S.list=Array.from({length:20},(_,i)=>make('s'+i,'working'));S.recover=[];nodes['sw-in'].value='';swFill();eq(swItems.length,12,'limit twelve');await swGo(0);eq(toasts.slice(-1)[0],['opened s0'],'live selection');
 __attachSwitcher();swOpen();ok(nodes['sw-in'].focused,'switcher focuses search');ok(!nodes['sw-ov'].classList.has('hidden'),'overlay open');pending.shift().resolve([{session:'history',label:'History'}]);await Promise.resolve();await Promise.resolve();await Promise.resolve();eq(S.recover[0].session,'history','history refreshed');
 let prevented=false;events.keydown[0]({key:'ArrowDown',preventDefault(){prevented=true}});eq(swSel,1,'keyboard down');ok(prevented,'keyboard prevents scroll');events.keydown[0]({key:'ArrowUp',preventDefault(){}});eq(swSel,0,'keyboard up');events.keydown[0]({key:'Escape'});ok(nodes['sw-ov'].classList.has('hidden'),'escape close');
 swOpen();pending.shift().reject(Error('unavailable'));for(let i=0;i<12;i++)await Promise.resolve();eq(S.recover,[],'failed history clears');nodes['sw-ov'].events.click.forEach(f=>f({target:nodes['sw-ov']}));ok(nodes['sw-ov'].classList.has('hidden'),'overlay click closes');
 const iconEl=node();iconEl.dataset={icon:'moon',size:'17tail'};iconEl.style.width='existing';document.icons=[iconEl];const known=node();known.attrs.title='Keep';const aria=node();aria.attrs['aria-label']='  label\nwith\tspaces  ';const data=node();data.dataset.tooltip='tip';const text=node();text.textContent='😀'.repeat(100);document.buttons=[known,aria,data,text];__attachIcons();eq(iconEl.innerHTML,ICON.moon,'hydrated svg');eq(iconEl.style.width,'existing','existing width');eq(iconEl.style.height,'17px','parseInt size');eq(known.title,undefined,'existing title not rewritten');eq(aria.title,'label with spaces','aria whitespace');eq(data.title,'tip','data fallback');eq(text.title.length,180,'UTF16 tooltip limit');eq(observed.slice(-1)[0][1],{childList:true,subtree:true},'observer options');const added=node();const addedButton=node();addedButton.textContent='Added';added.buttons=[addedButton];added.nodeType=1;tooltipObserver.f([{addedNodes:[added]}]);eq(addedButton.title,'Added','dynamic tooltips');
}
"#)]
    extern "C" {
        fn navigation_fixture();
        #[wasm_bindgen(catch)]
        async fn navigation_contracts(source: &str) -> Result<JsValue, JsValue>;
    }
    #[wasm_bindgen_test(async)]
    async fn navigation_matches_original_scores_icons_and_dom_contracts() {
        navigation_fixture();
        mount_switcher().unwrap();
        mount_icons().unwrap();
        set(
            &js_sys::global(),
            "__attachSwitcher",
            &function(|_| {
                attach_switcher()?;
                Ok(JsValue::UNDEFINED)
            }),
        )
        .unwrap();
        set(
            &js_sys::global(),
            "__attachIcons",
            &function(|_| {
                attach_icons()?;
                Ok(JsValue::UNDEFINED)
            }),
        )
        .unwrap();
        navigation_contracts(include_str!("../../../../dash/index.html"))
            .await
            .unwrap();
    }
}
