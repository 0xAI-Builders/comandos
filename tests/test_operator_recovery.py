import ast
import sys
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
from types import SimpleNamespace

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'lib'))
import operator_chat
import operator_catalog
import operator_receipts
from operator_dispatch import Dispatcher


def test_stale_concurrent_chat_saves_preserve_both_conversations(tmp_path):
    stores=[]
    for text in ('first','second'):
        store=operator_chat.load_store(tmp_path)
        conversation=operator_chat.new_conversation(store)
        operator_chat.append_message(conversation,'user',text)
        stores.append(store)
    with ThreadPoolExecutor(max_workers=2) as pool:
        list(pool.map(lambda store:operator_chat.save_store(tmp_path,store),stores))
    result=operator_chat.load_store(tmp_path)
    assert {c['messages'][0]['text'] for c in result['conversations']} == {'first','second'}
    assert (tmp_path/'conversations.json.bak').is_file()


def test_corrupt_chat_file_recovers_last_valid_generation(tmp_path):
    store=operator_chat.load_store(tmp_path)
    convo=operator_chat.new_conversation(store)
    operator_chat.append_message(convo,'user','keep me')
    operator_chat.save_store(tmp_path,store)
    operator_chat.append_message(convo,'assistant','second generation')
    operator_chat.save_store(tmp_path,store)
    (tmp_path/'conversations.json').write_text('{broken')
    recovered=operator_chat.load_store(tmp_path)
    assert recovered['conversations'][0]['messages'][0]['text']=='keep me'


def test_lazy_mutation_batch_records_result_before_transport_and_stops_on_close(tmp_path):
    source=(ROOT/'bin/cc-dash').read_text()
    node=next(n for n in ast.parse(source).body if isinstance(n,ast.FunctionDef) and n.name=='_run_tool_batch')
    ns={'operator_catalog':operator_catalog}
    exec(compile(ast.Module(body=[node],type_ignores=[]),'batch','exec'),ns)
    executed=[]
    def post(url,data):
        assert operator_receipts.recent(tmp_path)[0]['status']=='pending'
        executed.append(url)
        return {'ok':True,'operationId':'request123','operationKey':'test|%1'}
    dispatcher=Dispatcher(base_url='http://unused',token='',hooks_dir=str(tmp_path),receipt_root=tmp_path,
                          local_handlers={},http_post=post,default_session='test',default_pane='%1')
    iterator=ns['_run_tool_batch'](dispatcher,[{'name':'model_switch','input':{'session':'test'}},
                                             {'name':'model_switch','input':{'session':'test'}}])
    result=next(iterator)
    receipt=operator_receipts.recent(tmp_path)[0]
    assert receipt['status']=='dispatched' and 'request123' in receipt['detail']
    assert result['receiptId']==receipt['id']
    iterator.close()
    assert len(executed)==1
