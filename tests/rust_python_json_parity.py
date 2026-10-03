"""Native canonical JSON bytes/hash oracle, deterministic and network-free."""
import hashlib
import json
import math
import pathlib
import random
import struct
import subprocess

ROOT=pathlib.Path('/work')
BIN=ROOT/'.migration-build/target/debug/examples/python_json_contract'
values=[None,True,False,0,-1,2**256+7,-(2**512+11),0.0,-0.0,1e-7,1e-6,1e-5,1e-4,1e15,1e16,1e20,
        float.fromhex('0x0.0000000000001p-1022'),float.fromhex('0x1.fffffffffffffp+1023'),
        '', 'á/中文/😀/e\u0301/\u2028/\u007f', ''.join(map(chr,range(128))),
        {'$serde_json::private::Number':'literal','$serde_json::private::RawValue':'[1]'},
        {'z':0,'a':{'b':1.0,'a':2**200},'𐀀':['ñ',True,None]},
        {'é':1,'e\u0301':2,'\uffff':3,'😀':4}]
rng=random.Random(0xC0A4D05)
for _ in range(6000):
    value=struct.unpack('>d',rng.getrandbits(64).to_bytes(8,'big'))[0]
    if math.isfinite(value):values.append(value)
for exponent in range(-320,309):
    for coefficient in [1.0,1.2345678901234567,9.999999999999998]:
        value=coefficient*(10.0**exponent)
        if math.isfinite(value):values.extend([value,-value])
for _ in range(200):
    chars=[]
    for _ in range(rng.randrange(1,40)):
        codepoint=rng.randrange(0x110000)
        if not 0xd800<=codepoint<=0xdfff:chars.append(chr(codepoint))
    values.append({'name':''.join(chars),'float':rng.random(),'large':rng.getrandbits(300)})
raw=[json.dumps(value,ensure_ascii=True,separators=(',',':')) for value in values]
# Preserve JSON lexical forms that Python normalizes during decode.
raw.extend(['-0','1.000000000000000000000001','1E+001','1e-400','{"x":1,"x":2}',
            '{"\\u0024serde_json::private::Number":"literal"}'])
process=subprocess.run([str(BIN)],input='\n'.join(raw)+'\n',text=True,capture_output=True,timeout=30,check=True)
assert not process.stderr,process.stderr
assert process.stdout.endswith('\n')
lines=process.stdout.split('\n')[:-1];assert len(lines)==len(raw)
for index,(input_text,line) in enumerate(zip(raw,lines)):
    value=json.loads(input_text);ascii_text=json.dumps(value,sort_keys=True,separators=(',',':'))
    expected={'ascii':ascii_text,'unicode':json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False),
              'default_len':len(json.dumps(value)),'digest':hashlib.sha256(ascii_text.encode()).hexdigest()}
    actual=json.loads(line)
    assert actual==expected, (index,input_text,actual,expected)
print(f'{len(raw)} Python/Rust canonical JSON bytes, hashes and default lengths match')
