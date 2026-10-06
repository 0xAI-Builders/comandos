"""Execute immutable original bodies with inert Foundation/AppKit boundaries."""
import ast, hashlib, json, sys, types
v=json.load(sys.stdin)
source=open(v['source'],encoding='utf8').read()
assert hashlib.sha256(source.encode()).hexdigest()=='79670ba87d6be456c73059dba36f9c358838ca7fb8ce8fb2eb33424f92ad53bb'
tree=ast.parse(source)
cls=next(n for n in tree.body if isinstance(n,ast.ClassDef) and n.name=='AppController')
wanted={'_build_window','_make_strip_button','applyTheme_','_defer_terminal_action','_drain_pending_terminal_actions','_cancel_pending_terminal_actions','_select_win'}
nodes=[n for n in cls.body if isinstance(n,ast.FunctionDef) and n.name in wanted]
assert {n.name for n in nodes}==wanted
constants=[n for n in tree.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id in {'VALID_THEMES','TAB_STRIP_H'} for t in n.targets)]
calls=[]
class Obj:
 def __init__(self,name):self.name=name;self.rect=(0,0,1280,800)
 def __getattr__(self,key):
  def call(*args):
   if key=='alloc':return Obj(self.name)
   if key=='appearanceNamed_':return Obj(args[0])
   if key.startswith('initWithFrame'):self.rect=args[0];return self
   if key.startswith('initWithContentRect'):self.rect=args[0];calls.append(['window',list(args[0])]);return self
   if key=='frame':return types.SimpleNamespace(size=types.SimpleNamespace(width=self.rect[2],height=self.rect[3]))
   if key=='contentView':return Obj('content')
   if key=='setMinSize_':calls.append(['minimum',list(args[0])])
   if key=='setPosition_ofDividerAtIndex_':calls.append(['divider',args[0]])
   if key=='setTitle_' and self.name=='NSWindow':calls.append(['title',args[0]])
   if key=='setAppearance_':calls.append(['appearance',args[0].name])
   if key=='setHasHorizontalScroller_':calls.append(['scroll',args[0]])
   return self
  return call
ns=dict(json=json,sys=sys,objc=types.SimpleNamespace(python_method=lambda f:f),NSApp=Obj('app'),NSMakeRect=lambda *a:a,NSAppearanceNameDarkAqua='DarkAqua',TAB_STRIP_H=30,ES=True,_request=lambda url:url,DASH_URL='owned',terminal_action_session=lambda args:args[0] if args and isinstance(args[0],str) else None)
for name in ['NSWindow','NSAppearance','NSSplitView','NSView','NSScrollView','WKUserContentController','WKWebViewConfiguration','WKWebView','DashBridge','NSButton','NSFont']:ns[name]=Obj(name)
for name in ['NSApplicationActivationPolicyRegular','NSWindowStyleMaskTitled','NSWindowStyleMaskClosable','NSWindowStyleMaskMiniaturizable','NSWindowStyleMaskResizable','NSBackingStoreBuffered','NSViewWidthSizable','NSViewHeightSizable','NSViewMinYMargin','NSBezelStyleRecessed','NSButtonTypePushOnPushOff']:ns[name]=1
exec(compile(ast.Module(body=constants+nodes,type_ignores=[]),v['source'],'exec'),ns)
controller=types.SimpleNamespace()
controller._enable_local_access=lambda cfg:None
controller._make_strip_button=types.MethodType(ns['_make_strip_button'],controller)
op=v['op']
if op=='window':ns['_build_window'](controller);result=calls
elif op=='theme':
 controller.theme='noche';controller.tabs=[{'webview':types.SimpleNamespace(evaluateJavaScript_completionHandler_=lambda js,_:calls.append(js))}]
 ns['applyTheme_'](controller,v['name']);result={'theme':controller.theme,'scripts':calls}
elif op=='queue':
 controller.terminals_ready=v['ready'];controller.webterm_token=v['token'];controller._pending_terminal_actions=[]
 for session in v['sessions']:ns['_defer_terminal_action'](controller,lambda s:calls.append(s),session)
 if v.get('cancel'):ns['_cancel_pending_terminal_actions'](controller,v['cancel'])
 pending=len(controller._pending_terminal_actions);ns['_drain_pending_terminal_actions'](controller);result={'pending':pending,'calls':calls}
elif op=='select':
 replies=iter(v['replies'])
 def tmux(*args):
  calls.append(list(args));code,out=next(replies);return types.SimpleNamespace(returncode=code,stdout=out)
 ns['tmuxc']=tmux;ns['_select_win'](controller,v['session'],v['win']);result=calls
else:raise AssertionError(op)
print(json.dumps(result))
