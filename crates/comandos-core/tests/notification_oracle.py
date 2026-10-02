"""Synthetic Python policy oracle. Run only through scripts/rust-sandbox."""
import json
import sys
from pathlib import Path
assert Path('/work/scripts/rust-sandbox').exists(), 'Run inside rust-sandbox'
sys.path.insert(0, '/work/lib')
import notification_delivery as nd

NOW = 1_800_000_000_000
routes = []
kinds = list(nd.CATEGORIES) + ['prompt_accepted', 'turn_started', 'unknown', None]
clients = [[], [{}], [{'deviceId':'a','visible':True,'lastSeenAt':NOW,'canPlayAudio':False}],
    [{'deviceId':'a','visible':True,'lastSeenAt':NOW,'lastInteractionAt':NOW-1000,'canPlayAudio':True}],
    [{'deviceId':'a','visible':True,'lastSeenAt':NOW-90000,'lastInteractionAt':NOW-600000,'canPlayAudio':True}],
    [{'deviceId':'a','visible':True,'lastSeenAt':NOW-90001,'lastInteractionAt':NOW,'canPlayAudio':True}],
    [{'deviceId':'a','visible':False,'lastSeenAt':NOW,'hiddenSince':NOW-120000,'lastInteractionAt':NOW}],
    [{'deviceId':'a','visible':True,'connected':False,'lastSeenAt':NOW,'lastInteractionAt':NOW,'canPlayAudio':True}],
    [{'deviceId':'a','visible':True,'lastSeenAt':NOW,'lastInteractionAt':NOW,'canPlayAudio':False},
     {'deviceId':'b','visible':True,'lastSeenAt':NOW,'lastInteractionAt':NOW-10000,'canPlayAudio':True}],
    [{'deviceId':'a','visible':True,'lastSeenAt':NOW,'lastInteractionAt':None,'canPlayAudio':True}],
    [{'deviceId':None,'visible':True,'lastSeenAt':NOW,'lastInteractionAt':0,'canPlayAudio':True}]]
prefs = [nd.default_prefs(), nd.merge_prefs(None, {'muted':True}),
    nd.merge_prefs(None, {'modes': {k:'sound' for k in nd.default_prefs()['modes']}}),
    {**nd.default_prefs(), 'floatMs':None}, None]
for kind in kinds:
    for present in clients:
        for pref in prefs:
            for focus in (False, True):
                for age in (0,120000,120001,600000):
                    event = {'eventId':'e','kind':kind,'occurredAtMs':NOW-age}
                    routes.append({'event':event,'clients':present,'prefs':pref,'now':NOW,'focus':focus,
                        'expected':nd.route_event(event,present,pref,NOW,focus)})
events = [
    {'eventId':'p1','kind':'permission_requested','sessionKey':'s','paneId':'%1','projectKey':None,'sequence':1,'occurredAtMs':0},
    {'eventId':'p2','kind':'input_requested','sessionKey':'s','paneId':'%2','projectKey':None,'sequence':2,'occurredAtMs':10000},
    {'eventId':'p3','kind':'input_requested','sessionKey':'s','paneId':'%3','projectKey':None,'sequence':3,'occurredAtMs':20000},
    {'eventId':'done','kind':'turn_completed','sessionKey':'s','paneId':'%1','sequence':4,'occurredAtMs':30000},
    {'eventId':'conv','kind':'permission_requested','conversationId':'c','sequence':5,'occurredAtMs':40000},
    {'eventId':'none','kind':'permission_requested','sequence':6},
    {'eventId':'pane-key','kind':'permission_requested','paneKey':'k','paneId':'%2','sequence':7},
    {'eventId':'pane-clear','kind':'pane_closed','paneKey':'k','sequence':8},
    {'eventId':'news','kind':'news_edition','sourceEventId':'news-edition:abc','projectKey':'ignored','title':None,'excerpt':0,'sequence':9},
]
updates = [None, [], {}, {'modes':None}, {'modes':[]}, {'modes':{'unknown':'visual'}},
    {'modes':{'done':'loud'}}, {'volume':True}, {'volume':-1}, {'volume':1.01}, {'volume':None},
    {'volume':0}, {'volume':1}, {'muted':0}, {'muted':True}, {'ignored':True,'floatMs':3}]
merged = []
for update in updates:
    try:
        result = {'ok':nd.merge_prefs(None,update)}
    except ValueError as err:
        result = {'error':str(err)}
    merged.append({'update':update,'expected':result})
fixture = {'routes':routes,'events':events,'groups':nd.group_keys(events,None),
    'pending':nd.pending_requests(events),'merge':merged,
    'notices':[nd.notice(e,[],None,NOW,{'p2'},'group',True) for e in events]}
target = Path('/work/crates/comandos-core/tests/notification_fixture.json')
if '--write' in sys.argv:
    target.write_text(json.dumps(fixture, ensure_ascii=False))
else:
    assert json.loads(target.read_text()) == fixture, 'Policy oracle fixture differs from Python'
    print(f'{len(routes)} notification routes, {len(merged)} preferences and {len(events)} notices match Python')
