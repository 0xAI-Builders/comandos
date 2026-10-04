"""Model boundaries shared by the executable tests and read-only SDK oracle."""
import copy,json

INITIAL={'protocolVersion':'2025-03-26','capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}}
TOOL={'name':'tool','inputSchema':{}}
TEXT={'type':'text','text':'ok'}
CASES=[]

def case(label,method,path,value,accepted=True,phase=None):
    replies=copy.deepcopy({'initialize':INITIAL,'tools/list':{'tools':[TOOL]},'tools/call':{'content':[TEXT]}})
    target=replies[method]
    for key in path[:-1]:target=target[key]
    target[path[-1]]=value
    expected={'name':'google-drive','status':'connected','tools':1} if accepted else {'name':'google-drive','status':'failed','errors':['ValidationError']}
    if phase:expected={'name':'google-drive','status':'failed','phase':phase,'tools':1}
    CASES.append((label,replies,expected))

case('experimental-listChanged','initialize',['capabilities','experimental'],{'listChanged':{'vendor':'value'}})
case('custom-subscribe','initialize',['capabilities','vendor'],{'subscribe':'supported'})
case('coerced-tools-listChanged','initialize',['capabilities','tools'],{'listChanged':1})
case('bad-description','tools/list',['tools',0,'description'],[],False)
case('bad-resource-link-uri','tools/call',['content'],[{'type':'resource_link','uri':'not a URI','name':'example'}],False)
case('bad-audience','tools/call',['content',0,'annotations'],{'audience':['invalid-role']},False)

# All modeled fields are checked; extras are unconstrained and nullable fields
# stay nullable. These are protocol models, not JSON Schema validation of tools.
for field in ['instructions','_meta']:
    for v,ok in [(None,True),([] ,False),('text',field=='instructions'),({},field=='_meta')]:
        case(f'init-{field}-{v!r}','initialize',[field],v,ok)
for field in ['title','websiteUrl','version','name']:
    for v,ok in [('plain text',True),([],False),(None,field in ['title','websiteUrl'])]:
        case(f'server-{field}-{v!r}','initialize',['serverInfo',field],v,ok)
for cap in ['tools','prompts','resources','logging','completions']:
    case(f'{cap}-extra-flags','initialize',['capabilities',cap],{'subscribe':{'vendor':1}} if cap!='resources' else {'vendor':[]})
    case(f'{cap}-wrong-shape','initialize',['capabilities',cap],[],False)
    case(f'{cap}-null','initialize',['capabilities',cap],None)
    if cap=='tools':CASES[-1][2]['tools']=0
for cap,flags in [('tools',['listChanged']),('prompts',['listChanged']),('resources',['listChanged','subscribe'])]:
    for flag in flags:
        for v,ok in [(None,True),(0,True),(1.0,True),('YeS',True),('OFF',True),('t',True),('n',True),(2,False),('true ',False),({},False)]:
            case(f'{cap}-{flag}-{v!r}','initialize',['capabilities',cap],{flag:v},ok)
for v,ok in [({'custom':{}},True),({'custom':[]},False),({'custom':1},False),(None,True)]:
    case(f'experimental-{v!r}','initialize',['capabilities','experimental'],v,ok)
for v,ok in [({'requests':{'tools':{'call':{}}},'list':{},'cancel':{}},True),({'requests':{'tools':{'call':False}}},False),({'list':[]},False),({'cancel':'yes'},False),({'requests':{'tools':[]}},False),({'requests':[]},False),(None,True)]:
    case(f'tasks-{v!r}','initialize',['capabilities','tasks'],v,ok)
for field in ['title','description','outputSchema','_meta']:
    for v,ok in [(None,True),([],False),('text',field in ['title','description']),({},field in ['outputSchema','_meta'])]:
        case(f'tool-{field}-{v!r}','tools/list',['tools',0,field],v,ok)
for field in ['name','inputSchema']:
    case(f'tool-required-{field}','tools/list',['tools',0,field],None,False)
for field in ['readOnlyHint','destructiveHint','idempotentHint','openWorldHint']:
    for v,ok in [('false',True),(1,True),(None,True),([],False)]:
        case(f'tool-annotation-{field}-{v!r}','tools/list',['tools',0,'annotations'],{field:v,'vendor':[]},ok)
case('tool-annotation-title','tools/list',['tools',0,'annotations'],{'title':3},False)
for v,ok in [('required',True),('optional',True),('forbidden',True),(None,True),('invalid',False)]:
    case(f'execution-{v!r}','tools/list',['tools',0,'execution'],{'taskSupport':v,'vendor':[]},ok)
for method,path in [('initialize',['serverInfo']),('tools/list',['tools',0]),('tools/call',['content',0])]:
    for icons,ok in [(None,True),([{'src':'not a URI','sizes':['48x48'],'vendor':[]}],True),([{'src':[]}],False),([{'src':'x','mimeType':1}],False),([{'src':'x','sizes':[1]}],False),({},False)]:
        # Only resource_link has icons; text's icons are an unconstrained extra.
        if method=='tools/call':continue
        case(f'{method}-icons-{icons!r}',method,path+['icons'],icons,ok)
for method in ['tools/list','tools/call']:
    for v,ok in [(None,True),({},True),([],False)]:case(f'{method}-meta-{v!r}',method,['_meta'],v,ok)
for v,ok in [(None,True),('',True),(1,False),([],False)]:case(f'cursor-{v!r}','tools/list',['nextCursor'],v,ok)
for v,ok,phase in [(False,True,None),('false',True,None),(0,True,None),('YES',True,'read_access'),(1.0,True,'read_access'),(None,False,None),([],False,None)]:
    case(f'isError-{v!r}','tools/call',['isError'],v,ok,phase)
for v,ok in [(None,True),({},True),([],False)]:case(f'structured-{v!r}','tools/call',['structuredContent'],v,ok)

CONTENTS=[TEXT,{'type':'image','data':'not validated base64','mimeType':'image/example'}, {'type':'audio','data':'','mimeType':'audio/example'}, {'type':'resource_link','uri':'urn:example:test','name':'example'}, {'type':'resource','resource':{'uri':'file:///tmp/a','text':'ok'}}]
for content in CONTENTS:
    kind=content['type']
    for annotations,ok in [(None,True),({'audience':['user','assistant'],'priority':'0.5','vendor':[]},True),({'audience':[],'priority':True},True),({'audience':['system']},False),({'audience':'user'},False),({'priority':-0.1},False),({'priority':'NaN'},False),({'priority':1.1},False),([],False)]:
        case(f'{kind}-annotations-{annotations!r}','tools/call',['content'],[{**content,'annotations':annotations}],ok)
    case(f'{kind}-bad-meta','tools/call',['content'],[{**content,'_meta':[]}],False)
    case(f'{kind}-extras','tools/call',['content'],[{**content,'vendor':{'subscribe':[]},'annotations':{'lastModified':[]}}])
    for field in ['text'] if kind=='text' else ['data','mimeType'] if kind in ['image','audio'] else []:
        case(f'{kind}-bad-{field}','tools/call',['content'],[{**content,field:[]}],False)
for uri,ok in [('custom:',True),('https:foo',True),('mailto:x@y.test',True),('https://x/space here',True),('\nhttps://x\t',True),('relative/path',False),('',False),('https://',False),('http://exa mple.test',False),('https://x:99999',False)]:
    for content in CONTENTS[3:]:
        c=copy.deepcopy(content)
        (c if c['type']=='resource_link' else c['resource'])['uri']=uri
        case(f'{c["type"]}-uri-{uri!r}','tools/call',['content'],[c],ok)
for field in ['title','description','mimeType','size','icons']:
    for v,ok in [(None,True),([],field=='icons')]:
        case(f'resource-link-{field}-{v!r}','tools/call',['content'],[{**CONTENTS[3],field:v}],ok)
for v,ok in [('1',True),('1.0',True),(True,True),(1.0,True),(-1,True),(1.5,False),('1e0',False)]:
    case(f'resource-link-size-{v!r}','tools/call',['content'],[{**CONTENTS[3],'size':v}],ok)
for resource,ok in [({'uri':'x:y','blob':'not base64'},True),({'uri':'x:y','text':[],'blob':'fallback'},True),({'uri':'x:y','text':[]},False),({'uri':'x:y','text':'ok','mimeType':[]},False),({'uri':'x:y','blob':'ok','_meta':[]},False)]:
    case(f'embedded-{resource!r}','tools/call',['content'],[{'type':'resource','resource':resource}],ok)
case('all-content-types','tools/call',['content'],CONTENTS)
case('unknown-content-type','tools/call',['content'],[{'type':'vendor','text':'ok'}],False)

def entry(replies,fixture_path,executable):
    return {'command':executable,'args':[fixture_path,'--fixture','protocol'],'env':{'PROTOCOL_REPLIES':json.dumps(replies)}}

for v,ok in [('++1',False),('--1',False),('+-1',False),('1_0',True),('1_0.0_0',False),('1.',False),('1.00',True),('1_0.0',True),('１２',False),('0.000',True),('  +1  ',True)]:
    case(f'extended-size-{v!r}','tools/call',['content'],[{**CONTENTS[3],'size':v}],ok)
for v,ok in [('0_0.5',True),('.5',True),('1e-1',True),('1__0',False),('Infinity',False),('0.5_',False)]:
    case(f'extended-priority-{v!r}','tools/call',['content',0,'annotations'],{'priority':v},ok)
for method,path in [('initialize',['serverInfo']),('tools/list',['tools',0]),('tools/call',['content',0])]:
    case(f'extended-{method}-wrong-shape',method,path,[],False)
for field in ['execution','annotations']:
    case(f'extended-tool-{field}-wrong-shape','tools/list',['tools',0,field],[],False)
for v,ok in [(None,False),([],False),(7,True),(7.0,True),(True,True),('2099-01-01',True)]:
    case(f'extended-version-{v!r}','initialize',['protocolVersion'],v,False)
    if ok:CASES[-1][2]['errors']=['RuntimeError']

for v,ok in [(10**400,True),('1_.0',False),('_1.0',False),('1__0.0',False),('1_0.00',True),('1_0.0_0',False),('1.0000000000000000001',False)]:
    case(f'number-boundary-{str(v)[:24]}','tools/call',['content'],[{**CONTENTS[3],'size':v}],ok)

case('capabilities-wrong-shape','initialize',['capabilities'],None,False)
case('tools-array-wrong-shape','tools/list',['tools'],None,False)
case('content-array-wrong-shape','tools/call',['content'],{},False)
case('resource-link-name-wrong-shape','tools/call',['content'],[{**CONTENTS[3],'name':[]}],False)
case('resource-link-icons-invalid','tools/call',['content'],[{**CONTENTS[3],'icons':[{'src':'x','sizes':[1]}]}],False)
case('resource-link-icons-valid','tools/call',['content'],[{**CONTENTS[3],'icons':[{'src':'x','sizes':['48x48']}]}])
