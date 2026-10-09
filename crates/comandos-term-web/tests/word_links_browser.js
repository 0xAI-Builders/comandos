// Load into full_page_links_browser.js's isolated terminal fixture. Its fetch
// stub answers /terminal-link; labels deliberately contain NO OSC 8 metadata.
window.runWordLinkRegression = async () => {
  const t=window.__comandosTerm;
  const wait=ms=>new Promise(r=>setTimeout(r,ms));
  const frame=()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
  const menu=()=>document.querySelector('#comandos-link-menu');
  const close=()=>document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));
  const paint=()=>fixtureWrite('\x1b[?1003h\x1b[?1006h\x1b[2J\x1b[2;1HAbre prototipo para verlo.');
  const p=()=>{const r=t.screen.getBoundingClientRect();return {clientX:r.x+5.5*t.cellWidth,clientY:r.y+1.5*t.cellHeight,bubbles:true,cancelable:true,button:0}};
  const tap=async()=>{const at=p(),el=document.elementFromPoint(at.clientX,at.clientY),finger=new Touch({identifier:31,target:el,clientX:at.clientX,clientY:at.clientY});el.dispatchEvent(new TouchEvent('touchstart',{bubbles:true,cancelable:true,touches:[finger],changedTouches:[finger]}));await wait(80);el.dispatchEvent(new TouchEvent('touchend',{bubbles:true,cancelable:true,touches:[],changedTouches:[finger]}));};
  const checks={};close();await document.fonts.ready;paint();await frame();await wait(150);window.wordDelay=0;window.wordRequests=[];
  await tap();await wait(180);
  checks.wordWithoutOsc8OpensMenu=menu()?.querySelector('a')?.href==='https://example.com/recovered/complete?q=1';
  checks.correctTextAndPane=wordRequests.length===1&&wordRequests[0].before==='Abre '&&wordRequests[0].after==='prototipo para verlo.'&&wordRequests[0].row===1;
  let copied;Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:async value=>{copied=value}}});menu()?.querySelector('button')?.click();await wait(10);
  checks.copiesRecoveredDestination=copied==='https://example.com/recovered/complete?q=1';close();
  window.wordDelay=250;await tap();t.textarea.dispatchEvent(new KeyboardEvent('keydown',{key:'x',code:'KeyX',bubbles:true,cancelable:true}));await wait(350);
  checks.typingCancelsLateMenu=!menu();close();
  await tap();fixtureWrite('\x1b[2;1HPantalla diferente        ');await wait(350);checks.repaintCancelsStaleMenu=!menu();
  window.wordDelay=0;paint();await frame();const at=p(),el=document.elementFromPoint(at.clientX,at.clientY);
  el.dispatchEvent(new MouseEvent('mousedown',{...at,buttons:1}));el.dispatchEvent(new MouseEvent('mouseup',{...at,buttons:0}));await wait(100);
  checks.mouseAlsoRecoversWord=menu()?.querySelector('a')?.href==='https://example.com/recovered/complete?q=1';close();
  return checks;
};
