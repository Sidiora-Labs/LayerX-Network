import argparse
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import time

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, NoEncryption, load_pem_private_key

sys.path.insert(0, str(Path(__file__).resolve().parents[4] / 'agent/sdk/python'))
from layerx_sdk.verifier import AuthorizedReceiptBatch, _decode_protocol_receipt, verify_receipt_outcome


class Signatures:
    def verify_ed25519(self, public_key, signature, message):
        try:
            Ed25519PublicKey.from_public_bytes(public_key).verify(signature, message)
            return True
        except InvalidSignature:
            return False


def main():
    parser = argparse.ArgumentParser()
    for name in ['gateway', 'did', 'destination', 'asset', 'amount', 'encoder', 'sequencer-key']:
        parser.add_argument('--' + name, required=True)
    for name in ['ca', 'auth-config', 'signer', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--network-id', type=int, required=True)
    args = parser.parse_args()
    common = runpy.run_path(str(Path(__file__).with_name('program-journey.py')))
    common['protected'](args.auth_config)
    seed = common['protected'](args.signer)
    key = (Ed25519PrivateKey.from_private_bytes(seed) if len(seed) == 32
           else load_pem_private_key(seed, password=None))
    assert isinstance(key, Ed25519PrivateKey)
    assert args.did == 'did:layerx:' + key.public_key().public_bytes_raw().hex()
    assert args.gateway.startswith('https://') and 0 < args.network_id < 2**32
    asset = common['fixed'](args.asset)
    sequencer = common['fixed'](args.sequencer_key)
    amount = int(args.amount)
    assert 0 < amount < 2**128 and str(amount) == args.amount
    args.output.mkdir(mode=0o700, parents=True, exist_ok=True)
    counter = 0

    def write(name, value):
        path = args.output / name
        with path.open('xb') as output:
            os.fchmod(output.fileno(), 0o600)
            output.write(value)
            output.flush()
            os.fsync(output.fileno())
        return path

    def rpc(method, params, pending=False):
        nonlocal counter
        counter += 1
        body = json.dumps({'jsonrpc': '2.0', 'id': counter, 'method': method, 'params': params}).encode()
        request = write(f'{counter:02d}-request.json', body)
        result = subprocess.run(
            ['curl', '--fail-with-body', '--silent', '--show-error', '--max-time', '120',
             '--cacert', str(args.ca), '--config', str(args.auth_config), '--request', 'POST',
             args.gateway.rstrip('/') + '/rpc', '--header', 'Content-Type: application/json',
             '--data-binary', '@' + str(request)], stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, check=False)
        write(f'{counter:02d}-response.json', result.stdout)
        assert result.returncode == 0, f'payment transport failed: {result.returncode}'
        value = json.loads(result.stdout)
        if pending and value.get('id') == counter and value.get('error', {}).get('code') == -32001:
            state = value['error'].get('data', {})
            if state.get('state') == 'pending' and state.get('requested_commitment') == 'executed':
                return state
        assert value.get('id') == counter and 'error' not in value, value
        return value['result']

    def account(did):
        snapshot = rpc('lx_getBalances', [did])
        assert snapshot['did'] == did and snapshot['verification'] == 'state_proven'
        matches = [record for record in snapshot['accounts']
                   if record['asset_id'] == args.asset and record['name'] in
                   [f'agent:{did}:main', f'agent:{did}:asset:{args.asset}']]
        assert len(matches) == 1, 'one funded account is required for each payment participant'
        assert matches[0]['verification'] == 'state_proven'
        assert matches[0]['canonical_value'] and matches[0]['proof_material']
        return matches[0]

    source_record, destination_record = account(args.did), account(args.destination)
    assert source_record['name'] == f'agent:{args.did}:main', 'funded native MAIN source required'
    source, destination = source_record['account_id'], destination_record['account_id']
    before = rpc('lx_getBalance', [destination])
    source_before = rpc('lx_getAccount', [source])
    assert source_before['verification'] == 'state_proven'
    assert source_before['account_id'] == source and source_before['asset_id'] == args.asset
    identity_before = rpc('lx_getSequence', [args.did, 'identity'])
    assert identity_before['did'] == args.did
    assert identity_before['verification'] == 'authenticated_node_snapshot'
    now = time.time_ns() // 1_000_000
    signing = {'seed': key.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption()).hex(),
               'actor': args.did, 'from': source, 'to': destination, 'asset': args.asset,
               'from_name': source_record['name'], 'to_name': destination_record['name'],
               'amount': args.amount, 'source_sequence': int(source_before['next_sequence']),
               'identity_sequence': int(identity_before['next_sequence']), 'network_id': args.network_id,
               'not_before': now - 1000, 'not_after': now + 120000,
               'idempotency_key': os.urandom(32).hex()}
    result = subprocess.run([args.encoder], input=json.dumps(signing).encode(),
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    assert result.returncode == 0, f'canonical payment signing failed: {result.returncode}'
    signed = json.loads(result.stdout)
    write('payment.lxa', bytes.fromhex(signed['canonical']))
    deadline = time.monotonic() + 120
    while True:
        executed = rpc('lx_sendActivity', [signed['canonical'], 'executed'], pending=True)
        if executed.get('commitment') == 'executed':
            break
        assert time.monotonic() < deadline, executed
        time.sleep(0.1)
    assert executed['activity_id'] == signed['activity_id'] and executed['result_code'] == 0
    receipt = rpc('lx_getReceipt', [signed['activity_id']])
    decoded, _ = _decode_protocol_receipt(bytes.fromhex(receipt['receipt']))
    verified = verify_receipt_outcome(
        bytes.fromhex(receipt['receipt']),
        AuthorizedReceiptBatch(decoded.batch_id, asset, decoded.previous_state_root,
                               decoded.resulting_state_root, sequencer),
        Signatures(), protocol_version=3).receipt
    assert verified.activity_id.hex() == signed['activity_id']
    assert (verified.module_id, verified.operation, verified.result_code) == (1, 5, 0)
    assert verified.asset == asset and verified.amount == amount
    assert verified.from_account.hex() == source and verified.to_account.hex() == destination
    assert verified.from_sequence == int(source_before['next_sequence'])
    after = rpc('lx_getBalance', [destination])
    assert int(after['balance']) == int(before['balance']) + amount
    source_after = rpc('lx_getAccount', [source])
    identity_after = rpc('lx_getSequence', [args.did, 'identity'])
    assert verified.fee_charged > 0
    assert int(source_after['balance']) == int(source_before['balance']) - amount - verified.fee_charged
    assert verified.from_balance_before == int(source_before['balance']) - verified.fee_charged
    assert verified.from_balance_after == int(source_after['balance'])
    assert int(source_after['next_sequence']) == int(source_before['next_sequence']) + 1
    assert int(identity_after['next_sequence']) == int(identity_before['next_sequence']) + 1
    replayed = rpc('lx_sendActivity', [signed['canonical'], 'executed'])
    assert replayed == executed, 'payment idempotency replay changed the result'
    assert rpc('lx_getBalance', [destination]) == after, 'payment replay changed the balance'
    assert rpc('lx_getAccount', [source]) == source_after, 'payment replay changed the debit account'
    assert rpc('lx_getSequence', [args.did, 'identity']) == identity_after, 'payment replay changed identity state'
    assert rpc('lx_getReceipt', [signed['activity_id']]) == receipt
    authority = {'batch_id': verified.batch_id.hex(), 'asset': verified.asset.hex(),
                 'previous_state_root': verified.previous_state_root.hex(),
                 'resulting_state_root': verified.resulting_state_root.hex(),
                 'sequencer_public_key': args.sequencer_key}
    write('receipt.json', json.dumps({'result': dict(receipt, authority=authority)}, indent=2).encode())
    write('result.json', json.dumps({'activity_id': signed['activity_id'], 'before': before,
          'after': after, 'source_before': source_before, 'source_after': source_after,
          'identity_before': identity_before, 'identity_after': identity_after,
          'fee_charged': str(verified.fee_charged), 'result': executed, 'replayed': True}, indent=2).encode())
    print(json.dumps({'activity_id': signed['activity_id'], 'amount': args.amount,
                      'destination_balance': after['balance'], 'replayed': True}))


if __name__ == '__main__':
    main()
