import http.client
import json
import os
from pathlib import Path
import struct
import time


def records(path=None):
    data = (Path(os.environ['LAYERX_AUTHORITY_REPLICA_LOG']) if path is None else path).read_bytes()
    offset = 0
    result = []
    while offset + 32 <= len(data) and data[offset:offset + 4] != bytes(4):
        assert data[offset:offset + 4] == b'LXPL'
        size = int.from_bytes(data[offset + 16:offset + 20], 'big')
        body = data[offset + 32:offset + 32 + size]
        assert len(body) == size and body[:4] == b'LXBE'
        header_length = int.from_bytes(body[77:79], 'big')
        cursor = 79 + header_length
        signature = body[cursor:cursor + 64]
        cursor += 64
        depth, index, count = struct.unpack_from('>BII', body, cursor)
        cursor += 9
        siblings = body[cursor:cursor + depth * 32]
        cursor += len(siblings)
        receipt_length = int.from_bytes(body[cursor:cursor + 4], 'big')
        receipt = body[cursor + 4:cursor + 4 + receipt_length]
        assert len(receipt) == receipt_length
        proof = struct.pack('>HHIIBI', 1, 0x4d50, index, count, depth, len(siblings)) + siblings
        result.append(dict(version=body[4], digest=body[5:37], batch=body[37:69],
                           header=body[79:79 + header_length], signature=signature,
                           receipt=receipt, proof=proof, index=index, count=count))
        offset += 32 + size
    return result


def request(batch, digest):
    connection = http.client.HTTPConnection('127.0.0.1', int(os.environ['LAYERX_AUTHORITY_PORT']), timeout=5)
    connection.request('GET', f'/v1/batches/{batch.hex()}/receipt-authority?receipt_digest={digest.hex()}',
                       headers={'Authorization': 'Bearer ' + os.environ['LAYERX_AUTHORITY_BEARER_TOKEN']})
    response = connection.getresponse()
    status, body = response.status, response.read()
    connection.close()
    return status, body


published = records(Path(os.environ['LAYERX_AUTHORITY_REPLICA_LOG']).parents[1] /
                    'logs/receipt-authority.log')
assert len(published) == 12 and published[-1]['version'] == ord('3')
last = published[-2]
assert last['version'] != ord('3') and last['header'] == published[-1]['header']
deadline = time.monotonic() + 5
while True:
    status, body = request(last['batch'], last['digest'])
    if status == 200:
        identity = json.loads(body)['batch_evidence']['batch_identity']
        assert identity['kind'] == 'occupancy_maintenance_v2'
        assert bytes.fromhex(identity['receipt_hex']) == published[-1]['receipt']
        break
    assert status in (404, 503) and time.monotonic() < deadline
    time.sleep(.01)
stored = records()
checked = 0
for activity in stored:
    if activity['version'] == ord('3'):
        continue
    matches = [item for item in stored if item['version'] == ord('3') and item['header'] == activity['header']]
    assert len(matches) == 1
    maintenance = matches[0]
    assert maintenance['signature'] == activity['signature']
    assert activity['index'] < maintenance['index'] == maintenance['count'] - 1
    assert activity['count'] == maintenance['count']
    status, body = request(activity['batch'], activity['digest'])
    assert status == 200
    document = json.loads(body)['batch_evidence']
    assert bytes.fromhex(document['header_hex']) == activity['header']
    assert bytes.fromhex(document['header_signature']) == activity['signature']
    assert bytes.fromhex(document['receipt_proof_hex']) == activity['proof']
    identity = document['batch_identity']
    assert identity['kind'] == 'occupancy_maintenance_v2'
    assert bytes.fromhex(identity['receipt_hex']) == maintenance['receipt']
    assert bytes.fromhex(identity['receipt_proof_hex']) == maintenance['proof']
    changed = bytes([activity['batch'][0] ^ 1]) + activity['batch'][1:]
    assert request(changed, activity['digest'])[0] == 404
    checked += 1
assert checked == 6
print('six replica maintenance attachments match retained signed headers, leaves and proofs; wrong batches refused')
