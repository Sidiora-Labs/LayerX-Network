import http.client
import json
import time


def qualify(raw_request, comet_port, evidence_path):
    def payload(method, params, identifier='comet-proof'):
        return json.dumps(dict(jsonrpc='2.0', id=identifier, method=method, params=params))

    def query(method, params, identifier='comet-proof'):
        status, _, body = raw_request('POST', '/comet', payload(method, params, identifier))
        return status, json.loads(body), body

    def upstream(method, params, identifier='comet-proof'):
        connection = http.client.HTTPConnection('127.0.0.1', comet_port, timeout=30)
        connection.request('POST', '/', payload(method, params, identifier), {'Content-Type': 'application/json'})
        response = connection.getresponse()
        body = response.read()
        assert response.status == 200, body
        connection.close()
        return body

    deadline = time.monotonic() + 90
    while time.monotonic() < deadline:
        status, reply, _ = query('commit', {})
        if status == 200 and int(reply['result']['signed_header']['header']['height']) >= 4:
            break
        time.sleep(.2)
    else:
        raise AssertionError('real Comet committed history unavailable')
    assert reply['id'] == 'comet-proof' and reply['jsonrpc'] == '2.0', reply
    latest = int(reply['result']['signed_header']['header']['height'])
    height = str(latest - 2)
    for identifier in [7, 'preserved-comet-id']:
        status, reply, body = query('commit', dict(height=height), identifier)
        assert status == 200 and reply['id'] == identifier, reply
        assert reply['result']['canonical'] is True, reply
        assert body == upstream('commit', dict(height=height), identifier)
    params = dict(height=height, page='1', per_page='100')
    status, reply, body = query('validators', params)
    assert status == 200 and int(reply['result']['count']) > 0, reply
    assert reply['result']['block_height'] == height, reply
    assert body == upstream('validators', params)
    key = '0885fcd13735f4309833a503ee804ea32395851479'
    params = dict(path='/store/evm/key', data='0x' + key, height=height, prove=True)
    expected = upstream('abci_query', params | {'data': key})
    evidence_path.write_bytes(expected)
    context = {
        'latest': json.loads(upstream('commit', {})),
        'anchor': json.loads(upstream('commit', {'height': str(int(height) + 1)})),
    }
    status, reply, body = query('abci_query', params)
    context['boundary_status'] = status
    context['boundary_reply'] = reply
    if status != 200:
        context['query_after'] = json.loads(upstream('abci_query', params | {'data': key}))
        context['latest_after'] = json.loads(upstream('commit', {}))
        context['anchor_after'] = json.loads(upstream('commit', {'height': str(int(height) + 1)}))
    evidence_path.with_suffix('.context.json').write_text(json.dumps(context, indent=2) + '\n')
    assert status == 200, reply
    response = reply['result']['response']
    assert response['height'] == height and response.get('code', 0) == 0, reply
    assert response['value'], reply
    assert [op['type'] for op in response['proofOps']['ops']] == ['ics23:iavl', 'ics23:simple'], reply
    assert all(op['data'] and op['key'] for op in response['proofOps']['ops']), reply
    assert body == expected
    future = str(2**63 - 2)
    status, reply, _ = query('abci_query', params | {'height': future})
    assert status == 503 and reply['error']['code'] == 'comet_evidence_unavailable', reply
    status, reply, _ = query('commit', dict(height=future))
    assert status == 503 and reply['error']['code'] == 'comet_evidence_unavailable', reply
    for method, params in [
        ('broadcast_tx_sync', {'tx': '00'}),
        ('broadcast_evidence', {}),
        ('unsafe_flush_mempool', {}),
        ('abci_query', params | {'prove': False}),
        ('abci_query', params | {'path': '/store/evm/subspace'}),
        ('abci_query', params | {'height': '0'}),
        ('commit', {'height': None}),
        ('validators', {'height': height, 'page': '1', 'per_page': '101'}),
    ]:
        status, reply, _ = query(method, params)
        assert status == 200 and reply['error']['code'] in [-32601, -32602], reply
    for body in [
        '{"jsonrpc":"2.0","id":1,"method":"commit","params":{"height":"1","height":"2"}}',
        '{"jsonrpc":"2.0","id":1,"id":2,"method":"commit","params":{}}',
        '{"jsonrpc":"2.0","id":null,"method":"commit","params":{}}',
    ]:
        status, _, data = raw_request('POST', '/comet', body)
        assert status == 200 and json.loads(data)['error']['code'] in [-32600, -32602], data
    body = payload('commit', {})
    assert raw_request('POST', '/comet', body.ljust(4097))[0] == 413
    print('real Comet TLS transport: commits, validators, composed state proofs, exact envelopes and refusals passed')
