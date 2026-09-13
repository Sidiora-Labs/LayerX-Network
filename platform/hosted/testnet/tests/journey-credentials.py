import argparse
import json
import os
from pathlib import Path
import re
import runpy
import ssl
import urllib.request


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--gateway', required=True)
    parser.add_argument('--ca', type=Path, required=True)
    parser.add_argument('--session', type=Path, required=True)
    parser.add_argument('--destination-session', type=Path, required=True)
    parser.add_argument('--public-key', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    protected = runpy.run_path(str(Path(__file__).with_name('program-journey.py')))['protected']
    assert args.gateway.startswith('https://')
    assert re.fullmatch('[0-9a-f]{64}', args.public_key)
    args.output.mkdir(mode=0o700, parents=True, exist_ok=True)

    def write(name, value):
        with (args.output / name).open('x') as output:
            os.fchmod(output.fileno(), 0o600)
            output.write(value)
            output.flush()
            os.fsync(output.fileno())

    sessions = {}
    for name, path in [('source', args.session), ('destination', args.destination_session)]:
        token = protected(path).decode().removesuffix('\n')
        assert re.fullmatch(r'ses_[0-9a-f]{32}\.[0-9a-f]{64}', token), 'invalid identity session'
        sessions[name] = token
        write(name + '.curl', f'header = "Authorization: Bearer {token}"\n')
    scopes = ['activity:write', 'program:call', 'program:read', 'receipt:read']
    document = {'signer_public_key': args.public_key, 'scopes': scopes,
                'quota_requests': 1000, 'quota_window_seconds': 60}
    request = urllib.request.Request(
        args.gateway.rstrip('/') + '/v1/keys', data=json.dumps(document).encode(),
        headers={'Authorization': 'Bearer ' + sessions['source'],
                 'Content-Type': 'application/json', 'Idempotency-Key': os.urandom(32).hex()})
    with urllib.request.urlopen(request, context=ssl.create_default_context(cafile=args.ca),
                                timeout=30) as response:
        assert response.status in (200, 201), 'gateway key issuance refused'
        body = response.read(65537)
        assert len(body) <= 65536
    result = json.loads(body)
    assert result['ok'] is True
    key = result['key']
    assert key['authorization_scheme'] == 'LayerX-Key' and key['scopes'] == scopes
    assert key['signer_public_key'] == args.public_key
    assert re.fullmatch('[a-zA-Z0-9]{1,64}', key['id'])
    assert re.fullmatch('lxp_live_[0-9a-f]{64}', key['secret'])
    write('gateway.curl', f'header = "Authorization: LayerX-Key {key["id"]}:{key["secret"]}"\n')
    print('Signer-bound gateway key issued with payment, Programs and receipt scopes')


if __name__ == '__main__':
    main()
