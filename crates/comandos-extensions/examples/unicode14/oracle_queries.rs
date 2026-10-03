//! Temporary first-party Python measurement text. Migration obligation:
//! remove executable queries from the final maintained audit after freezing.
pub const QUERY: &str = r#"
import sys,json,base64,struct,hashlib,unicodedata as u,_sre,re,re._casefix as cf,platform,os,sysconfig
req=json.load(sys.stdin)
assert sys.version_info[:3]==(3,11,15)
assert u.unidata_version=='14.0.0'
op=req['op']
if op=='identity':
    paths=[sys.executable,sys._base_executable,cf.__file__,re._parser.__file__,re._compiler.__file__]
    lib=os.path.join(sysconfig.get_config_var('LIBDIR'),sysconfig.get_config_var('LDLIBRARY'))
    if os.path.isfile(lib):paths.append(lib)
    if hasattr(u,'__file__'):paths.append(u.__file__)
    print(json.dumps({'v':1,'python':sys.version,'unicode':u.unidata_version,'unicodedata_origin':u.__spec__.origin,'executable':sys.executable,'files':{p:hashlib.sha256(open(p,'rb').read()).hexdigest() for p in paths},'extra':cf._EXTRA_CASES}))
elif op in ('primitive','canonical'):
    rows=req['rows']; assert len(rows)<=8
    output=[]
    for start,end in rows:
        assert 0<=start<end<=0x110000 and end-start<=1024
        data=bytearray()
        for cp in range(start,end):
            c=chr(cp)
            if op=='primitive':
                d=u.decimal(c,None); assert c.isdecimal()==(d is not None)
                a=c.isalnum(); ids=c.isidentifier(); cont=('A'+c).isidentifier()
                flags=sum(int(x)<<i for i,x in enumerate([d is not None,a,a or cp==95,c.isspace(),len((c+'A').splitlines())==2,_sre.unicode_iscased(cp),ids and cp!=95,cont,ids,cont]))
                data.extend(struct.pack('<IIIHBB',cp,_sre.unicode_tolower(cp),ord(c.upper()[0]),flags,255 if d is None else d,0))
            else:
                name=u.name(c,None)
                data.extend(struct.pack('<IH',cp,65535 if name is None else len(name)))
                if name is not None:data.extend(name.encode('ascii'))
        output.append({'v':1,'op':op,'start':start,'end':end,'count':end-start,'data':base64.b64encode(data).decode('ascii')})
    print(json.dumps(output,separators=(',',':')))
elif op=='lookup':
    rows=req['rows']; assert len(rows)<=8 and all(len(row)<=128 for row in rows)
    output=[]
    for row in rows:
        results=[]
        for cps in row:
            s=''.join(map(chr,cps))
            try:
                v=u.lookup(s); result={'class':'Character' if len(v)==1 else 'Sequence','value':list(map(ord,v))}
            except Exception as e:result={'class':type(e).__name__}
            reclass=[]
            for p in ('\\N{'+s+'}','[\\N{'+s+'}]'):
                try:re.compile(p); reclass.append('Compiled')
                except Exception as e:reclass.append(type(e).__name__)
            results.append({'input':cps,'lookup':result,'re':reclass})
        output.append(results)
    print(json.dumps(output,separators=(',',':')))
else:raise ValueError('unknown operation')
"#;
