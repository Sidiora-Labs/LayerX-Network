import json
import os
from pathlib import Path
import subprocess

from provision import fields, protected_bytes, protected_json, require, write_json
from owner_native import protected_write, digest, span


def prepare(work_dir, executable, config_directory):
    root = Path(work_dir) / 'human-evidence-input'
    config_directory = Path(config_directory)
    require(config_directory.is_absolute() and config_directory.resolve() == config_directory,
            config_directory, 'canonical onboarding configuration directory')
    for suffix in ('TENANCY_DIGEST', 'AUTH_INDEX_KEY', 'STREAM_CURSOR_KEY'):
        name = 'LAYERX_HUMAN_' + suffix
        value = protected_bytes(config_directory / name, 128).decode()
        require(os.environ.get(name) == value, config_directory, 'same protected production configuration')
    funding = int(os.environ['LAYERX_HUMAN_ONBOARDING_INITIAL_FUNDING'])
    require(0 < funding < 2**128, config_directory, 'finite onboarding funding')
    request = protected_json(root / 'owner-request.json')
    fields(request, 'email display_name idempotency_key now', root, 'owner provisioning request')
    call = {key: request[key] for key in ('email', 'display_name', 'idempotency_key')}
    completed = subprocess.run([executable, 'prepare'], input=json.dumps(call).encode(), capture_output=True)
    require(completed.returncode == 0, root, 'real KMS sponsor preparation')
    owner = json.loads(completed.stdout)
    fields(owner, 'principal did public_key pending_key recovery_root recovery_threshold recovery_delay_seconds registration_action recovery_action recipient', root, 'KMS sponsor export')
    for name in ('public_key', 'pending_key', 'recovery_root', 'registration_action', 'recovery_action'):
        require(type(owner[name]) is list and len(owner[name]) == 32
                and all(type(value) is int and 0 <= value <= 255 for value in owner[name])
                and any(owner[name]), root, name)
    require(owner['public_key'] != owner['pending_key']
            and owner['did'] == 'did:layerx:' + owner['principal'], root, 'actual provider and KMS identity binding')
    write_json(root / 'owner-kms.json', owner)
    write_json(root / 'onboarding-configuration.json', dict(directory=str(config_directory),
        sponsor_principal=owner['principal'], initial_funding=funding))
    write_json(Path(work_dir) / 'human-owner-result.json', {key: owner[key] for key in
        ('principal', 'did', 'recovery_root', 'recovery_threshold', 'recovery_delay_seconds')})
    public = bytes(owner['public_key'])
    import hashlib
    account = hashlib.sha256(b'LX:ACCOUNT:v1' + span(('agent:' + owner['did'] + ':main').encode())).hexdigest()
    write_json(root / 'owner-admission.json', dict(did=owner['did'], public_key=public.hex(), owner_account=account))
    protected_write(root / 'owner-admission.txt', owner['did'].encode().hex().encode() + b':' + public.hex().encode() + b':0\n')


def sign(config, owner, label, payload, sequence, not_before, not_after, action):
    require(label in ('credit', 'identity', 'rotation', 'recovery'), config, 'bootstrap operation')
    request = dict(principal=owner['principal'], operation=label, sequence=sequence,
        not_before_ms=not_before, not_after_ms=not_after, action_key=list(action), fee_limit=config['fee_limit'],
        credit=list(payload) if label == 'credit' else None, effective_sequence=None,
        policy_start_ms=None, policy_end_ms=None)
    if label == 'rotation':
        require(len(payload) == 92 and payload[:4] == b'\x71\2\0\4', config, 'canonical rotation policy')
        request.update(policy_start_ms=int.from_bytes(payload[68:76], 'big'),
            policy_end_ms=int.from_bytes(payload[76:84], 'big'), effective_sequence=int.from_bytes(payload[84:92], 'big'))
    from onboarding_socket import request as signed_request
    result = signed_request(config['kms_signer'], 'sign', request)
    fields(result, 'activity activity_id', config, 'signed bootstrap response')
    signed = bytes.fromhex(result['activity'])
    verify_signed(signed, config, owner, payload, sequence, not_before, not_after, action,
                  8 if label == 'credit' else 7, 1 if label in ('credit', 'identity') else 2 if label == 'rotation' else 3)
    require(digest(b'activity-id', signed).hex() == result['activity_id'], config, 'signed activity identity')
    return signed


def verify_signed(signed, config, owner, payload, sequence, not_before, not_after, action, module, ordinal):
    from owner_native import Reader
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    r = Reader(signed, 'KMS signed activity')
    require(len(signed) <= 1048576 and r.take(5) == b'\0\3\x10\1\14', config, 'signed native header')
    def tag(value):
        require(r.number(1) == value, config, 'ordered canonical activity field')
    tag(1); require(r.number(2) == 3, config, 'inner protocol')
    tag(2); require(r.number(4) == config['network_id'], config, 'network')
    tag(3); require(r.number(4) == module << 16 | ordinal, config, 'operation')
    tag(4); require(r.span(255) == owner['did'].encode(), config, 'actor')
    tag(5); require(r.span(32) == bytes(owner['public_key']), config, 'owner')
    tag(6); require(r.number(8) == sequence, config, 'sequence')
    tag(7); require(r.number(8) == not_before and r.number(8) == not_after, config, 'time bounds')
    tag(8); require(r.span(32) == action, config, 'action key')
    tag(9); require(r.number(16) == config['fee_limit'], config, 'fee limit')
    tag(10); require(r.span(32) == digest(b'payload-hash', payload), config, 'payload hash')
    tag(11); require(r.span(524288) == payload, config, 'canonical payload')
    unsigned = signed[:4] + b'\13' + signed[5:r.offset]
    tag(12); signature = r.span(64); r.finish()
    require(len(signature) == 64, config, 'signature length')
    Ed25519PublicKey.from_public_bytes(bytes(owner['public_key'])).verify(signature, digest(b'signature-preimage', unsigned))


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('--work-dir', required=True)
    parser.add_argument('--executable', required=True)
    parser.add_argument('--config-directory', required=True)
    args = parser.parse_args()
    prepare(args.work_dir, args.executable, args.config_directory)
