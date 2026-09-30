import sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'lib'))
import operator_receipts
from operator_dispatch import Dispatcher

# The chat-store recovery tests left with the chat (S4); the conversations on
# disk are no longer read. What survives is the dispatcher's durable receipt.


def test_mutation_records_pending_receipt_before_transport_then_dispatched(tmp_path):
    executed=[]
    def post(url,data):
        assert operator_receipts.recent(tmp_path)[0]['status']=='pending'
        executed.append(url)
        return {'ok':True,'operationId':'request123','operationKey':'test|%1'}
    dispatcher=Dispatcher(base_url='http://unused',token='',hooks_dir=str(tmp_path),receipt_root=tmp_path,
                          local_handlers={},http_post=post,default_session='test',default_pane='%1')
    result=dispatcher.run('model_switch',{'session':'test'})
    receipt=operator_receipts.recent(tmp_path)[0]
    assert receipt['status']=='dispatched' and 'request123' in receipt['detail']
    assert result['receiptId']==receipt['id']
    assert len(executed)==1
