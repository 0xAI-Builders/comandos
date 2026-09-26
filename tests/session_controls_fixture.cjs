// Read-only app shell + simulated APIs for remote browser checks. Never contacts tmux.
const http=require('node:http'),fs=require('node:fs'),path=require('node:path');
const root=path.resolve(__dirname,'..');
const registry={harnesses:{codex:{label:'Codex',accounts:[{alias:'main',selectable:true,motorSelectable:true},{alias:'work',selectable:true,motorSelectable:true},{alias:'expired',selectable:false}]},claude:{label:'Claude Code',accounts:[{alias:'main',selectable:true,motorSelectable:true}]},grok:{label:'Grok Build',accounts:[{alias:'main',selectable:true,motorSelectable:true}]}},motors:{codex:{label:'OpenAI',models:[{id:'gpt-6-astra',name:'GPT-6 Astra',efforts:['low','medium','high','xhigh','max','ultra']},{id:'gpt-6-sol',name:'GPT-6 Sol',efforts:['low','medium','high','xhigh','max','ultra']},{id:'future',name:'Future model',efforts:['high'],soon:true}]},claude:{label:'Anthropic',models:[{id:'opus',name:'Opus',efforts:['low','high']}]},grok:{label:'xAI',models:[{id:'grok-4.7',name:'Grok 4.7',efforts:['low','high']}]}},matrix:[{id:'codex:codex',harness:'codex',motor:'codex',selectable:true},{id:'claude:claude',harness:'claude',motor:'claude',selectable:true},{id:'grok:grok',harness:'grok',motor:'grok',selectable:true}]};
const source={session:'fixture',pane:'%7',project:'ComandOS · prueba aislada',cwd:'/tmp',agent:'codex',motor:'codex',model:'gpt-6-astra',effort:'high',account:'main',harnessAccount:'main',motorAccount:'main',alive:true,status:'idle',observedConfig:{identity:{pid:42},conversationId:'fixture-source'}};
let items=[{...source}],posts=[],operation=null,sequence=0;
const config={toHarness:'codex',motor:'codex',model:'gpt-6-sol',effort:'medium',harnessAccount:'main',motorAccount:'main'};
const server=http.createServer((req,res)=>{
  const url=new URL(req.url,'http://fixture');let filename=path.join(root,url.pathname.startsWith('/assets/')?'':'dash',url.pathname==='/'?'index.html':url.pathname);
  if(filename.startsWith(root+path.sep)&&fs.existsSync(filename)&&fs.statSync(filename).isFile()){
    const ext=path.extname(filename);res.setHeader('Content-Type',({'.js':'text/javascript','.css':'text/css','.html':'text/html','.svg':'image/svg+xml','.ttf':'font/ttf','.png':'image/png'})[ext]||'application/octet-stream');
    let data=fs.readFileSync(filename);if(ext==='.html')data=Buffer.from('<script>window.fixtureErrors=[];addEventListener("error",e=>fixtureErrors.push(e.message));addEventListener("unhandledrejection",e=>fixtureErrors.push(String(e.reason)));</script>'+data);res.end(data);return;
  }
  let raw='';req.on('data',d=>raw+=d);req.on('end',()=>{
    let data={};try{data=raw?JSON.parse(raw):{}}catch(e){res.writeHead(400);res.end('{}');return;}
    let out={};if(req.method==='POST'&&!url.pathname.startsWith('/__fixture'))posts.push({path:url.pathname,data});
    if(url.pathname==='/__fixture'){
      if(data.action==='reset'){items=[structuredClone(source)];posts=[];operation=null;}
      if(data.action==='confirm'&&operation){const d=posts.filter(p=>p.path==='/session/configure').at(-1).data;operation={...operation,state:'confirmed',ok:true,detail:'confirmado',harness:d.toHarness,motor:d.motor,model:d.model,effort:d.effort,harnessAccount:d.harnessAccount,motorAccount:d.motorAccount,identity:{pid:84},conversationId:'fixture-destination',ts:Date.now()};delete operation.stage;delete operation.stageCode;items[0]={...items[0],agent:d.toHarness,motor:d.motor,model:d.model,effort:d.effort,account:d.harnessAccount,harnessAccount:d.harnessAccount,motorAccount:d.motorAccount,observedConfig:{identity:{pid:84},conversationId:'fixture-destination'}};}
      if(data.action==='fail'&&operation){operation={...operation,ok:false,state:'failed',detail:'Fallo de prueba',ts:Date.now()};delete operation.stage;delete operation.stageCode;}
      out={posts,operation,registry,items};
    }
    if(url.pathname==='/state')out=items;
    if(url.pathname==='/providers')out=registry;
    if(url.pathname==='/model/status')out=operation||{};
    if(url.pathname==='/session-config-history')out={items:[{config,count:8,lastUsed:1}],previous:{config:{...config,model:'gpt-6-astra',effort:'high'},count:1,lastUsed:1},scope:'project',provenance:'confirmed-operations'};
    if(url.pathname==='/session/configure'){operation={operationId:'fixture-operation-'+(++sequence),operationKey:'fixture|%7',state:'waiting',stage:'waiting',stageCode:'waiting',ts:Date.now()};out={...operation,pending:true,queued:true};}
    if(url.pathname==='/model/switch-cancel'){operation=null;out={ok:true};}
    if(['/tabs','/tab-history','/events','/ssh'].includes(url.pathname))out=[];
    if(url.pathname==='/operator')out={id:'fixture-chat',messages:[],conversations:[],models:[]};
    if(url.pathname==='/fs/dirs')out={dirs:[],path:'/tmp'};
    if(url.pathname==='/session-profiles')out={profiles:[]};
    res.setHeader('Content-Type','application/json');res.end(JSON.stringify(out));
  });
});
server.listen(Number(process.env.FIXTURE_PORT||4794),'127.0.0.1',()=>console.log('Fixture ready on '+server.address().port));
