"""Immutable original AST bodies, with inert filesystem/tmux/widgets/reclock boundaries."""
import ast,json,sys,types,re,math
i=json.load(sys.stdin);tree=ast.parse(open(i['source'],encoding='utf8').read())
names={'open_web_modal','open_chain_modal','open_analytics_modal','reader_layout_state','_side_tabs_from_web','_side_pin_pos','_side_term_apply_share','_side_term_save_share','_side_scroll_by','_side_sync_arrows','_side_tabs_wheel','_side_head_motion','_mosaic_sessions','_chain_modal_msg','_layout_load','_layout_save','_mosaic_open','_init_pane_position','_load_pane_position','_left_fx_run','_tween'}
nodes=[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name in names]
assert {n.name for n in nodes}==names
class Paned:
 def __init__(self):self.position=i.get('position',200)
 def get_property(self,k):return i.get('bounds',[0,10000])[0 if k=='min-position' else 1]
 def get_allocated_height(self):return i.get('total',1000)
 def get_position(self):return self.position
 def set_position(self,n):self.position=n
 def remove(self,*_):pass
 def pack2(self,*_):pass
class Widget:
 def __init__(self,**kw):self.children=[];self.slots=[]
 def __getattr__(self,k):return lambda *_a,**_k:None
 def get_style_context(self):return self
 def pack_start(self,c,*_):self.children.append(c)
 def pack_end(self,c,*_):self.children.append(c)
 def add(self,c):self.children.append(c)
 def get_visible(self):return i.get('visible',True)
 def get_allocated_width(self):return i.get('width',500)
 def attach(self,w,c,r,span,h):self.slots.append([r,c,span])
 def get_position(self):return i.get('position',200)
state=i.get('state',{});paned=Paned();js=[];saved=[];queued=[];grids=[]
def grid():g=Widget();grids.append(g);return g
class File:
 def __enter__(self):return self
 def __exit__(self,*_):pass
 def read(self):return json.dumps(i.get('document',i.get('labels',{})))
 def write(self,s):saved.append(json.loads(s))
ns={'SESSION_RE':re.compile(r'^[A-Za-z0-9_.-]{1,80}$'),'_SIDE':state,'_side_paned':paned,'wv':types.SimpleNamespace(get_zoom_level=lambda:i.get('zoom',1)),
 '_side_term_host':Widget(),'json':json,'TABS_FILE':'inert-tabs','APP_LAYOUT_FILE':'inert-layout','PANE_POSITION_FILE':'inert-position','open':lambda *_:File(),
 'os':types.SimpleNamespace(replace=lambda *_:None),'subprocess':types.SimpleNamespace(run=lambda *a,**k:types.SimpleNamespace(stdout='\n'.join(i.get('sessions',[])))),
 '_layout_save':lambda:saved.append(dict(state)),'_dash_js':js.append,'BASE_URL':'http://127.0.0.1:7337','re':re,'ES':True,
 'Gtk':types.SimpleNamespace(Box=Widget,Grid=grid,Label=Widget,EventBox=Widget,Orientation=types.SimpleNamespace(VERTICAL=0,HORIZONTAL=1)),
 'GLib':types.SimpleNamespace(markup_escape_text=lambda s:s,timeout_add=lambda *a:queued.append(a)),
 '_pane_position_initialized':False,'_MOSAIC':{'on':False},'_icon_btn':lambda *a,**k:Widget(),'_close_extension_shelf':lambda:None,'paned':paned,'_terminal_column':Widget()}
exec(compile(ast.Module(body=nodes,type_ignores=[]),i['source'],'exec'),ns)
op=i['op']
if op=='reader':print(json.dumps(ns['reader_layout_state'](i['open'],i['terminal'])))
elif op=='side-tabs':print(json.dumps(ns['_side_tabs_from_web'](i['tabs'])))
elif op=='pin':print(json.dumps(ns['_side_pin_pos']()))
elif op=='share':ns['_side_term_apply_share']();print(json.dumps(paned.position))
elif op=='save-share':ns['_side_term_save_share']();print(json.dumps({'state':state,'saved':saved}))
elif op=='drag':state['drag']=i['drag'];ns['_side_head_motion'](None,types.SimpleNamespace(y_root=i['y']));print(json.dumps(paned.position))
elif op=='mosaic':print(json.dumps(ns['_mosaic_sessions']()))
elif op=='slots':ns['_mosaic_sessions']=lambda:[('fixture'+str(x),'s'+str(x)) for x in range(i['n'])];ns['_mosaic_open']();print(json.dumps(grids[0].slots))
elif op=='chain':ns['_CHAINMODAL']={'win':Widget()};ns['_chain_modal_msg'](i['message']);print(json.dumps(js))

elif op=='pane-init':ns['_init_pane_position'](None,types.SimpleNamespace(width=i['width']));print(json.dumps(paned.position if ns['_pane_position_initialized'] else None))

elif op=='left-frame':
 class DrawWidget(Widget):
  def connect(self,event,callback):self.draw=callback
 class Cr:
  def __init__(self):self.rects=[];self.alpha=None;self.image=None;self.gradient=None
  def rectangle(self,*a):self.rects.append(list(a))
  def paint_with_alpha(self,a):self.alpha=a
  def set_source(self,g):self.gradient=g
  def __getattr__(self,k):return lambda *_:None
 class Gradient:
  def __init__(self,*a):self.stops=[]
  def add_color_stop_rgba(self,*a):self.stops.append(list(a))
 da=DrawWidget();cr=Cr()
 ns.update(_left_fx_clear=lambda:None,_animations_enabled=lambda:True,THEME={},_LEFT_FX={},LEFT_FX_MS=220,
  Gdk=types.SimpleNamespace(RGBA=lambda:types.SimpleNamespace(parse=lambda *_:None,red=0,green=0,blue=0),cairo_set_source_pixbuf=lambda cr,img,x,y:setattr(cr,'image',[x,y])),
  cairo=types.SimpleNamespace(LinearGradient=Gradient),Gtk=types.SimpleNamespace(DrawingArea=lambda:da,Align=types.SimpleNamespace(FILL=0)),modal_overlay=Widget(),
  _tween=lambda w,step,*a,**kw:step(i['progress']))
 ns['_left_fx_run'](i['hiding'],['snap',i['x'],0, i['width'],500],i['width']);da.draw(da,cr)
 k=i['progress'] if i['hiding'] else 1-i['progress']
 print(json.dumps({'offset':cr.image[0]-i['x'],'mask_start':None if i['hiding'] else float(cr.rects[0][0]), 'alpha':cr.alpha,'edge':i['x']+i['width']+cr.image[0]-i['x'], 'shadow_alpha':cr.gradient.stops[0][-1] if cr.gradient else None}))

elif op=='wheel':
 ns['_side_scroll_by']=lambda step:saved.append(step)
 ns['Gdk']=types.SimpleNamespace(ScrollDirection=types.SimpleNamespace(UP='up',LEFT='left',DOWN='down',RIGHT='right'))
 ns['_side_tabs_wheel'](None,types.SimpleNamespace(get_scroll_deltas=lambda:(i['discrete'] is None,i['dx'],i['dy']),direction='up' if i['discrete']==-1 else 'down'))
 print(json.dumps(float(saved[-1])))

elif op=='tween':
 class TweenWidget:
  def add_tick_callback(self,callback):self.tick=callback
 widget=TweenWidget();values=[];ns['_tween'](widget,values.append,1.,0.,ms=i['millis'])
 for now in i['times']:widget.tick(widget,types.SimpleNamespace(get_frame_time=lambda:now))
 print(json.dumps(values))

elif op=='modal-size':
 from urllib.parse import urlencode
 class Dialog(Widget):
  def set_default_size(self,w,h):self.size=[w,h]
 d=Dialog()
 ns.update(win=types.SimpleNamespace(get_size=lambda:(i['width'],i['height'])),
  _WEBMODAL={'win':d,'wv':Widget(),'title':Widget()},_CHAINMODAL={'win':d,'wv':Widget()},_ANMODAL={'win':d,'wv':Widget()},
  Gtk=types.SimpleNamespace(WindowPosition=types.SimpleNamespace(CENTER_ON_PARENT=0)),
  _cur_page=lambda:None,_cur_pane_id=lambda _: '',term_session=lambda _: '',urlencode=urlencode,_DASH_V='private')
 if i['kind']=='url':ns['open_web_modal']('http://127.0.0.1:7337/','owned')
 elif i['kind']=='chains':ns['open_chain_modal']()
 else:ns['open_analytics_modal']()
 print(json.dumps(d.size))
elif op=='modal-close':
 parent=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='show_modal_panel')
 nodes=[n for n in parent.body if isinstance(n,ast.FunctionDef) and n.name in {'_on_backdrop_press','_key_handler'}]
 closed=[]
 ns.update(dismissable=i['dismissable'],on_key=None,_close=lambda:closed.append(1),Gdk=types.SimpleNamespace(KEY_Escape=27))
 exec(compile(ast.Module(body=nodes,type_ignores=[]),i['source'],'exec'),ns)
 consumed=ns['_on_backdrop_press']() if i['backdrop'] else ns['_key_handler'](None,types.SimpleNamespace(keyval=27 if i['escape'] else 65))
 print(json.dumps({'closed':bool(closed),'consumed':consumed}))
