import ast,json,sys,io,types,urllib.parse,os,time
value=json.load(sys.stdin);source=value['source'];tree=ast.parse(open(source,encoding='utf8').read())
names={'load_tab_metadata','load_saved_tabs','ssh_host_from_session','restore_tab_spec','merge_tab_labels','cancel_restore_snapshot','find_project_dir','archive_tab','term_url','_ui_lang','initial_theme'}
nodes=[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name in names]
assert {n.name for n in nodes}==names
constants=[n for n in tree.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id in {'TAB_KINDS','VALID_THEMES','DOT_COLORS','DOT_IDLE','DEFAULT_THEME'} for t in n.targets)]
data=value.get('data');saved=[];calls=[]
class File(io.StringIO):
 def close(self):saved.append(self.getvalue());super().close()
def fakeopen(path,mode='r'):
 if mode=='w':return File()
 if 'file_bytes_hex' in value:return io.StringIO(bytes.fromhex(value['file_bytes_hex']).decode('utf8'))
 return io.StringIO(json.dumps(data))
class Url:
 @classmethod
 def alloc(cls):return cls()
 def init(self):return self
 def setScheme_(self,x):pass
 def setPath_(self,x):pass
 def setPercentEncodedQuery_(self,x):self.query=x
 def URL(self):return self.query
class Reply(io.BytesIO):
 def __enter__(self):return self
 def __exit__(self,*_):pass
ns=dict(json=json,open=fakeopen,TABS_META_FILE='metadata',TABS_FILE='tabs',TAB_HISTORY_FILE='history',time=types.SimpleNamespace(time=lambda:value.get('ts',42)),
 tmuxc=lambda *args:types.SimpleNamespace(returncode=0,stdout=value.get('cwd','') if args[-1]=='#{pane_current_path}' else value.get('cmd','')),
 os=types.SimpleNamespace(makedirs=lambda *_a,**_k:None,path=types.SimpleNamespace(dirname=lambda _: '.',expanduser=lambda _:value.get('codebase','owned')),replace=lambda *_:None,scandir=os.scandir,environ={'LANG':value.get('env_lang','')}),
 urllib=types.SimpleNamespace(parse=urllib.parse,request=types.SimpleNamespace(urlopen=lambda *_a,**_k:Reply(json.dumps({'_lang':value.get('conf_lang')}).encode()))),
 NSURLComponents=Url,TERM_HTML='owned-term',WS_URL='owned-ws',DASH_URL='owned-origin',http_get_json=lambda _:value.get('prefs'))
exec(compile(ast.Module(body=constants+nodes,type_ignores=[]),source,'exec'),ns)
op=value['op']
if op=='metadata':
 try:result=ns['load_tab_metadata']()
 except TypeError as error:result={'error':str(error)}
elif op=='saved':result=ns['load_saved_tabs']()
elif op=='ssh':result=ns['ssh_host_from_session'](value['session'])
elif op=='restore':result=ns['restore_tab_spec'](value['key'],data,value.get('project_dir',''))
elif op=='merge':result=ns['merge_tab_labels'](value['saved'],value['current'])
elif op=='cancel':result=ns['cancel_restore_snapshot'](value['saved'],value['session'])
elif op=='project':result=ns['find_project_dir'](value['session']) or None
elif op=='history':ns['archive_tab'](value['key'],value.get('label',''),value.get('reason','closed'));result=json.loads(saved[-1])
elif op=='term-url':result='&'.join(part for part in ns['term_url'](value.get('session'),value['theme'],value['auth']).split('&') if not part.startswith('ws='))
elif op=='lang':result=ns['_ui_lang']()
elif op=='theme':result=ns['initial_theme']()
elif op=='constants':result={k:ns[k] for k in ['VALID_THEMES','DOT_COLORS','DOT_IDLE','DEFAULT_THEME']}
elif op=='bridge':
 cls=next(n for n in tree.body if isinstance(n,ast.ClassDef) and n.name=='DashBridge');method=next(n for n in cls.body if isinstance(n,ast.FunctionDef) and n.name=='userContentController_didReceiveScriptMessage_')
 exec(compile(ast.Module(body=[method],type_ignores=[]),source,'exec'),ns)
 controller=types.SimpleNamespace(applyTheme_=lambda x:calls.append(['theme',x]),renameTab_to_=lambda s,l:calls.append(['rename',s,l]),openOrFocus_win_label_=lambda s,w,l:calls.append(['open',s,w,l]))
 ns['userContentController_didReceiveScriptMessage_'](types.SimpleNamespace(controller=controller),None,types.SimpleNamespace(body=lambda:data));result=calls
elif op=='retry':
 cls=next(n for n in tree.body if isinstance(n,ast.ClassDef) and n.name=='AppController');method=next(n for n in cls.body if isinstance(n,ast.FunctionDef) and n.name=='webView_didFailProvisionalNavigation_withError_')
 ns['NSTimer']=types.SimpleNamespace(scheduledTimerWithTimeInterval_target_selector_userInfo_repeats_=lambda *args:calls.append([args[0],args[2],args[4]]))
 exec(compile(ast.Module(body=[method],type_ignores=[]),source,'exec'),ns);page=object();ns['webView_didFailProvisionalNavigation_withError_'](types.SimpleNamespace(dash_web=page),page if value['dashboard'] else object(),None,None);result=calls
else:raise AssertionError(op)
print(json.dumps(result,ensure_ascii=True))
