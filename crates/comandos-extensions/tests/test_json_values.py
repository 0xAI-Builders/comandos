"""Opaque JSON and large integer preservation at the native protocol boundary."""
import math
from test_serve import upstream, launch, initialize, send, receive, close


def test_arbitrary_integers_and_opaque_keys_survive_proxy(tmp_path, upstream):
    payload={'integer':2**256+7,'negative':-(2**129+3),
             'opaque':{'$serde_json::private::Number':'literal-number-key',
                       '$serde_json::private::RawValue':'[1,2]'},'fraction':-0.0}
    process=launch(tmp_path,{**upstream,'disabled_tools':['blocked']})
    try:
        initialize(process)
        send(process,'tools/call',{'name':'echo','arguments':payload})
        result=receive(process)['result']['structuredContent']
        assert result==payload
        assert type(result['integer']) is int and type(result['negative']) is int
        assert type(result['fraction']) is float and math.copysign(1, result['fraction']) == -1
    finally:close(process)
