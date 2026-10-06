"""Execute immutable original function bodies with inert boundary adapters."""
import ast,json,sys,types
from urllib.parse import urlencode,urlparse
i=json.load(sys.stdin)
tree=ast.parse(open(i['source'],encoding='utf8').read())
names={'_account_bar_class','_acct_awaiting','_wait_account_switch','_pane_frames','_shell_pill_xy','_pill_row_y','_card_rect','_gutter_neighbor','_gutter_target','_gutter_half','_grip_rect','_shelf_height','_shelf_separator_css','_extension_message','_pane_pill','_motor_badge','_esc','open_ai_session_here'}
selected=[]
for n in tree.body:
    if isinstance(n,ast.FunctionDef) and n.name in names:selected.append(n)
    if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id in ('_ACCT_FINAL','_ACCT_STAGES','HEADER_EXTRA','GRIP_SLACK','_SHELF_HANDLE','_SHELF_MIN','_SHELF_TERMINAL_MIN','_PV_HEX','_PV_ICON','_STATE_UI','MOTOR_LOGO','_BADGE_CSS','PALETTE','PAL_DIA','PAL_CALIDO','PAL_BRUNO','PAL_UBUNTU','THEMES') for target in n.targets for t in ast.walk(target)):selected.append(n)
if {n.name for n in selected if isinstance(n,ast.FunctionDef)}!=names:raise RuntimeError('original functions absent')
delays=[];notes=[];calls=[];index=0
class Response:
    def __init__(self,data):self.data=data
    def __enter__(self):return self
    def __exit__(self,*_):return False
    def read(self):return json.dumps(self.data).encode()
def fake_urlopen(url,timeout):
    global index
    calls.append([url,timeout]);v=i['statuses'][min(index,len(i['statuses'])-1)];index+=1
    if v is None:raise OSError('inert transport failure')
    return Response(v)
ns={'ES':not i.get('english',False),'BASE_URL':'http://127.0.0.1:7337','urlencode':urlencode,'urlparse':urlparse,'json':json,
    'time':types.SimpleNamespace(sleep=lambda n:delays.append(n)),
    'urllib':types.SimpleNamespace(request=types.SimpleNamespace(urlopen=fake_urlopen))}
exec(compile(ast.Module(body=selected,type_ignores=[]),i['source'],'exec'),ns)
if i['op']=='account':
    result=ns['_wait_account_switch']('fixture|%7','fixture-id','fixture',notes.append,attempts=i.get('attempts',120))
    print(json.dumps({'result':result,'notes':notes,'delays':delays,'calls':calls}))
elif i['op']=='quota': print(json.dumps([ns['_account_bar_class'](p) for p in i['percents']]))
elif i['op']=='geometry':
    geo={p['id']:(p['left'],p['top'],p['width']) for p in i['panes']}
    ns['_PANE_BOX']={p['id']:(p['height'],p['id'] in i['active']) for p in i['panes']}
    ns['_PANE_GUTTERS']={};ns['_PANE_GEO']={}
    term=types.SimpleNamespace(get_row_count=lambda:i['grid'][1],get_column_count=lambda:i['grid'][0])
    g=i['geom'];frames,rows=ns['_pane_frames'](term,geo,g['origin_x'],g['origin_y'],g['cell_w'],g['cell_h'],i['focused'])
    gutters=ns['_PANE_GUTTERS'][id(term)]
    pills={p['id']:ns['_shell_pill_xy'](rows,p['id'],g['origin_x'],g['origin_y'],p['left'],p['top'],g['cell_w'],g['cell_h']) for p in i['panes']}
    cards={pid:ns['_card_rect'](rows,pid,g['cell_h']) for pid in rows}
    print(json.dumps({'frames':frames,'rows':rows,'gutters':gutters,'grips':[ns['_grip_rect'](g) for g in gutters],'pills':pills,'cards':cards}))
elif i['op']=='gutter':
    geo={p['id']:(p['left'],p['top'],p['width']) for p in i['panes']};heights={p['id']:p['height'] for p in i['panes']}
    pane=i['pane'];orient=i['orientation'];nb=ns['_gutter_neighbor'](geo,heights,pane,orient)
    print(json.dumps({'neighbor':nb,'target':ns['_gutter_target'](geo,heights,pane,orient,i['cell'],nb),'half':ns['_gutter_half'](geo,heights,pane,orient,nb) if nb else None}))

elif i['op']=='shelf':
    css=[]
    for theme in ['noche','dia','calido','termius','bruno','superglass','neon','contraste','ubuntu']:
        ns['THEME']=ns['THEMES'][theme];css.append(ns['_shelf_separator_css']().decode())
    print(json.dumps({'heights':[ns['_shelf_height'](a,b) for a,b in i['cases']], 'css':css}))
elif i['op']=='extension-message':
    closes=[]
    ns['_EXTENSION_SHELF']={'view':types.SimpleNamespace(get_uri=lambda:i['uri'])}
    ns['_close_extension_shelf']=lambda:closes.append(1)
    ns['_extension_message'](None,types.SimpleNamespace(get_js_value=lambda:types.SimpleNamespace(to_string=lambda:i['raw'])))
    print(json.dumps({'closes':len(closes)}))

elif i['op']=='cards':
    colors={}
    class Widget:
        def __init__(self,kind,**kwargs):
            self.kind=kind;self.classes=[];self.properties=dict(kwargs);self.children=[]
        def get_style_context(self):return self
        def add_class(self,name):self.classes.append(name)
        def pack_start(self,child,*_):self.children.append(child)
        def add(self,child):self.children.append(child)
        def set_markup(self,v):self.properties['markup']=v
        def set_valign(self,v):self.properties['valign']=v
        def set_halign(self,v):self.properties['halign']=v
        def set_ellipsize(self,v):self.properties['ellipsize']=v
        def set_max_width_chars(self,v):self.properties['max_width_chars']=v
        def set_tooltip_text(self,v):self.properties['tooltip']=v
        def result(self):
            for cls in self.classes:
                if cls.startswith('pp-logo-') and cls in colors:self.properties['color']=colors[cls]
            return {'kind':self.kind,'classes':self.classes,'properties':self.properties,'children':[c.result() for c in self.children]}
    class Css:
        def load_from_data(self,v):
            s=v.decode();colors[s.split('.pp-logo.',1)[1].split('{',1)[0]]=s.split('background-color:',1)[1].split(';',1)[0]
    ns['Gtk']=types.SimpleNamespace(Box=lambda **kw:Widget('Box',**kw),Label=lambda **kw:Widget('Label',**kw),Separator=lambda **kw:Widget('Separator',**kw),EventBox=lambda **kw:Widget('EventBox',**kw),CssProvider=Css,
        Align=types.SimpleNamespace(START='Start',CENTER='Center'),Orientation=types.SimpleNamespace(HORIZONTAL='Horizontal',VERTICAL='Vertical'),
        STYLE_PROVIDER_PRIORITY_APPLICATION=600,StyleContext=types.SimpleNamespace(add_provider_for_screen=lambda *_:None))
    ns['Pango']=types.SimpleNamespace(EllipsizeMode=types.SimpleNamespace(END='End'))
    ns['Gdk']=types.SimpleNamespace(Screen=types.SimpleNamespace(get_default=lambda:None))
    ns['GLib']=types.SimpleNamespace(markup_escape_text=lambda s:str(s).replace('&','&amp;').replace('<','&lt;').replace('>','&gt;').replace('"','&quot;').replace("'",'&apos;'))
    ns['_svg_image']=lambda name,size,color:Widget('SVG',icon=name,size=size,color=color)
    ns['_pane_card_ai']=lambda sess:None
    out=[]
    for item in i['cases']:
        ns['THEME']={'fg':item['fg']};ns['tabs']={'fixture':types.SimpleNamespace(_label=item['title'])}
        out.append(ns['_pane_pill']('fixture',item['pane'],False).result())
    print(json.dumps(out))

elif i['op']=='wizard':
    scripts=[];reads=[]
    def tmuxc(*args):
        reads.append(args)
        return types.SimpleNamespace(stdout=i['cwd'] if args[-1]=='#{pane_current_path}' else i['session'])
    ns['tmuxc']=tmuxc;ns['_dash_js']=scripts.append;ns['notify_popup']=lambda *_:None
    ns['open_ai_session_here'](i['session'],i['pane'])
    print(json.dumps({'scripts':scripts,'reads':reads}))
