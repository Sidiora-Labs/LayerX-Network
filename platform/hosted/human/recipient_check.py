import hashlib
import json
import sys

from onboarding_socket import request
from provision import Refused, fields, require, strict_pairs
from recipient_binding import sign_binding, unhex


def refused(operation):
    try:
        operation()
    except (OSError, Refused, ValueError):
        return
    raise ValueError('recipient authority accepted a refused request')


def main():
    require(len(sys.argv) == 2 and sys.argv[1] in ('authorized', 'wrong-peer'),
            'recipient check', 'explicit process role')
    raw = sys.stdin.buffer.read(8193)
    require(0 < len(raw) <= 8192, 'recipient check', 'bounded public fixture')
    value = json.loads(raw, object_pairs_hook=strict_pairs)
    fields(value, 'principal network_id checkpoint account asset authority', 'recipient check', 'public native evidence')
    configuration = dict(socket='/run/layerx/human/recipient.sock', peer_uid=4020, peer_gid=4020)
    fact = {name: unhex(value[name], 32) for name in ('account', 'asset', 'authority')}
    checkpoint = unhex(value['checkpoint'], 32)
    body = dict(principal=value['principal'], asset=list(fact['asset']), checkpoint=list(checkpoint))
    if sys.argv[1] == 'wrong-peer':
        refused(lambda: request(configuration, 'settlement-recipient', body))
        print('recipient process peer refusal passed')
        return
    signed = sign_binding(configuration, value['principal'], fact, value['network_id'], checkpoint)
    require(sign_binding(configuration, value['principal'], fact, value['network_id'], checkpoint) == signed,
            'recipient check', 'exact signature retry')
    for field in ('account', 'authority'):
        changed = dict(fact, **{field: hashlib.sha256(fact[field]).digest()})
        refused(lambda: sign_binding(configuration, value['principal'], changed, value['network_id'], checkpoint))
    refused(lambda: sign_binding(configuration, value['principal'], fact, value['network_id'] + 1, checkpoint))
    for changed in (dict(body, principal='unregistered-native-owner'),
                    dict(body, asset=list(hashlib.sha256(fact['asset']).digest())),
                    dict(body, checkpoint=[0] * 32), dict(body, recipient=[1] * 20)):
        refused(lambda: request(configuration, 'settlement-recipient', changed))
    print('recipient native binding, retry and seven authority refusals passed')


if __name__ == '__main__':
    try:
        main()
    except Exception:
        raise SystemExit('actual recipient authority checks failed') from None
