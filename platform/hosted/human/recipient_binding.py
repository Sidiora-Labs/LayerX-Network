import hashlib
import json
import re
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from onboarding_socket import request
from provision import fields, protected_json, require, strict_pairs


def unhex(value, length):
    require(type(value) is str and re.fullmatch('[0-9a-f]{' + str(length * 2) + '}', value),
            'recipient binding', 'canonical hexadecimal field')
    result = bytes.fromhex(value)
    require(any(result), 'recipient binding', 'nonzero field')
    return result


def sign_binding(configuration, principal, fact, network, checkpoint):
    fields(configuration, 'socket peer_uid peer_gid', 'recipient binding', 'socket configuration')
    require(type(principal) is str and re.fullmatch('[a-z0-9_-]{1,128}', principal)
            and type(network) is int and 0 < network < 2**32,
            'recipient binding', 'principal and network')
    require(len(checkpoint) == 32 and any(checkpoint)
            and all(len(fact[name]) == 32 and any(fact[name]) for name in ('account', 'asset', 'authority')),
            'recipient binding', 'native fact bounds')
    did = 'did:layerx:' + principal
    canonical = ('agent:' + did + ':main').encode()
    account = hashlib.sha256(b'LX:ACCOUNT:v1' + len(canonical).to_bytes(4, 'big') + canonical).digest()
    require(account == fact['account'], 'recipient binding', 'principal MAIN account')
    result = request(configuration, 'settlement-recipient', dict(principal=principal,
        asset=list(fact['asset']), checkpoint=list(checkpoint)))
    fields(result, 'network_id principal did account public_key asset checkpoint recipient signature',
           'recipient binding', 'authenticated signer response')
    require(result['network_id'] == network and type(result['network_id']) is int
            and result['principal'] == principal and result['did'] == did
            and unhex(result['account'], 32) == account
            and unhex(result['public_key'], 32) == fact['authority']
            and unhex(result['asset'], 32) == fact['asset']
            and unhex(result['checkpoint'], 32) == checkpoint,
            'recipient binding', 'exact native authority and checkpoint')
    recipient, signed = unhex(result['recipient'], 20), unhex(result['signature'], 64)
    message = (b'LX:SETTLE:RECIPIENT:v1\0' + network.to_bytes(4, 'big') + account
               + fact['asset'] + recipient + checkpoint)
    Ed25519PublicKey.from_public_bytes(fact['authority']).verify(signed, message)
    return dict(account='0x' + account.hex(), asset='0x' + fact['asset'].hex(),
        recipient='0x' + recipient.hex(), request_anchor='0x' + checkpoint.hex(), signature='0x' + signed.hex())


def main():
    require(len(sys.argv) == 2, 'recipient binding', 'protected configuration argument')
    data = sys.stdin.buffer.read(8193)
    require(0 < len(data) <= 8192, 'recipient binding', 'request bound')
    value = json.loads(data, object_pairs_hook=strict_pairs)
    fields(value, 'principal network_id checkpoint account asset authority', 'recipient binding', 'request')
    fact = {name: unhex(value[name], 32) for name in ('account', 'asset', 'authority')}
    result = sign_binding(protected_json(sys.argv[1]), value['principal'], fact,
                          value['network_id'], unhex(value['checkpoint'], 32))
    sys.stdout.write(json.dumps(result, separators=(',', ':')) + '\n')


if __name__ == '__main__':
    try:
        main()
    except Exception:
        sys.stderr.write('Human settlement recipient authorization refused\n')
        raise SystemExit(1) from None
