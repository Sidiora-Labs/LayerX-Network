import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import struct
import subprocess
import sys
import time

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
from cryptography.hazmat.primitives.serialization import load_pem_private_key

sys.path.insert(0, str(Path(__file__).resolve().parents[4] / 'agent/sdk/python'))
from layerx_sdk.programs import ProgramTrustContext, verify_program_receipt
from layerx_sdk.verifier import AuthorizedReceiptBatch


class Signatures:
    def verify_ed25519(self, public_key, signature, message):
        try:
            Ed25519PublicKey.from_public_bytes(public_key).verify(signature, message)
            return True
        except InvalidSignature:
            return False


def protected(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as handle:
        info = os.fstat(handle.fileno())
        assert stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
        assert stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1
        value = handle.read(4097)
        assert 0 < len(value) <= 4096
        return value


def blob(value):
    return struct.pack('>I', len(value)) + value


def fixed(value):
    decoded = bytes.fromhex(value)
    assert len(decoded) == 32 and decoded.hex() == value and any(decoded)
    return decoded


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--gateway', required=True)
    parser.add_argument('--ca', type=Path, required=True)
    parser.add_argument('--auth-config', type=Path, required=True)
    parser.add_argument('--signer', type=Path, required=True)
    parser.add_argument('--did', required=True)
    parser.add_argument('--asset', required=True)
    parser.add_argument('--network-id', type=int, required=True)
    parser.add_argument('--sequencer-key', required=True)
    parser.add_argument('--wasm', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    assert args.gateway.startswith('https://') and 0 < args.network_id < 2**32
    protected(args.auth_config)
    seed = protected(args.signer)
    key = (Ed25519PrivateKey.from_private_bytes(seed) if len(seed) == 32
           else load_pem_private_key(seed, password=None))
    assert isinstance(key, Ed25519PrivateKey)
    public = key.public_key().public_bytes_raw()
    assert args.did == 'did:layerx:' + public.hex()
    asset = fixed(args.asset)
    sequencer = fixed(args.sequencer_key)
    assert args.wasm.is_file() and not args.wasm.is_symlink()
    wasm = args.wasm.read_bytes()
    assert wasm.startswith(b'\0asm\1\0\0\0') and 0 < len(wasm) <= 524184
    args.output.mkdir(mode=0o700, parents=True, exist_ok=True)
    counter = 0

    def write(name, value):
        path = args.output / name
        with path.open('xb') as handle:
            os.fchmod(handle.fileno(), 0o600)
            handle.write(value)
            handle.flush()
            os.fsync(handle.fileno())
        return path

    def request(route, document, ordinal=None, key_id=None, refused=False, method="POST"):
        nonlocal counter
        counter += 1
        body = document if isinstance(document, bytes) else json.dumps(document).encode()
        body_path = write(f'{counter:02d}-request.bin', body)
        command = ['curl', '--fail-with-body', '--silent', '--show-error', '--max-time', '120',
                   '--cacert', str(args.ca), '--config', str(args.auth_config), '--request', method,
                   args.gateway.rstrip('/') + route, '--header',
                   'Content-Type: ' + ('application/octet-stream' if ordinal else 'application/json'),
                   '--data-binary', '@' + str(body_path)]
        if key_id is not None:
            command += ['--header', 'Idempotency-Key: ' + key_id]
        result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        write(f'{counter:02d}-response.json', result.stdout)
        value = json.loads(result.stdout)
        if refused:
            assert result.returncode == 22, value
            assert value['class'] == 'PolicyRefusal' and value['retriability'] == 'Terminal', value
            assert isinstance(value['protocol_result_code'], int) and value['protocol_result_code'] != 0, value
            return value
        assert result.returncode == 0, f'Programs request failed: {result.stdout.decode()}'
        assert 'error' not in value and value.get('ok', True) is True, value
        return value['result'] if 'result' in value else value['value']

    def submit(route, canonical, ordinal, key_id):
        deadline = time.monotonic() + 120
        while True:
            value = request(route, canonical, ordinal, key_id)
            if value.get('state') not in ('unknown', 'pending'):
                return value
            assert time.monotonic() < deadline, {
                'state': value.get('state'), 'activity_id': value.get('activity_id'),
            }
            time.sleep(0.1)

    def rpc(method, params):
        return request('/rpc', {'jsonrpc': '2.0', 'id': counter + 1, 'method': method, 'params': params})

    def signed(ordinal, payload):
        sequence = int(rpc('lx_getSequence', [args.did, 'identity'])['next_sequence'])
        assert 0 <= sequence < 2**64
        key_id = os.urandom(32)
        now = time.time_ns() // 1000000
        fields = (b'\x01' + struct.pack('>H', 3) + b'\x02' + struct.pack('>I', args.network_id)
                  + b'\x03' + struct.pack('>I', (9 << 16) | ordinal) + b'\x04' + blob(args.did.encode())
                  + b'\x05' + blob(public) + b'\x06' + struct.pack('>Q', sequence)
                  + b'\x07' + struct.pack('>QQ', now - 30000, now + 120000)
                  + b'\x08' + blob(key_id) + b'\x09' + (10**12).to_bytes(16, 'big')
                  + b'\x0a' + blob(hashlib.sha256(b'LXP/v1/payload-hash\0' + payload).digest())
                  + b'\x0b' + blob(payload))
        unsigned = struct.pack('>HHB', 3, 0x1001, 11) + fields
        digest = hashlib.sha256(b'LXP/v1/signature-preimage\0' + unsigned).digest()
        signature = key.sign(digest)
        key.public_key().verify(signature, digest)
        canonical = struct.pack('>HHB', 3, 0x1001, 12) + fields + b'\x0c' + blob(signature)
        return canonical, key_id.hex()

    program = os.urandom(32)
    deploy = program + struct.pack('>HBB', 2, 0, 0) + bytes(32) + hashlib.sha256(wasm).digest() + blob(wasm)
    canonical, key_id = signed(1, deploy)
    deployed = submit('/v1/programs/deploy', canonical, 1, key_id)
    assert deployed['state'] == 'completed' and deployed['receipt'], deployed
    duplicate, duplicate_key = signed(1, deploy)
    write('refused-deploy.lxa', duplicate)
    write('refused-deploy-payload.bin', deploy)
    refusal = request('/v1/programs/deploy', duplicate, 1, duplicate_key, refused=True)
    duplicate_id = hashlib.sha256(b'LXP/v1/activity-id\0' + duplicate).hexdigest()
    selector = {'idempotency_key': duplicate_key, 'expected_activity_id': duplicate_id,
                'requested_verification_level': 'sequencer-signed'}
    route = '/v1/programs/receipts/by-idempotency/' + duplicate_key
    refused = request(route, selector, method='GET')
    assert refused['result_code'] == refusal['protocol_result_code'], (refused, refusal)
    assert refused['state'] == 'refused' and refused['receipt'], refused
    replay_refusal = request('/v1/programs/deploy', duplicate, 1, duplicate_key, refused=True)
    assert replay_refusal['protocol_result_code'] == refusal['protocol_result_code']
    assert request(route, selector, method='GET') == refused
    activity_refused = request('/v1/programs/activities/' + duplicate_id,
                              {'activity_id': duplicate_id, 'requested_verification_level': 'sequencer-signed'},
                              method='GET')
    assert activity_refused['state'] == 'refused' and activity_refused['receipt'] == refused['receipt']
    assert activity_refused['result_code'] == refusal['protocol_result_code']
    seed = os.urandom(16)
    account = hashlib.sha256(b'LayerX/programs/program-account/v1\0' + program + blob(seed)).digest()
    register = program + b'LXPA1' + asset + blob(seed)
    canonical, _ = signed(6, register)
    registered = rpc('lx_sendActivity', [canonical.hex(), 'executed'])
    assert registered['commitment'] == 'executed' and registered['result_code'] == 0, registered
    before = rpc('lx_getBalance', [account.hex()])
    assert int(before['balance']) == 0, before
    calldata = (b'\x01\x01' + struct.pack('>H', len(seed)) + seed + account + asset
                + account + account + (1).to_bytes(16, 'big') + os.urandom(32) + os.urandom(32))
    capabilities = b'\x00\x04\x03\x05' + asset + account + (1).to_bytes(16, 'big') + b'\x07\x08'
    entrypoint = b'layerx_call'
    access = b'LayerX/programs/access-declaration/v1\0\0'
    resources = [100_000_000, 16_777_216, 1_048_576, 1_048_576, 64, 1_048_576, 4096]
    call = (struct.pack('>32sHHIHII7Q', program, 2, len(entrypoint), len(calldata), len(capabilities),
                        len(access), 1024, *resources) + entrypoint + calldata + capabilities + access)
    canonical, key_id = signed(3, call)
    write('call.lxa', canonical)
    write('call-idempotency-key', key_id.encode())
    executed = submit('/v1/programs/call', canonical, 3, key_id)
    assert executed['state'] == 'executed' and executed['result_code'] == 0, executed
    assert executed['receipt'] and executed['terminal_payload'] and executed['call_graph'], executed
    expected_activity = hashlib.sha256(b'LXP/v1/activity-id\0' + canonical).hexdigest()
    assert executed['activity_id'] == expected_activity and executed['program_id'] == program.hex()
    authority = executed['authority']
    verified = verify_program_receipt(
        executed,
        AuthorizedReceiptBatch(fixed(authority['batch_id']), bytes.fromhex(authority['asset']),
                               fixed(authority['previous_state_root']), fixed(authority['resulting_state_root']),
                               sequencer),
        Signatures(), ProgramTrustContext(sequencer, protocol_version=3)).verification.receipt
    assert (verified.module_id, verified.operation, verified.result_code) == (9, 3, 0)
    assert verified.activity_id.hex() == expected_activity
    assert verified.program_outcome is not None and verified.program_outcome.result_code == 0
    assert executed['outcome']['kind'] == 'completed' and executed['outcome']['code'] == 0
    write('call-activity-id', expected_activity.encode())
    replayed = submit('/v1/programs/call', canonical, 3, key_id)
    assert replayed == executed, (executed, replayed)
    after = rpc('lx_getBalance', [account.hex()])
    assert int(after['balance']) == 1, after
    write('result.json', json.dumps({'program_id': program.hex(), 'program_account': account.hex(),
          'before': before, 'after': after, 'deployment': deployed, 'refused_deployment': refused, 'registration': registered,
          'result': executed}, indent=2).encode())
    print(json.dumps({'program_id': program.hex(), 'program_account': account.hex(),
                      'activity_id': executed['activity_id'], 'escrow_balance': '1', 'replayed': True}))


if __name__ == '__main__':
    main()
