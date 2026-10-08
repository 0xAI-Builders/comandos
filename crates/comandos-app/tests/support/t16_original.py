"""Execute selected original AST bodies; never import cc-app or run its startup."""
import ast, json, pathlib, types, re, sys
p=json.load(sys.stdin)
source=pathlib.Path(p['source']).read_text()
tree=ast.parse(source)
ns={'json':json,'re':re,'ES':p.get('es',False),'time':types.SimpleNamespace(time=lambda:p.get('now',0)),
    'secrets':types.SimpleNamespace(token_hex=lambda _:p.get('new_id','fixture'))}
def load(names):
    nodes=[n for n in tree.body if isinstance(n,(ast.FunctionDef,ast.ClassDef)) and n.name in names]
    assert len(nodes)==len(names)
    exec(compile(ast.Module(body=nodes,type_ignores=[]),p['source'],'exec'),ns)
op=p['op']
if op=='filter':
    node=next(n for n in tree.body if isinstance(n,ast.ClassDef) and n.name=='SnippetsDialog')
    methods=[n for n in node.body if isinstance(n,ast.FunctionDef) and n.name in ('_filter','_save_from_editor','_delete')]
    node=ast.ClassDef(name=node.name,bases=[],keywords=[],body=methods,decorator_list=[])
    ast.fix_missing_locations(node);exec(compile(ast.Module(body=[node],type_ignores=[]),p['source'],'exec'),ns)
    d=ns['SnippetsDialog']();d._items=p['items'];d._q=types.SimpleNamespace(get_text=lambda:p['query'])
    print(json.dumps([d._items.index(x) for x in d._filter(d._items,p['query'])]));
elif op=='crud':
    node=next(n for n in tree.body if isinstance(n,ast.ClassDef) and n.name=='SnippetsDialog')
    node=ast.ClassDef(name=node.name,bases=[],keywords=[],body=[n for n in node.body if isinstance(n,ast.FunctionDef) and n.name in ('_save_from_editor','_delete')],decorator_list=[])
    ast.fix_missing_locations(node);exec(compile(ast.Module(body=[node],type_ignores=[]),p['source'],'exec'),ns)
    ns['snip_save']=lambda items:None;d=ns['SnippetsDialog']();d._items=p['items'];d._render_list=lambda:None;d._render_preview=lambda:None
    item=next((x for x in d._items if x.get('id')==p.get('id')),None)
    if p.get('delete'):d._delete(item)
    else:
        entry=lambda s:types.SimpleNamespace(get_text=lambda:s)
        buf=types.SimpleNamespace(get_start_iter=lambda:None,get_end_iter=lambda:None,get_text=lambda *_:p['body'])
        d._save_from_editor(item,entry(p['name']),entry(p['tags']),types.SimpleNamespace(get_buffer=lambda:buf))
    print(json.dumps(d._items))
elif op=='paste':
    calls=[]
    def run(args,**kw):calls.append([args,kw.get('input')]);return types.SimpleNamespace(returncode=0,stderr='')
    ns['subprocess']=types.SimpleNamespace(run=run);load(['snip_paste']);err=ns['snip_paste'](p['session'],p['body']);print(json.dumps({'calls':calls,'error':err}))
elif op=='linkrows':
    import os,html
    labels=[];icons=[]
    class Widget:
        def __init__(self,*a,**kw):
            if kw.get('label') is not None:labels.append(kw['label'])
        def __getattr__(self,name):return lambda *a,**k: self if name=='get_style_context' else None
    ns.update(os=os,Gtk=types.SimpleNamespace(Popover=Widget,Box=Widget,Label=Widget,Button=Widget,Separator=Widget,ReliefStyle=types.SimpleNamespace(NONE=0),Orientation=types.SimpleNamespace(VERTICAL=0,HORIZONTAL=1),PositionType=types.SimpleNamespace(BOTTOM=0)),Gdk=types.SimpleNamespace(Rectangle=lambda:types.SimpleNamespace()),GLib=types.SimpleNamespace(markup_escape_text=html.escape),Pango=types.SimpleNamespace(EllipsizeMode=types.SimpleNamespace(MIDDLE=0)),_themed_icon_image=lambda icon,size:icons.append([icon,size]),copy_text_to_clipboard=lambda _:None)
    load(['_clean_local_path','link_actions_popover']);ns['link_actions_popover'](None,types.SimpleNamespace(x=1,y=2,time=0),p['url']);print(json.dumps({'labels':labels,'icons':icons}))
elif op=='notify':
    payload=[]
    ns.update(threading=types.SimpleNamespace(Thread=lambda target,**_:types.SimpleNamespace(start=target)),urllib=types.SimpleNamespace(request=types.SimpleNamespace(Request=lambda url,**kw:payload.append([url,json.loads(kw['data'])]),urlopen=lambda *a,**k:None)))
    load(['notify_popup']);ns['notify_popup'](p['title'],p['body']);print(json.dumps(payload))
elif op=='path':
    import os
    ns['os']=os;load(['_clean_local_path']);print(json.dumps(ns['_clean_local_path'](p['url'])))
elif op=='reply':
    sent=[]
    class Response:
        def __enter__(self):return self
        def __exit__(self,*_):pass
        def read(self):return json.dumps(p['items']).encode()
    ns.update(Gtk=types.SimpleNamespace(Clipboard=types.SimpleNamespace(get=lambda _:types.SimpleNamespace(set_text=lambda txt,_:sent.append(['clipboard',txt])))),Gdk=types.SimpleNamespace(SELECTION_CLIPBOARD=0),GLib=types.SimpleNamespace(idle_add=lambda fn,*args:fn(*args)),BASE_URL='http://private.invalid',urllib=types.SimpleNamespace(request=types.SimpleNamespace(urlopen=lambda *_a,**_k:Response())),threading=types.SimpleNamespace(Thread=lambda target,**_:types.SimpleNamespace(start=target)),term_session=lambda _:p['session'],notify_popup=lambda title,body:sent.append(['notify',title,body]))
    load(['copy_claude_reply']);ns['copy_claude_reply'](None);print(json.dumps(sent))
elif op=='auto':
    lib=ast.parse(pathlib.Path(p['lib']).read_text());nodes=[n for n in lib.body if isinstance(n,(ast.FunctionDef,ast.ClassDef)) or isinstance(n,ast.Assign)]
    exec(compile(ast.Module(body=nodes,type_ignores=[]),p['lib'],'exec'),ns)
    b=ns['Bridge']();print(json.dumps({'newest':[ns['newest_auto'](x) for x in p['listings']], 'poll':[b.poll(x) for x in p['listings']]}))
elif op=='release':
    calls=[]
    ns.update(copy_vte_selection=lambda _:None,tmuxc=lambda *args:calls.append(list(args)),link_actions_popover=lambda _t,_e,url:calls.append(['link',url]))
    load(['on_term_release'])
    term=types.SimpleNamespace(_primary_press=(10.,20.,p['url'],'%1'),get_has_selection=lambda:False)
    ns['on_term_release'](term,types.SimpleNamespace(button=p['button'],x=p['x'],y=p['y']))
    print(json.dumps(calls))
elif op=='pane':
    ns['term_session']=lambda _: 'fixture'
    ns['tmuxc']=lambda *_:types.SimpleNamespace(stdout=p['listing'])
    load(['pane_at'])
    term=types.SimpleNamespace(get_char_width=lambda:10,get_char_height=lambda:20)
    event=types.SimpleNamespace(x=10+p['col']*10,y=8+p['row']*20)
    print(json.dumps(ns['pane_at'](term,event)))
elif op=='menu':
    class Menu:
        def __init__(self):self.rows=[]
        def append(self,x):self.rows.append(x)
        def show_all(self):pass
        def popup_at_pointer(self,_):pass
    class Item:
        def __init__(self,label=None):self.label=label;self.sensitive=True;self.submenu=None
        def connect(self,*_):pass
        def set_sensitive(self,v):self.sensitive=v
        def set_submenu(self,v):self.submenu=v
    ns['Gtk']=types.SimpleNamespace(Menu=Menu,MenuItem=Item,SeparatorMenuItem=lambda:Item())
    ns['Gdk']=types.SimpleNamespace(ModifierType=types.SimpleNamespace(CONTROL_MASK=4))
    ns['term_session']=lambda _:p.get('session','fixture')
    ns['pane_at']=lambda *_:('%1',2)
    ns['copy_mode_pane']=lambda *_:None
    ns['tmuxc']=lambda *args:types.SimpleNamespace(returncode=0,stdout='fixture\n' if args[0]=='list-sessions' else '')
    ns['append_work_mark_menu']=lambda *_:None
    for name in ('new_local_tab','new_session_dialog'):ns[name]=lambda:None
    load(['ssh_host_from_session','on_term_button'])
    term=types.SimpleNamespace(get_parent=lambda:types.SimpleNamespace(_key='fixture'))
    ns['on_term_button'](term,types.SimpleNamespace(button=3,state=0))
    print(json.dumps([{'label':x.label,'enabled':x.sensitive} for x in term._menu.rows if x.label is not None]))
elif op=='snipcss':
    ns['THEME']=p['theme'];load(['_build_hb_css']);css=ns['_build_hb_css']().decode();a=css.index('.cc-snip-dialog');b=css.index('.cc-snip-dialog .cc-snip-close:hover',a);b=css.index('}',b)+1;print(json.dumps(css[a:b]))
else:raise ValueError(op)
