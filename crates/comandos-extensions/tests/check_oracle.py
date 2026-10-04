"""Compare native synthetic results against the original SDK-backed check."""
import asyncio, json, pathlib, sys, tempfile
sys.path.insert(0,'/work/lib')
import extension_proxy
sys.path.insert(0,'/work/crates/comandos-extensions/tests')
from test_check import run,spec,MAIL

cases=[('demo',spec(mode)) for mode in ['normal','no-tools','cap','rpc','invalid-init','server-info','tools-capability','invalid-content','unsupported-version']]
cases += [('google-drive',spec(mode)) for mode in ['normal','denied','invalid-content']]
cases += [(name,spec(env={'EMAIL':email,'EMAIL_FIELD':field})) for name,expected in MAIL.items() for email in [expected,'other@example.test'] for field in ['email','emailAddress']]
cases += [('gmail',spec(mode,env={'EMAIL':MAIL['gmail']})) for mode in ['bad-json','no-text']]
cases += [('gmail',spec(env={'PROFILE_JSON':profile})) for profile in ['{"emailAddress":"other","emailAddress":"jesusbatallar@gmail.com"}','{"email":"jesusbatallar@gmail.com","extra":NaN}','{"emailAddress":NaN,"email":"jesusbatallar@gmail.com"}']]
from check_protocol_cases import CASES,entry as protocol_entry
cases += [('google-drive',protocol_entry(replies,'/work/crates/comandos-extensions/tests/test_check.py',sys.executable)) for _,replies,_ in CASES]
mismatches=[]
for name,entry in cases:
    with tempfile.TemporaryDirectory() as temp:
        home=pathlib.Path(temp)
        expected=asyncio.run(extension_proxy.check(home,name,entry))
        _,actual,_=run(home,{name:entry})
        if actual!=[expected]:mismatches.append({'entry':entry,'python':expected,'rust':actual})
for mismatch in mismatches:print(json.dumps(mismatch))
assert not mismatches,f'{len(mismatches)} of {len(cases)} SDK check scenarios mismatched'
print(f'{len(cases)} SDK check scenarios matched')
