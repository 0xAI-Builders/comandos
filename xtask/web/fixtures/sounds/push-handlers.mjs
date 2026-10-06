export async function checkPushHandlers(){
 await window.fixtureReady;
 const box=document.querySelector('#push-settings');
 const waitState=async state=>{for(let i=0;i<100&&(box.dataset.state!==state||[...box.querySelectorAll('button')].some(b=>b.disabled));i++)await new Promise(r=>setTimeout(r,10));if(box.dataset.state!==state)throw new Error('push handler failed '+state);};
 const before={state:box.dataset.state,prompts:window.fixturePrompts};
 document.querySelector('#push-enable').click();await waitState('enabled');const enabled=box.querySelector('#push-status').textContent;
 document.querySelector('#push-test').click();await new Promise(r=>setTimeout(r,30));const tested=box.querySelector('#push-status').textContent;
 document.querySelector('#push-disable').click();await waitState('disabled');
 const result={before,after:{state:box.dataset.state,prompts:window.fixturePrompts},requests:window.fixtureRequests.map(r=>[r.method,r.url]),buttons:[...box.querySelectorAll('button')].map(b=>({id:b.id,hidden:b.hidden,disabled:b.disabled})),messages:{enabled,tested,disabled:box.querySelector('#push-status').textContent}};
 if(before.prompts!==0||result.after.prompts!==1||result.after.state!=='disabled'||result.buttons.some(b=>b.disabled)||result.requests.length!==4)throw new Error('push DOM handlers failed original assertions');
 return result;
}
