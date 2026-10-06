"""Ejecuta únicamente AST de funciones originales con colaboradores sintéticos."""
import ast,json,sys,types,os
source=sys.argv[1];mode=sys.argv[2];tree=ast.parse(open(source).read());data=json.load(sys.stdin)
def load(names,ns):
 for n in tree.body:
  if isinstance(n,(ast.FunctionDef,ast.Assign)) and ((isinstance(n,ast.FunctionDef) and n.name in names) or (isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id in names for t in n.targets))):exec(compile(ast.Module([n],[]),source,'exec'),ns)
if mode=='keys':
 gdk=types.SimpleNamespace(ModifierType=types.SimpleNamespace(CONTROL_MASK=1,SHIFT_MASK=2),**{'KEY_'+k:v for k,v in data['keys'].items()});out=[]
 for c in data['cases']:
  log=[]
  def record(name):return lambda *a,**k:log.append(name)
  class Term:
   _sel_flag=False
   def paste_clipboard(self):log.append('Paste')
   def get_has_selection(self):return c['selection']
   def copy_clipboard_format(self,*a):log.append('Copy')
   def unselect_all(self):pass
   def grab_focus(self):pass
   def get_font_scale(self):return 1.
   def set_font_scale(self,v):log.append('ZoomIn' if v>1 else 'ZoomOut' if v<1 else 'ZoomReset')
  term=Term();page=types.SimpleNamespace(_term=term,_key='fixture');nb=types.SimpleNamespace(get_visible=lambda:True,set_visible=record('ToggleTerminals'),get_current_page=lambda:0,get_nth_page=lambda _:page)
  ns={'Gdk':gdk,'Vte':types.SimpleNamespace(Terminal=Term,Format=types.SimpleNamespace(TEXT=0)),'win':types.SimpleNamespace(get_focus=lambda:term if c['terminal'] else None),'nb':nb,'cur_term':lambda:term if c['has_term'] else None,'ws_cancel_drag':lambda:c['drag'],'show_help':record('Help'),'exit_copy_mode':lambda _:False,'time':types.SimpleNamespace(monotonic=lambda:0),'handle_ctrl_c':lambda *a:log.append('CtrlC') or False,'wv':types.SimpleNamespace(load_uri=record('Reload')),'URL':'fixture://fake','term_session':lambda _:'fixture','tmuxc':lambda *a:types.SimpleNamespace(stdout='%fake'),'open_ai_session_here':record('StartAI'),'_mru_toggle':lambda:log.append('MruOrNext') or True,'_cycle_tab':lambda d:log.append('Cycle('+str(d)+')'),'_mosaic_toggle':record('Mosaic'),'open_switcher':record('Switcher'),'open_snippets_dialog':record('Snippets'),'new_local_tab':record('NewTerminal'),'open_xterm_tab':record('NewXterm'),'close_tab':record('CloseTab'),'Gtk':types.SimpleNamespace(main_quit=record('Quit'))}
  load({'on_key'},ns);result=ns['on_key'](None,types.SimpleNamespace(keyval=c['key'],state=(1 if c['control'] else 0)|(2 if c['shift'] else 0)))
  out.append('CancelDrag' if c['key']==gdk.KEY_Escape and c['drag'] else log[0] if log else 'Pass')
 print(json.dumps(out))
elif mode=='fuzzy':
 ns={};load({'fuzzy_score','SW_ORDER','DOT_COLORS','DOT_IDLE'},ns)
 out=[]
 for q,s in data:
  score=ns['fuzzy_score'](q,s);out.append(-1 if score is None else round(score*100))
 print(json.dumps({'scores':out,'dots':ns['DOT_COLORS'],'idle':ns['DOT_IDLE'],'order':ns['SW_ORDER']}))

elif mode=='switcher':
 ns={'ES':data['english'] is False};load({'fuzzy_score','SW_ORDER','SW_HINT','DOT_COLORS','DOT_IDLE'},ns)
 class Widget:
  def __init__(self,*a,**k):self.children=[];self.label=k.get('label');self.markup=None
  def get_style_context(self):return self
  def add_class(self,*a):pass
  def set_property(self,*a):pass
  def set_markup(self,s):self.markup=s
  def pack_start(self,w,*a):self.children.append(w)
  def pack_end(self,w,*a):self.children.append(w)
  def add(self,w):self.children.append(w)
  def get_children(self):return self.children[:]
  def remove(self,w):self.children.remove(w)
  def show_all(self):pass
  def select_row(self,r):pass
 pages=[types.SimpleNamespace(_key=r['key'],label=r['label']) for r in data['open']];lb=Widget();rows=[]
 ns.update(Gtk=types.SimpleNamespace(ListBoxRow=Widget,Box=Widget,Label=Widget,Orientation=types.SimpleNamespace(HORIZONTAL=0)),entry=types.SimpleNamespace(get_text=lambda:data['query']),lb=lb,rows=rows,notebook_pages=lambda:pages,tabs={r['key']:True for r in data['open']},tab_page_label=lambda p:p.label,STATE_ITEMS=data['items'],STATE_CACHE=data['states'])
 parent=next(n for n in tree.body if isinstance(n,ast.FunctionDef)and n.name=='open_switcher')
 for name in ['candidates','refill']:
  n=next(n for n in parent.body if isinstance(n,ast.FunctionDef) and n.name==name);exec(compile(ast.Module([n],[]),source,'exec'),ns)
 ns['refill']();print(json.dumps([{'key':key,'open':opened,'label':r.children[0].children[1].label,'hint':r.children[0].children[2].label,'dot':r.children[0].children[0].markup} for r,key,opened in rows]))
elif mode=='agents':
 import signal
 ns={'os':os,'signal':signal,'time':types.SimpleNamespace(sleep=lambda _:None,monotonic=lambda:0),'_read_cmdline':lambda _:[],'still_same':lambda _:[]}
 load({'DOUBLE_TAP_S','EXIT_WAIT_S','GRACE_S','_WRAPPERS','is_agent_name','ctrl_c_action','parse_client_state','client_blocks_stop','descendant_pids','_children_index','_is_agent_proc','foreground_agent','detached_descendants','signal_records','cleanup_after_exit','_parse_stat'},ns)
 out=[]
 for c in data:
  procs=[dict(pid=r['pid'],ppid=r['parent'],pgid=r['group'],sid=r['session'],tpgid=r['foreground'],start=r['start_time'],comm=r['comm']) for r in c['records']];argv={r['pid']:r['argv'] for r in c['records']};agent=ns['foreground_agent'](c['root'],procs,lambda pid:argv[pid]);targets=ns['detached_descendants'](agent,procs) if agent else [];sent=[];clock=[0]
  def alive(records):return [r for r in records if r['pid']!=20]
  def sleep(dt):clock[0]+=dt
  ns['cleanup_after_exit'](c['root'],procs,alive=alive,kill=lambda pid,sig:sent.append([pid,'Interrupt' if sig==signal.SIGINT else 'Terminate']),sleep=sleep,clock=lambda:clock[0],cmdline=lambda pid:argv[pid]);out.append({'leader':agent['pid'] if agent else None,'targets':[r['pid'] for r in targets],'signals':sent,'elapsed':clock[0]})
 print(json.dumps(out))

elif mode=='primitives':
 ns={};load({'DOUBLE_TAP_S','ctrl_c_action','parse_client_state','client_blocks_stop','_parse_stat'},ns)
 print(json.dumps({'actions':[ns['ctrl_c_action'](*c)for c in data['actions']], 'clients':[ns['parse_client_state'](s,'/private/tty')for s in data['clients']], 'blocked':[ns['client_blocks_stop'](s)for s in data['states']]}))

elif mode=='gestures':
 library=os.path.join(os.path.dirname(os.path.dirname(source)),'lib','agent_stop.py');libtree=ast.parse(open(library).read());agent={}
 for n in libtree.body:
  if isinstance(n,ast.Assign)and any(isinstance(t,ast.Name)and t.id=='DOUBLE_TAP_S'for t in n.targets)or isinstance(n,ast.FunctionDef)and n.name=='ctrl_c_action':exec(compile(ast.Module([n],[]),library,'exec'),agent)
 results=[]
 for scenario in data:
  armed=[];ns={'agent_stop':types.SimpleNamespace(**agent),'Vte':types.SimpleNamespace(Format=types.SimpleNamespace(TEXT=0)),'_arm_agent_cleanup':lambda t:armed.append(t.name)}
  load({'handle_ctrl_c'},ns)
  class Term:
   def __init__(self,name):self.name=name;self.selection=False
   def get_has_selection(self):return self.selection
   def copy_clipboard_format(self,*a):pass
   def unselect_all(self):self.selection=False
  terms={}
  for event in scenario:
   if event[0]=='new':terms[event[1]]=Term(event[1])
   elif event[0]=='close':del terms[event[1]]
   else:
    term=terms[event[1]];term.selection=event[3];ns['handle_ctrl_c'](term,event[4],event[2])
  results.append(len(armed))
 print(json.dumps(results))
