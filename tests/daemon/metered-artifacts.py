#!/usr/bin/env python3
import json
import struct
import sys
import urllib.request
from pathlib import Path


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError('artifact endpoint redirected')


port, token_path, activity, digest = sys.argv[1:]
assert port.isdecimal() and 0 < int(port) <= 65535
assert all(len(value) == 64 and bytes.fromhex(value).hex() == value
           for value in (activity, digest))
token = Path(token_path).read_text(encoding='ascii')
assert 32 <= len(token) <= 128 and all(0x21 <= ord(c) <= 0x7e for c in token)
request = urllib.request.Request(
    f'http://127.0.0.1:{port}/v1/programs/activities/{activity}/artifacts?receipt_digest={digest}',
    headers={'Authorization': 'Bearer ' + token})
with urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect).open(request, timeout=10) as response:
    assert response.status == 200
    body = response.read(4 * 1024 * 1024 + 1)
assert len(body) <= 4 * 1024 * 1024
document = json.loads(body)
assert set(document) == {'activity_id', 'receipt_digest', 'terminal_payload', 'call_graph'}
assert document['activity_id'] == activity and document['receipt_digest'] == digest
for field in ('terminal_payload', 'call_graph'):
    encoded = document[field]
    data = bytes.fromhex(encoded)
    assert data.hex() == encoded and 0 < len(data) <= 1024 * 1024
    sys.stdout.buffer.write(struct.pack('>I', len(data)))
    sys.stdout.buffer.write(data)
