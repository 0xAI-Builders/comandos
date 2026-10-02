/* GENERADO por tools/analytics_extract.cjs desde el mockup aprobado (rama prototype/analytics-grill).
   No editar a mano: cambia el mockup, regenera y actualiza tests/fixtures/analytics. */
(function (root) {
  function create(AN, view = {}) {

  // Frontera con el modelo. El mockup dibujaba datos fijos y de confianza; aquí cada nombre que llega del modelo
  // (carpetas, alias de cuenta, nombre del modelo, proyectos de Pomodoro) se escapa antes de que lo vea el código copiado.
  // 🍅 va como entidad: el shell cambia el carácter por un icono y no debe tocar atributos.
  const esc = s => String(s).replace(/[&<>"']|🍅/gu, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;','🍅':'&#127813;'}[c]));
  let uid = 0, phoneDay = 0;
  const ACC = AN.accounts.map(a => ({ ...a, c: a.color, cli: esc(a.cli), alias: esc(a.alias), model: a.model && { ...a.model, n: esc(a.model.n) } }));
  const accs = () => ACC.map(a => ({ ...a, hoy: { ...a.hoy }, sem: { ...a.sem }, model: a.model && { ...a.model } }));
  const DAYS = AN.days;
  const CHRONO = DAYS.slice().reverse();
  // Ninguna sesión mide cero: el mockup divide entre las horas de la cuenta y 0/0 daba NaN.
  const SESS = AN.sessions.map(s => ({ ...s, proj: esc(s.proj), en: Math.min(24, Math.max(s.en, s.st + 1 / 60)) }));
  const sessions = () => SESS;
  const LAST = Object.fromEntries(Object.entries(AN.lastWeek).map(([p, h]) => [esc(p), h]));
  const WASTE = AN.waste;
  // Toda cuenta con 5 h tiene entrada: sin ventana abierta (0 % y sin reset) se dibuja sin hora de reset.
  const H5 = Object.fromEntries(ACC.filter(a => a.h5 != null).map(a => [a.id, { left: a.h5Left ?? null, reset: a.h5Reset ?? null }]));
  const FOCUS = AN.pomodoros.map(f => ({ ...f, proj: esc(f.proj), status: f.status === 'completed' ? 'completado' : 'cancelado', ag: {} }));
  const focusAll = () => FOCUS;
  const TODAYD = AN.week.today, NOWH = AN.week.now;
  const isPast = () => AN.week.offset < 0;
  const weekLabel = () => AN.week.label;
  const isPhone = () => !!view.phone;

const A_=id=>ACC.find(a=>a.id===id);
const fmtH=h=>{let hh=Math.floor(h),mm=Math.round((h-hh)*60);if(mm===60){hh++;mm=0}return String(hh).padStart(2,'0')+':'+String(mm).padStart(2,'0')};
const dur=h=>!h?'—':h<1?Math.round(h*60)+' min':h.toFixed(1)+' h';
function defs(id,c){return `<defs>
 <linearGradient id="${id}l" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${c}" stop-opacity=".95"/><stop offset="1" stop-color="${c}" stop-opacity=".6"/></linearGradient>
 <linearGradient id="${id}sh" x1="0" x2="1"><stop offset="0" stop-color="#000" stop-opacity=".45"/><stop offset=".18" stop-color="#000" stop-opacity="0"/><stop offset=".7" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity=".5"/></linearGradient>
 <linearGradient id="${id}g" x1="0" x2="1"><stop offset="0" stop-color="#fff" stop-opacity=".2"/><stop offset=".12" stop-color="#fff" stop-opacity=".05"/><stop offset=".55" stop-color="#fff" stop-opacity="0"/><stop offset=".86" stop-color="#fff" stop-opacity=".1"/><stop offset="1" stop-color="#fff" stop-opacity=".03"/></linearGradient>
 <radialGradient id="${id}fl" cx=".5" cy=".5" r=".5"><stop offset="0" stop-color="#000" stop-opacity=".55"/><stop offset="1" stop-color="#000" stop-opacity="0"/></radialGradient>
 <linearGradient id="${id}k" x1="0" x2="1"><stop offset="0" stop-color="#262d3d"/><stop offset=".45" stop-color="#3d4659"/><stop offset="1" stop-color="#1c2230"/></linearGradient>
 <linearGradient id="${id}cork" x1="0" x2="1"><stop offset="0" stop-color="#7a5232"/><stop offset=".5" stop-color="#b5814f"/><stop offset="1" stop-color="#6a4529"/></linearGradient></defs>`}
function liquid(id,path,W,H,yTop,yBot,q,c,period=60,amp=2.6){
 const y=yBot-(yBot-yTop)*q/100,n=Math.ceil((W+2*period)/(period/2));
 const wv=`M0 ${y} `+Array.from({length:n},(_,i)=>`q ${period/4} ${i%2?amp:-amp} ${period/2} 0`).join(' ')+` V ${H} H 0 Z`;
 const bub=q>6?[.3,.55,.7,.42].map((f,i)=>`<circle class="bub" cx="${W*f}" cy="${yBot-6}" r="${[1.6,1.1,2,1.3][i]}" fill="#fff" style="--rise:${-(yBot-y-10)}px;animation-delay:${i*1.1}s;animation-duration:${3.6+i*.7}s"/>`).join(''):'';
 return `<clipPath id="${id}c"><path d="${path}"/></clipPath><g clip-path="url(#${id}c)">
  <rect width="${W}" height="${H}" fill="#0b0f17"/>
  ${q>0?`<g class="wave" style="animation-duration:11s;--sh:${-period}px"><path d="${wv}" fill="${c}" opacity=".32" transform="translate(${-period/2},-2.5)"/></g>
  <g class="wave" style="--sh:${-period}px"><path d="${wv}" fill="url(#${id}l)"/></g>
  <rect x="0" y="${y-.6}" width="${W}" height="1.4" fill="#fff" opacity=".5"/>${bub}`:''}
  <rect width="${W}" height="${H}" fill="url(#${id}sh)"/></g>`;
}
function glass(id,path,W,hl){return `<path d="${path}" fill="url(#${id}g)" stroke="#ffffff42" stroke-width="1.3"/><path d="${path}" fill="none" stroke="#ffffff10" stroke-width="5"/>${hl||''}`}
const floor=(id,cx,y,rx)=>`<ellipse cx="${cx}" cy="${y}" rx="${rx}" ry="${rx*.13}" fill="url(#${id}fl)"/>`;
const LOGO={"claude":"<svg viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z\" fill=\"#D97757\" fill-rule=\"nonzero\"></path></svg>","codex":"<svg viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M19.503 0H4.496A4.496 4.496 0 000 4.496v15.007A4.496 4.496 0 004.496 24h15.007A4.496 4.496 0 0024 19.503V4.496A4.496 4.496 0 0019.503 0z\" fill=\"#fff\"></path><path d=\"M9.064 3.344a4.578 4.578 0 012.285-.312c1 .115 1.891.54 2.673 1.275.01.01.024.017.037.021a.09.09 0 00.043 0 4.55 4.55 0 013.046.275l.047.022.116.057a4.581 4.581 0 012.188 2.399c.209.51.313 1.041.315 1.595a4.24 4.24 0 01-.134 1.223.123.123 0 00.03.115c.594.607.988 1.33 1.183 2.17.289 1.425-.007 2.71-.887 3.854l-.136.166a4.548 4.548 0 01-2.201 1.388.123.123 0 00-.081.076c-.191.551-.383 1.023-.74 1.494-.9 1.187-2.222 1.846-3.711 1.838-1.187-.006-2.239-.44-3.157-1.302a.107.107 0 00-.105-.024c-.388.125-.78.143-1.204.138a4.441 4.441 0 01-1.945-.466 4.544 4.544 0 01-1.61-1.335c-.152-.202-.303-.392-.414-.617a5.81 5.81 0 01-.37-.961 4.582 4.582 0 01-.014-2.298.124.124 0 00.006-.056.085.085 0 00-.027-.048 4.467 4.467 0 01-1.034-1.651 3.896 3.896 0 01-.251-1.192 5.189 5.189 0 01.141-1.6c.337-1.112.982-1.985 1.933-2.618.212-.141.413-.251.601-.33.215-.089.43-.164.646-.227a.098.098 0 00.065-.066 4.51 4.51 0 01.829-1.615 4.535 4.535 0 011.837-1.388zm3.482 10.565a.637.637 0 000 1.272h3.636a.637.637 0 100-1.272h-3.636zM8.462 9.23a.637.637 0 00-1.106.631l1.272 2.224-1.266 2.136a.636.636 0 101.095.649l1.454-2.455a.636.636 0 00.005-.64L8.462 9.23z\" fill=\"url(#lg-codex)\"></path><defs><linearGradient gradientUnits=\"userSpaceOnUse\" id=\"lg-codex\" x1=\"12\" x2=\"12\" y1=\"3\" y2=\"21\"><stop stop-color=\"#B1A7FF\"></stop><stop offset=\".5\" stop-color=\"#7A9DFF\"></stop><stop offset=\"1\" stop-color=\"#3941FF\"></stop></linearGradient></defs></svg>","grok":"<svg fill=\"currentColor\" fill-rule=\"evenodd\" viewBox=\"0 0 24 24\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M9.27 15.29l7.978-5.897c.391-.29.95-.177 1.137.272.98 2.369.542 5.215-1.41 7.169-1.951 1.954-4.667 2.382-7.149 1.406l-2.711 1.257c3.889 2.661 8.611 2.003 11.562-.953 2.341-2.344 3.066-5.539 2.388-8.42l.006.007c-.983-4.232.242-5.924 2.75-9.383.06-.082.12-.164.179-.248l-3.301 3.305v-.01L9.267 15.292M7.623 16.723c-2.792-2.67-2.31-6.801.071-9.184 1.761-1.763 4.647-2.483 7.166-1.425l2.705-1.25a7.808 7.808 0 00-1.829-1A8.975 8.975 0 005.984 5.83c-2.533 2.536-3.33 6.436-1.962 9.764 1.022 2.487-.653 4.246-2.34 6.022-.599.63-1.199 1.259-1.682 1.925l7.62-6.815\"></path></svg>"};
function limits(a){const L=[];if(a.week!=null)L.push({n:'Semana',q:100-a.week,left:a.left,reset:a.reset});
 if(a.model)L.push({n:a.model.n,q:100-a.model.v,left:a.model.left??a.left,reset:a.model.reset??a.reset});
 if(a.h5!=null)L.push({n:'Sesión 5 h',q:100-a.h5,left:H5[a.id].left,reset:H5[a.id].reset});return L}
const logoOf=a=>a.provider;
const liqC=(a,q)=>q<=10?'#FF7580':q<=30?'#FFAE1A':a.c;
function colorLogo(k,size,x,y){return `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="0 0 24 24">${LOGO[k].replace(/^<svg[^>]*>/,'').replace(/<\/svg>$/,'').replace(/fill="currentColor"/,'').replace('<path d="M9.27','<path fill="#fff" d="M9.27')}</svg>`}
function logoTile(a,s=26){return `<span class="ltile" style="width:${s}px;height:${s}px">${LOGO[logoOf(a)].replace('<svg','<svg width="'+(s-8)+'" height="'+(s-8)+'"').replace(/fill="currentColor"/,'fill="#fff"')}</span>`}
function brandBottle(q,c,k,name){
 const id='w'+(uid++),W=96,H=210,path='M24 56 Q21 44 34 42 L62 42 Q75 44 72 56 L76 68 L76 188 Q76 200 64 200 L32 200 Q20 200 20 188 L20 68 Z';
 return `<svg viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="${name} queda ${q} por ciento">${defs(id,c)}
 ${floor(id,48,203,36)}
 <rect x="30" y="6" width="36" height="26" rx="5" fill="url(#${id}k)"/>${[0,1,2,3,4,5].map(i=>`<rect x="${34+i*5.4}" y="10" width="2" height="18" rx="1" fill="#0008"/>`).join('')}<rect x="27" y="30" width="42" height="10" rx="3" fill="#1b2130"/>
 ${liquid(id,path,W,H,72,196,q,c,56)}
 ${glass(id,path,W,`<rect x="26" y="74" width="4" height="108" rx="2" fill="#fff" opacity=".2"/>`)}
 <g><rect x="20" y="104" width="56" height="40" fill="#0A0D13" opacity=".5"/><rect x="20" y="104" width="56" height="1" fill="#ffffff30"/><rect x="20" y="143" width="56" height="1" fill="#ffffff30"/>
 ${colorLogo(k,14,41,110)}<text x="48" y="137" text-anchor="middle" font-family="Ubuntu Sans Mono,monospace" font-size="8.5" font-weight="600" fill="#EAF0FB" letter-spacing=".5">${name.toUpperCase().replace('SESIÓN ','')}</text></g>
 </svg>`;
}
function capt(l,small){return `<div class="bcap"><span>${l.n}</span><b class="${l.q<=10?'bad':l.q<=30?'warn':''}">${l.q}%</b><em>${l.left==null?'':'reset en '+l.left}</em></div>`}
function barShelf(as){
 return `<div class="bar"><div class="bar-glow"></div><div class="bar-row">${as.map(a=>{const L=limits(a);return `<div class="bgrp" style="--ac:${a.c}"><div class="bgrp-b">${L.map(l=>`<div class="bcol">${brandBottle(l.q,liqC(a,l.q),logoOf(a),l.n)}${capt(l)}</div>`).join('')}</div><div class="plaque">${logoTile(a,22)}<b>${a.cli}</b><small>${a.alias}</small></div></div>`}).join('')}</div><div class="bar-wood"></div></div>
 <div class="bar-stats">${as.map(a=>`<div style="--ac:${a.c}"><span class="dot"></span><b>${a.cli} ${a.alias}</b><span>hoy ${dur(a.hoy.h)} · ${a.hoy.ses} ses</span><span>semana ${dur(a.sem.h)} · ${a.sem.ses} ses</span></div>`).join('')}</div>`;
}
const dlab=d=>{const x=DAYS.find(y=>y[0]===d);return x[1]+' '+x[2]};
const accName=id=>{const a=A_(id);return a.cli+' '+a.alias};
function projTot(ss){const m={};ss.forEach(s=>{m[s.proj]=m[s.proj]||{h:0,acc:{},n:0};m[s.proj].h+=s.en-s.st;m[s.proj].n++;m[s.proj].acc[s.acc]=(m[s.proj].acc[s.acc]||0)+s.en-s.st});
 return Object.entries(m).map(([p,v])=>({p,h:v.h,n:v.n,acc:Object.entries(v.acc).sort((a,b)=>b[1]-a[1])[0][0]})).sort((a,b)=>b.h-a.h)}
const dayTot=d=>sessions().filter(s=>s.d===d).reduce((x,s)=>x+s.en-s.st,0);
function weekHead(extra=''){const all=sessions(),tot=all.reduce((x,s)=>x+s.en-s.st,0),days=new Set(all.map(s=>s.d)).size,top=projTot(all)[0];
 return `<div class="wk-sum"><div><small>Total con agentes</small><b>${dur(tot)}</b></div><div><small>Días activos</small><b>${days} de 8</b></div><div><small>Sesiones</small><b>${all.length}</b></div><div><small>Proyecto principal</small><b>${top?top.p:'—'}</b><em>${top?dur(top.h):''}</em></div>${extra}</div>`}
function merged(ss){const by={};ss.forEach(s=>{const k=s.proj+'|'+s.acc;(by[k]=by[k]||[]).push({...s,n:1})});
 const out=[];Object.values(by).forEach(arr=>{arr.sort((a,b)=>a.st-b.st);let cur=null;arr.forEach(s=>{if(cur&&s.st<=cur.en+.17){cur.en=Math.max(cur.en,s.en);cur.n++}else{cur&&out.push(cur);cur={...s}}});cur&&out.push(cur)});return out.sort((a,b)=>a.st-b.st)}
function lanes(ss){const out=[];let cl=[],clEnd=-1;const flush=()=>{const ends=[];cl.forEach(s=>{let l=ends.findIndex(e=>e<=s.st+.01);if(l<0){l=ends.length;ends.push(0)}ends[l]=s.en;s.l=l});cl.forEach(s=>{s.nl=ends.length;out.push(s)});cl=[]};
 ss.slice().sort((a,b)=>a.st-b.st).forEach(s=>{if(cl.length&&s.st>=clEnd)flush();cl.push({...s});clEnd=Math.max(clEnd,s.en)});flush();return out}
const tip2=s=>`data-tip="${dlab(s.d)} · ${fmtH(s.st)}–${fmtH(s.en)} · ${dur(s.en-s.st)}${s.n>1?' · '+s.n+' sesiones unidas':''}|${s.proj}|${accName(s.acc)}"`;
function compAxis(PH,GAP=16,src){const act=Array(24).fill(0);(src||sessions()).forEach(s=>{for(let h=Math.floor(s.st);h<Math.ceil(s.en);h++)act[h]=1});
 const segs=[];let h=0;while(h<24){if(act[h]){segs.push({a:h,b:h+1,on:1});h++}else{let e=h;while(e<24&&!act[e])e++;if(e-h>=2)segs.push({a:h,b:e,on:0});else for(let i=h;i<e;i++)segs.push({a:i,b:i+1,on:1});h=e}}
 let y=0;segs.forEach(s=>{s.y=y;s.hgt=s.on?PH:GAP;y+=s.hgt});
 const Y=t=>{const s=segs.find(s=>t>=s.a&&t<=s.b)||segs.at(-1);return s.on?s.y+(t-s.a)*PH:s.y+(t-s.a)/(s.b-s.a)*GAP};
 return {segs,Y,H:y}}
function calHead(d,wd,dd,extra=''){const n=sessions().filter(s=>s.d===d).length;return `<div class="cal-h"><span><b>${wd}</b> ${dd}</span><em>${dur(dayTot(d))} · ${n} ses${extra}</em></div>`}
function axisHtml(ax){return ax.segs.map(s=>s.on?(s.a%2===0?`<span style="top:${s.y}px">${String(s.a).padStart(2,'0')}:00</span>`:''):`<span class="gap" style="top:${s.y+s.hgt/2}px">${String(s.a).padStart(2,'0')}–${String(s.b).padStart(2,'0')}</span>`).join('')}
function gapBands(ax){return ax.segs.map(s=>s.on?`<div class="hline" style="top:${s.y}px"></div>`:`<div class="gapband" style="top:${s.y}px;height:${s.hgt}px"></div>`).join('')}
const nowY=(ax)=>ax.Y(NOWH);
function K1(){const ax=compAxis(34),MAXL=2;
 const cols=CHRONO.map(([d,wd,dd])=>{const ss=lanes(merged(sessions().filter(s=>s.d===d)));
  const vis=ss.filter(s=>s.l<MAXL),hid=ss.filter(s=>s.l>=MAXL);
  const more={};hid.forEach(s=>{const k=Math.floor(s.st);(more[k]=more[k]||[]).push(s)});
  return `<div class="cal-col ${d===TODAYD?'today':''}">${calHead(d,wd,dd)}<div class="cal-body" style="height:${ax.H}px">${gapBands(ax)}
   ${vis.map(s=>{const t=ax.Y(s.st),hp=ax.Y(s.en)-t;return `<div class="ev" style="--ac:${A_(s.acc).c};top:${t}px;height:${Math.max(7,hp-2)}px;left:${s.l/Math.min(MAXL,s.nl)*100}%;width:calc(${100/Math.min(MAXL,s.nl)}% - 3px)" ${tip2(s)}>${hp>=30?`<b>${s.proj}</b><span>${fmtH(s.st)}${s.n>1?' · ×'+s.n:''}</span>`:hp>=16?`<b>${s.proj}</b>`:''}</div>`}).join('')}
   ${Object.entries(more).map(([h,arr])=>`<button class="more" style="top:${ax.Y(+h)}px" data-pop="${encodeURIComponent(JSON.stringify(arr.map(s=>[fmtH(s.st)+'–'+fmtH(s.en),s.proj,accName(s.acc),A_(s.acc).c])))}" data-title="${wd} ${dd} · más sesiones a las ${String(h).padStart(2,'0')}:00">+${arr.length}</button>`).join('')}
   ${d===TODAYD?`<div class="cal-now" style="top:${nowY(ax)}px"></div>`:''}</div></div>`}).join('');
 return weekHead()+`<div class="wk-box cal"><div class="cal-axis" style="height:${ax.H}px">${axisHtml(ax)}</div><div class="cal-cols">${cols}</div></div><div class="wk-note">Une las sesiones seguidas del mismo proyecto (×N), encoge las horas en que nunca trabajas y muestra máximo 2 en paralelo; el resto queda en el botón +N.</div>`}
function usedFor(a){return a.weekUsed===undefined?a.week:a.weekUsed}
function costData(){const as=accs(),ss=sessions();const hAcc={};ss.forEach(s=>hAcc[s.acc]=(hAcc[s.acc]||0)+s.en-s.st);
 const P={};ss.forEach(s=>{const h=s.en-s.st,p=P[s.proj]=P[s.proj]||{p:s.proj,h:0,tok:0,acc:{}};p.h+=h;p.tok+=s.tok;const a=p.acc[s.acc]=p.acc[s.acc]||{h:0,tok:0,q:0};a.h+=h;a.tok+=s.tok;a.q+=h/hAcc[s.acc]*(usedFor(as.find(x=>x.id===s.acc))||0)});
 return Object.values(P).sort((a,b)=>b.tok-a.tok)}
const fmtTok=t=>t>=1000?(t/1000).toFixed(1)+'B':Math.round(t)+'M';
function franjas(){const F=[['Madrugada',0,6],['Mañana',6,12],['Tarde',12,18],['Noche',18,24]];
 return F.map(([n,a,b])=>{let h=0;const pr={};sessions().forEach(s=>{const o=Math.max(0,Math.min(s.en,b)-Math.max(s.st,a));if(o>0){h+=o;pr[s.proj]=(pr[s.proj]||0)+o}});return {n,a,b,h,top:Object.entries(pr).sort((x,y)=>y[1]-x[1]).slice(0,3)}})}
function vsWeek(){const cur={};sessions().forEach(s=>cur[s.proj]=(cur[s.proj]||0)+s.en-s.st);const all=new Set([...Object.keys(cur),...Object.keys(LAST)]);
 const rows=[...all].map(p=>({p,now:cur[p]||0,prev:LAST[p]||0,d:(cur[p]||0)-(LAST[p]||0)})).filter(r=>r.now||r.prev).sort((a,b)=>Math.abs(b.d)-Math.abs(a.d));
 const tn=rows.reduce((x,r)=>x+r.now,0),tp=rows.reduce((x,r)=>x+r.prev,0);return {rows,tn,tp}}
const sgn=d=>d>0.05?`<b class="up">▲ ${dur(d)}</b>`:d<-0.05?`<b class="dn">▼ ${dur(-d)}</b>`:'<b class="eq">=</b>';
function pHour(){const F=franjas(),mx=Math.max(...F.map(f=>f.h),1);return `<div class="panel"><h5>¿A qué hora trabajas?</h5>
 <div class="fr">${F.map(f=>`<div class="frc"><div class="frb"><i style="height:${f.h/mx*100}%"></i></div><b>${dur(f.h)}</b><span>${f.n}</span><em>${String(f.a).padStart(2,'0')}–${String(f.b).padStart(2,'0')} h</em><small>${f.top.map(t=>t[0]).join(' · ')||'—'}</small></div>`).join('')}</div></div>`}
function layeredBottle(a,parts,waste){const id='lb'+(uid++),W=110,H=230,path='M30 64 Q26 50 42 47 L68 47 Q84 50 80 64 L86 80 L86 206 Q86 220 72 220 L38 220 Q24 220 24 206 L24 80 Z',yTop=84,yBot=216;let y=yBot;
 const layers=parts.map(p=>{const hh=(yBot-yTop)*p.q/100,r=`<rect x="0" y="${y-hh}" width="${W}" height="${hh}" fill="${p.c}" opacity="${p.o}" data-tip="${p.p}|${p.q.toFixed(1)}% de la semana|${accName(a.id)}"/><rect x="0" y="${y-hh}" width="${W}" height="1" fill="#0A0D13" opacity=".6"/>`;y-=hh;return r}).join('');
 return `<svg viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="${accName(a.id)} gastado por proyecto">${defs(id,a.c)}${floor(id,55,224,40)}
 <rect x="36" y="8" width="38" height="28" rx="6" fill="url(#${id}k)"/><rect x="32" y="34" width="46" height="10" rx="3" fill="#1b2130"/>
 <clipPath id="${id}c"><path d="${path}"/></clipPath><g clip-path="url(#${id}c)"><rect width="${W}" height="${H}" fill="#0b0f17"/>${layers}<rect width="${W}" height="${H}" fill="url(#${id}sh)"/></g>
 ${glass(id,path,W,`<rect x="29" y="86" width="4" height="114" rx="2" fill="#fff" opacity=".2"/>`)}
 <line x1="22" x2="88" y1="${y}" y2="${y}" stroke="#fff" stroke-dasharray="3 3" opacity=".5"/></svg>`}
function wasteRest(WW){const rest=WW.slice(1),low=rest.filter(x=>x.cyc[0]<=5),hi=rest.filter(x=>x.cyc[0]>5);return (hi[0]?`${accName(hi[0].id)} dejó ${hi[0].cyc[0]}%. `:'')+(low.length?`${low.map(x=>accName(x.id)).join(' y ')} ${low.length>1?'se agotan':'se agota'}.`:'')}
function insightCards(skipHour){const cd=costData(),top=cd[0],v=vsWeek(),F=franjas().sort((a,b)=>b.h-a.h)[0],WW=WASTE.filter(x=>x.cyc.length).sort((a,b)=>b.cyc[0]-a.cyc[0]),w=WW[0],up=v.rows.filter(r=>r.d>0)[0],mixP=cd.find(p=>Object.keys(p.acc).length>1);
 const card=(k,title,body,viz)=>`<div class="ins"><span class="ik">${k}</span><h6>${title}</h6><p>${body}</p>${viz}</div>`;
 const topAcc=top?Object.entries(top.acc).sort((a,b)=>b[1].q-a[1].q)[0]:null;const out=[];
 if(top)out.push(card('Lo más caro',`${top.p} se comió ${topAcc[1].q.toFixed(0)}% de la semana de ${accName(topAcc[0])}`,`${fmtTok(top.tok)} tokens en ${dur(top.h)}. Le siguen ${cd.slice(1,3).map(p=>p.p).join(' y ')}.`,`<div class="mini-rank">${cd.slice(0,5).map(p=>`<div><span>${p.p}</span><i style="width:${p.tok/top.tok*100}%"></i><b>${fmtTok(p.tok)}</b></div>`).join('')}</div>`));
 if(w)out.push(card('Cuota que sobra',`${accName(w.id)} dejó ${w.cyc[0]}% sin usar`,`${w.cyc.length>1?`Pasa casi cada semana (${w.cyc.join('%, ')}%). `:''}${wasteRest(WW)}`,`<div class="wsm">${WW.map(x=>`<div><span class="dot" style="background:${A_(x.id).c}"></span><span>${accName(x.id)}</span><span class="wbar"><i style="width:${100-x.cyc[0]}%;--ac:${A_(x.id).c}"></i><u style="width:${x.cyc[0]}%"></u></span><b class="${x.cyc[0]>=40?'warnT':''}">${x.cyc[0]}%</b></div>`).join('')}</div>`));
 out.push(card('Vs la semana pasada',`${v.tn>=v.tp?'Trabajaste':'Bajaste'} ${dur(Math.abs(v.tn-v.tp))} ${v.tn>=v.tp?'más':'menos'}`,up?`${up.p} fue lo que más subió: ${dur(up.prev)} → ${dur(up.now)}.`:'',`<div class="vsl">${v.rows.slice(0,4).map(r=>`<div><span>${r.p}</span><em>${dur(r.prev)} → ${dur(r.now)}</em>${sgn(r.d)}</div>`).join('')}</div>`));
 if(mixP)out.push(card('Cuentas mezcladas',`${mixP.p} se trabajó en ${Object.keys(mixP.acc).length} cuentas`,'Si quieres separar el gasto por cliente, cada proyecto debería vivir en una sola cuenta.',`<div class="mxb solo">${Object.entries(mixP.acc).map(([id,x])=>`<i style="--ac:${A_(id).c};flex:${x.h}">${Math.round(x.h/mixP.h*100)}%</i>`).join('')}</div>`));
 if(!skipHour&&F&&F.h>0)out.push(card('Tu horario',`Trabajas más en la ${F.n.toLowerCase()}`,`${dur(F.h)} entre las ${String(F.a).padStart(2,'0')} y las ${String(F.b).padStart(2,'0')} h, sobre todo en ${F.top.map(t=>t[0]).join(', ')}.`,pHour().replace('<h5>¿A qué hora trabajas?</h5>','').replace('class="panel"','class="bare"')));
 return out}
function bottlesShelf(){const cd=costData(),as=accs();const pal=['1','.78','.6','.46','.34','.26'];
 return `<div class="bar p5bar"><div class="bar-glow"></div><div class="bar-row">${as.map(a=>{const parts=cd.filter(p=>p.acc[a.id]).map(p=>({p:p.p,q:p.acc[a.id].q})).sort((x,y)=>y.q-x.q);const shown=parts.slice(0,5),rest=parts.slice(5).reduce((x,p)=>x+p.q,0);if(rest>0)shown.push({p:'otros',q:rest});shown.forEach((p,i)=>{p.c=a.c;p.o=pal[i]});
  return `<div class="lbg"><div class="lbb">${layeredBottle(a,shown)}<div class="lbl">${shown.map(p=>`<div><i style="background:${a.c};opacity:${p.o}"></i><span>${p.p}</span><b>${p.q.toFixed(1)}%</b></div>`).join('')}<div class="lbq"><span>${isPast()?'quedó al reset':'queda'}</span><b>${usedFor(a)==null?'—':(100-usedFor(a))+'%'}</b></div></div></div><div class="plaque">${logoTile(a,22)}<b>${a.cli}</b><small>${a.alias}</small></div></div>`}).join('')}</div><div class="bar-wood"></div></div>`}
const head2=(t,s)=>`<div class="sect"><h3>${t}</h3><span>${s}</span></div>`;
const C1=()=>head2('¿Qué proyecto cuesta más?','lo gastado de cada cuenta, en capas por proyecto')+bottlesShelf()+head2('Hallazgos de la semana',weekLabel())+`<div class="ins-grid">${insightCards().join('')}</div>`;
const fmin=m=>m>=60?(m/60).toFixed(1)+' h':m+' min';
function fstats(fs){const done=fs.filter(f=>f.status==='completado'),min=fs.reduce((x,f)=>x+f.act,0),plan=fs.reduce((x,f)=>x+f.plan,0),pause=fs.reduce((x,f)=>x+f.pause,0);
 const days=[...new Set(done.map(f=>f.d))].sort();let streak=0;for(const [d] of DAYS){if(done.some(f=>f.d===d))streak++;else if(d!==TODAYD)break}
 return {n:fs.length,done:done.length,canc:fs.length-done.length,min,plan,pause,pct:fs.length?Math.round(done.length/fs.length*100):0,streak,agent:fs.reduce((x,f)=>x+Object.values(f.ag).reduce((a,b)=>a+b,0),0)}}
function fByProj(fs){const m={};fs.forEach(f=>{const p=m[f.proj]=m[f.proj]||{p:f.proj,min:0,n:0,done:0,ag:0};p.min+=f.act;p.n++;if(f.status==='completado')p.done++;p.ag+=Object.values(f.ag).reduce((a,b)=>a+b,0)});return Object.values(m).sort((a,b)=>b.min-a.min)}
function fByHour(fs){const H=Array.from({length:24},()=>({ok:0,no:0}));fs.forEach(f=>{const h=Math.floor(f.st);f.status==='completado'?H[h].ok++:H[h].no++});return H}
const PCOL={};['#FF6B5B','#2fd3c0','#8B7CFF','#FFAE1A','#4CC2FF','#C5E35A','#FF9AD5','#9AA6BF'].forEach((c,i)=>PCOL[i]=c);
function projColor(p){const ps=fByProj(focusAll()).map(x=>x.p);const i=ps.indexOf(p);return PCOL[i>=0&&i<7?i:7]}
function tomato(ok,size=26,title=''){const id='tm'+(uid++);return `<svg class="tom ${ok?'':'x'}" viewBox="0 0 32 32" width="${size}" height="${size}" role="img" aria-label="${title}"><title>${title}</title>
 <defs><radialGradient id="${id}" cx=".38" cy=".38" r=".7"><stop offset="0" stop-color="${ok?'#ff8a73':'#3a4256'}"/><stop offset=".55" stop-color="${ok?'#ef3b2c':'#2a3142'}"/><stop offset="1" stop-color="${ok?'#a81a10':'#1c2230'}"/></radialGradient></defs>
 <ellipse cx="16" cy="19" rx="12.5" ry="11" fill="url(#${id})" ${ok?'':'stroke="#ff758066" stroke-dasharray="2 2"'}/>
 <path d="M16 9 L13 4.5 L15.5 7.2 L16 3.5 L16.6 7.2 L19 4.5 Z M16 9 Q10 7 7.5 10 Q12 11.5 16 9 Q20 11.5 24.5 10 Q22 7 16 9Z" fill="${ok?'#3fae5a':'#4a5368'}"/>
 ${ok?'<ellipse cx="11" cy="15" rx="3" ry="2" fill="#fff" opacity=".35" transform="rotate(-30 11 15)"/>':''}</svg>`}
const hh=h=>String(Math.floor(h)).padStart(2,'0')+':00';
function pnums(fs){const s=fstats(fs),today=fs.filter(f=>f.d===TODAYD);const days=new Set(fs.map(f=>f.d)).size;
 const k=(l,v,e='')=>`<div><small>${l}</small><b>${v}</b>${e?`<em>${e}</em>`:''}</div>`;
 return `<div class="wk-sum pn">${k('Hoy',today.filter(f=>f.status==='completado').length+' 🍅',fmin(today.reduce((x,f)=>x+f.act,0)))}${k('Esta semana',s.done+' 🍅',fmin(s.min)+' de foco')}${k('Cancelados',s.canc)}${k('Promedio por día',(s.done/Math.max(1,days)).toFixed(1),'en '+days+' días con pomodoros')}</div>`}
function pPerHour(fs){const H=fByHour(fs),mx=Math.max(...H.map(h=>h.ok+h.no),1),best=H.map((h,i)=>({i,...h})).sort((a,b)=>b.ok-a.ok)[0];
 return `<div class="panel"><h5>¿A qué horas?</h5><p class="ph">Pomodoros que empezaste en cada hora. Tu hora más fuerte: <b style="color:var(--fg)">${hh(best.i)}</b>.</p><div class="ph24">${H.map((h,i)=>`<div class="phc" data-tip="${hh(i)}–${hh(i+1)}|${h.ok} completados${h.no?' · '+h.no+' cancelados':''}|"><span><i class="no" style="height:${h.no/mx*100}%"></i><i style="height:${h.ok/mx*100}%"></i></span><em>${i%3?'':String(i).padStart(2,'0')}</em></div>`).join('')}</div></div>`}
function pPerProj(fs){const ps=fByProj(fs),mx=ps[0]?ps[0].n:1;return `<div class="panel"><h5>¿En qué proyectos?</h5>${ps.map(p=>`<div class="pp"><span class="dot" style="background:${projColor(p.p)}"></span><span>${p.p}</span><span class="ppb"><i style="width:${p.done/mx*100}%;background:${projColor(p.p)}"></i></span><b>${p.done} 🍅</b><em>${fmin(p.min)}</em></div>`).join('')||'<div class="dim">Sin pomodoros.</div>'}</div>`}
const S5=()=>{const fs=focusAll();return pnums(fs)+`<div class="panel"><h5>Día por día</h5><div class="mtx-wrap"><table class="dy-tbl ps"><thead><tr><th>Día</th><th>Pomodoros</th><th>Horarios</th><th>Proyectos</th><th class="r">Foco</th></tr></thead><tbody>
 ${DAYS.map(([d,wd,dd])=>{const m=fs.filter(f=>f.d===d);const ok=m.filter(f=>f.status==='completado');const pr={};ok.forEach(f=>pr[f.proj]=(pr[f.proj]||0)+1);
  return `<tr class="${d===TODAYD?'today':''}"><td><b>${wd}</b> ${dd}</td><td><span class="ptoms">${m.map(f=>tomato(f.status==='completado',18,f.proj)).join('')}</span> <b>${ok.length}</b></td><td>${m.map(f=>`<span class="hchip ${f.status==='completado'?'':'x'}">${fmtH(f.st)}</span>`).join('')||'<span class="dim">—</span>'}</td><td>${Object.entries(pr).map(([p,n])=>`<span class="pc" style="--ac:${projColor(p)}">${p} <b>${n}</b></span>`).join(' ')||'<span class="dim">—</span>'}</td><td class="r mono">${fmin(m.reduce((x,f)=>x+f.act,0))}</td></tr>`}).join('')}
 <tr class="tot"><td>Semana</td><td><b>${fstats(fs).done}</b></td><td></td><td></td><td class="r mono">${fmin(fstats(fs).min)}</td></tr></tbody></table></div></div><div class="grid2">${pPerHour(fs)}${pPerProj(fs)}</div>`};
function calendarCuentas(){if(!isPhone())return K1();const days=CHRONO.slice(phoneDay,phoneDay+3);
 const saved=CHRONO.slice();CHRONO.splice(0,CHRONO.length,...days);const html=K1();CHRONO.splice(0,CHRONO.length,...saved);
 return `<div class="pnav"><button data-pd="-1" ${phoneDay<=0?'disabled':''} aria-label="Días anteriores">←</button><span>${days.map(d=>d[1]+' '+d[2]).join(' · ')}</span><button data-pd="1" ${phoneDay>=CHRONO.length-3?'disabled':''} aria-label="Días siguientes">→</button></div>`+html}
function pomodoroPhone(){const fs=focusAll();return pnums(fs)+`<div class="pcards1">${DAYS.map(([d,wd,dd])=>{const m=fs.filter(f=>f.d===d),ok=m.filter(f=>f.status==='completado');const pr={};ok.forEach(f=>pr[f.proj]=(pr[f.proj]||0)+1);
 return `<div class="pc1 ${d===TODAYD?'today':''}"><div class="pc1h"><span><b>${wd}</b> ${dd}</span><span class="ptoms">${m.map(f=>tomato(f.status==='completado',18,f.proj)).join('')}</span><b class="pc1n">${ok.length}</b></div>${m.length?`<div>${m.map(f=>`<span class="hchip ${f.status==='completado'?'':'x'}">${fmtH(f.st)}</span>`).join('')}</div><div class="pc1p">${Object.entries(pr).map(([p,n])=>`<span class="pc" style="--ac:${projColor(p)}">${p} <b>${n}</b></span>`).join(' ')}</div><div class="pc1f mono">${fmin(m.reduce((x,f)=>x+f.act,0))} de foco</div>`:'<div class="dim">sin pomodoros</div>'}</div>`}).join('')}</div>`+pPerHour(fs)+`<div style="margin-top:14px">${pPerProj(fs)}</div>`}

  const TABS = [['cuentas', 'Cuentas'], ['comparar', 'Comparar'], ['pomodoro', 'Pomodoro']];
  function body(tab) {
    const as = accs();
    if (tab === 'cuentas') return (isPast() ? '<div class="wk-note" style="margin:0 0 8px">Las botellas muestran tu cuota de ahora; el calendario es de la semana que elegiste.</div>' : '') + barShelf(as.filter(a => limits(a).length)) + `<div class="sect"><h3>${isPast() ? 'Semana' : 'Esta semana'}</h3><span>${weekLabel()}</span></div>` + calendarCuentas();
    if (tab === 'comparar') return C1();
    return isPhone() ? pomodoroPhone() : S5();
  }
  // Mismo marcado que draw() del mockup: cabecera con pestañas y flechas de semana, luego el cuerpo.
  function html(tab) {
    uid = 0;
    phoneDay = Math.max(0, Math.min(CHRONO.length - 3, view.phoneDay ?? CHRONO.length - 3));
    const off = AN.week.offset;
    const out = `<div class="mhead"><h2>Analytics</h2><div class="tabs" role="tablist">${TABS.map(([k, l]) => `<button class="tab ${k === tab ? 'on' : ''}" role="tab" aria-selected="${k === tab}" data-tab="${k}">${l}</button>`).join('')}</div>
  <div class="wnav"><button data-w="-1" ${off <= (view.minOffset ?? -1) ? 'disabled' : ''} aria-label="Semana anterior">←</button><span>${off ? 'Semana' : 'Esta semana'} · <b>${weekLabel()}</b></span><button data-w="1" ${off >= 0 ? 'disabled' : ''} aria-label="Semana siguiente">→</button></div></div>${body(tab)}`;
    return out.replaceAll('🍅', `<span class="tin">${tomato(true, 16)}</span>`);
  }
  return { html, phoneDays: () => CHRONO.length };

  }
  if (typeof module !== 'undefined' && module.exports) module.exports = { create };
  else root.AnalyticsRender = { create };
})(typeof window !== 'undefined' ? window : globalThis);
