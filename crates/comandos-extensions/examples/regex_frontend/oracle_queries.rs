//! Temporary first-party Python measurement query. Removal remains migration debt.
pub const QUERY: &str = r#"
import sys,json,re,warnings,unicodedata,hashlib,_sre,platform
from re import _parser,_compiler,_constants
req=json.load(sys.stdin)
if req.get('identity'):
 print(json.dumps(dict(implementation=platform.python_implementation(),executable=sys.executable,binary_sha256=hashlib.sha256(open(sys.executable,'rb').read()).hexdigest(),version=sys.version,unicode=unicodedata.unidata_version,recursion=sys.getrecursionlimit(),digits=sys.get_int_max_str_digits(),magic=_constants.MAGIC,codesize=_sre.CODESIZE,maxrepeat=int(_constants.MAXREPEAT),maxgroups=_constants.MAXGROUPS,files={m.__file__:hashlib.sha256(open(m.__file__,'rb').read()).hexdigest() for m in [_parser,_compiler,_constants]})))
else:
 rows=req['rows']; assert len(rows)<=8
 out=[]
 for row in rows:
  re.purge(); p=''.join(chr(c) for c in row['pattern']); result={'id':row['id']}
  with warnings.catch_warnings(record=True) as ws:
   warnings.simplefilter('always')
   try:
    r=re.compile(p); result.update(result='Compiled',flags=r.flags,groups=r.groups,names=[[list(map(ord,k)),v] for k,v in r.groupindex.items()])
    with warnings.catch_warnings():
     warnings.simplefilter('ignore'); result['width_diagnostic']=list(_parser.parse(p,0).getwidth())
    if row.get('search') is not None: result['search']=bool(r.search(''.join(chr(c) for c in row['search'])))
   except Exception as e:
    result.update(result=type(e).__name__,position=getattr(e,'pos',None),message=str(e),line=getattr(e,'lineno',None),column=getattr(e,'colno',None))
  result['warnings']=[]
  for w in ws:
   msg=str(w.message); pos=None; classified=True
   fam=(w.category.__name__=='FutureWarning' and msg.startswith(('Possible nested set at position ','Possible set difference at position ','Possible set intersection at position ','Possible set symmetric difference at position ','Possible set union at position '))) or (w.category.__name__=='DeprecationWarning' and msg.startswith('bad character in group name '))
   if fam:
    head,sep,tail=msg.rpartition(' at position ')
    if sep and tail and all('0'<=c<='9' for c in tail): pos=int(tail)
    else: classified=False
   else: classified=False
   result['warnings'].append(dict(category=w.category.__name__,position=pos,message=msg,classified=classified))
  if row.get('op')=='format':
   from jsonschema import FormatChecker
   try: FormatChecker().check(row['value'],'regex'); result.update(format_result='FormatValid')
   except Exception as e: result.update(format_result=type(e).__name__)
  if row.get('op')=='schema':
   import jsonschema
   C={'draft4':jsonschema.Draft4Validator,'draft6':jsonschema.Draft6Validator,'draft7':jsonschema.Draft7Validator,'draft201909':jsonschema.Draft201909Validator,'draft202012':jsonschema.Draft202012Validator}[row['draft']]
   schema=json.loads(row['schema_json']); instance=json.loads(row['content_json'])
   result['proof']={'schema_sha256':hashlib.sha256(row['schema_json'].encode()).hexdigest(),'selected_class':C.__name__}
   try:
    C.check_schema(schema); result['schema']={'result':'valid'}; result['instance']={'result':'Valid' if C(schema).is_valid(instance) else 'Invalid'}
   except Exception as e: result['schema']={'result':type(e).__name__}
  out.append(result)
 print(json.dumps(out,separators=(',',':')))
"#;
