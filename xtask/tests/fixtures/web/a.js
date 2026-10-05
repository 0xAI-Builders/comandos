// Usa globales del script en línea (api, esc) y define otros.
function tick(){ api("/state"); }
localStorage.getItem("cc-axo");
setInterval(tick, 2000);
function pomoRender(){}
function paint(items){
  const re = /"[a-z]+'/g;            // regex con comillas: no abre cadena
  const total = items.length / 2;   // división, no regex
  return `${esc(items[0])} ${total}` + re.source;
}
function local(){ const esc = s => s; return esc("sombra"); }
counter = 5;
window.shared = 1;
window.addEventListener('message', e => {
  if (e.data?.type !== 'ping') return;
  if ('pong' === e.data.type) {}
});
