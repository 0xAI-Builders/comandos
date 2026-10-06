// Remote parser proof only; no product geometry or screenshot claims.
const sha=async bytes=>Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes))).map(b=>b.toString(16).padStart(2,'0')).join('');
const get=async path=>{const r=await fetch(path,{signal:AbortSignal.timeout(5000)});if(!r.ok)throw new Error(path+' HTTP '+r.status);return r;};
function canonical(html){const root=document.createElement('template');root.innerHTML=html;const walk=(node,pre=false)=>{if(node.nodeType===3){const value=pre?node.data:node.data.replace(/\s+/g,' ');return value.trim()||pre?['text',value]:null;}if(node.nodeType!==1&&node.nodeType!==11)return null;const attrs=node.nodeType===1?Array.from(node.attributes,a=>[a.name,a.value]).sort((a,b)=>a[0].localeCompare(b[0])):[];return[node.nodeType===1?node.localName:'fragment',attrs,Array.from(node.childNodes,n=>walk(n,pre||node.localName==='pre')).filter(Boolean)];};return JSON.stringify(walk(root.content));}
export async function runMarkdownDiff(){
 const manifest=await(await get('/xtask/web/fixtures/markdown/corpus/manifest.json')).json();
 const synthetic=await(await get('/xtask/web/fixtures/b8/corpus.json')).json();
 if(manifest.entries.length!==200||synthetic.cases.length!==200)throw new Error('Expected 200 real-derived and 200 synthetic cases');
 const real=[];for(const row of manifest.entries){const bytes=await(await get('/xtask/web/fixtures/markdown/corpus/'+row.file)).arrayBuffer();if(await sha(bytes)!==row.sha256)throw new Error('Corpus hash mismatch: '+row.file);real.push({id:row.file,text:new TextDecoder().decode(bytes),kind:'real-derived-'+row.kind});}
 const original=fixtureOriginal.createRenderer(markdownit,DOMPurify);const candidate=NewsReader.createRenderer();const differences=[];let first;const deadline=setTimeout(()=>candidate.dispose(),25000);
 try{for(const row of [...synthetic.cases,...real]){const a=original(row.text),b=await candidate(row.text);if(!first)first={a,b};if(canonical(a)!==canonical(b))differences.push({...row,baseline:a,candidate:b});}}finally{clearTimeout(deadline);candidate.dispose();}
 const negativeCalibration={detected:canonical(first.a)!==canonical('<p>broken negative control</p>'+first.b)};
 const fallback=NewsReader.createRenderer(undefined,null);const plain=await fallback('<b>raw</b>');fallback.dispose();if(plain!==fixtureOriginal.createRenderer(markdownit,null)('<b>raw</b>'))throw new Error('Explicit null purifier fallback changed');
 const xss='<img src=x onerror=alert(1)><script>alert(2)</script>';if(DOMPurify.sanitize(xss)===xss)throw new Error('Purifier negative calibration failed');
 const wasmBytes=await(await get(fixtureWasmUrl)).arrayBuffer();
 return{status:differences.length===0&&negativeCalibration.detected?'pass':'fail',count:400,synthetic:200,realDerived:200,provenance:{sourceRows:manifest.source_rows,anonymization:manifest.anonymization,limits:manifest.limits},normalization:'sorted attributes; whitespace collapsed outside pre; pre text exact; all element and attribute values preserved',proof:{actualWasm:true,wasmUrl:fixtureWasmUrl,wasmSha256:await sha(wasmBytes),originalSha256:await sha(await(await get('/dash/news-reader.js')).arrayBuffer()),domPurifySupported:DOMPurify.isSupported},negativeCalibration,differences,visualAcceptance:false};
}
