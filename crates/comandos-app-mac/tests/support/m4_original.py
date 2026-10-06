import ast,hashlib,json,sys,types,os,threading
x=json.load(sys.stdin);raw=open(x['source'],'rb').read();assert hashlib.sha256(raw).hexdigest()=='79670ba87d6be456c73059dba36f9c358838ca7fb8ce8fb2eb33424f92ad53bb'
tree=ast.parse(raw);names={'restore_tab_spec','ssh_host_from_session','cancel_restore_snapshot'};method_names={'_restore_saved','_restore_one','_record_restore_alias','_restore_is_cancelled','_restore_current_session','_cancel_pending_restore','_finish_restore'}
funcs=[];methods=[]
for n in tree.body:
 if isinstance(n,ast.FunctionDef) and n.name in names:funcs.append(n)
 if isinstance(n,ast.ClassDef):
  for f in n.body:
   if isinstance(f,ast.FunctionDef) and f.name in method_names:f.decorator_list=[];methods.append(f)
ns={'json':json,'os':os,'sys':sys,'TAB_KINDS':{'project','scratch','shell','ssh','ssh-tab'},'threading':threading}
exec(compile(ast.Module(body=funcs,type_ignores=[]),x['source'],'exec'),ns)
class Owner:pass
for f in methods:
 exec(compile(ast.Module(body=[f],type_ignores=[]),x['source'],'exec'),ns);setattr(Owner,f.name,ns[f.name])
o=Owner();o._restore_lock=threading.Lock();o._restore_snapshot=x['saved'];o._restore_original_by_current={};o._restore_current_by_original={};o._restore_cancelled=set();alive={'alive'};opened=[];calls=[]
def tmux(*args):
 calls.append(['tmux',list(args)]);code=0
 if args[0]=='has-session':code=0 if args[-1].lstrip('=') in alive else 1
 if args[0]=='new-session':alive.add(args[args.index('-s')+1])
 return types.SimpleNamespace(returncode=code,stdout='',stderr='')
def post(path,body,timeout=3):
 calls.append(['post',path,body]);response={}
 if path=='/ssh-connect':alive.add('ssh-actual');response={'session':'ssh-actual'}
 if path=='/ensure':alive.add(body['session'])
 return response,None
o._tab_for_key=lambda sess:next((r for r in opened if r[0]==sess),None)
o._select_win=lambda sess,win:calls.append(['select',sess,win])
def add(sess,label,url,**meta):opened.append([sess,label,meta['kind']])
o._add_tab=add;o._save_tabs=lambda:None
ns.update(tmuxc=tmux,http_post=post,find_project_dir=lambda sess:'',AppHelper=types.SimpleNamespace(callAfter=lambda f,*args:f(*args)))
o._restore_saved(x['saved'],{})
json.dump({'opened':opened,'calls':calls},sys.stdout)
