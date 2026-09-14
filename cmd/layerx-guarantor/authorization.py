import json
import os
from pathlib import Path
import re
import stat
import subprocess


def require(value, message):
    if not value:
        raise ValueError(message)


def strict_pairs(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value, 'duplicate authorization configuration field')
        value[key] = item
    return value


def protected_descriptor(path, maximum):
    path = Path(path)
    require(path.is_absolute() and path.resolve() == path, 'authorization path is not canonical')
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        info = os.fstat(descriptor)
        require(stat.S_ISREG(info.st_mode) and stat.S_IMODE(info.st_mode) == 0o600
                and info.st_uid == os.geteuid() and info.st_nlink == 1
                and 0 < info.st_size <= maximum, 'authorization material is not protected')
        return descriptor
    except Exception:
        os.close(descriptor)
        raise


def fields(value, names):
    require(type(value) is dict and set(value) == set(names.split()), 'authorization configuration fields')


def unhex(value, length):
    require(type(value) is str and re.fullmatch('[0-9a-f]{' + str(length * 2) + '}', value),
            'authorization configuration hexadecimal field')
    result = bytes.fromhex(value)
    require(any(result), 'authorization configuration zero field')
    return result


def endpoint(value):
    for name in ('peer_uid', 'peer_gid'):
        require(type(value[name]) is int and 0 <= value[name] < 2**32, 'authorization peer identity')
    path = Path(value['socket'])
    require(path.is_absolute() and path.parent.resolve() == path.parent, 'authorization socket path')


def configuration(path):
    with os.fdopen(protected_descriptor(path, 16384), 'rb') as source:
        data = source.read(16385)
    require(len(data) <= 16384, 'authorization configuration size')
    value = json.loads(data, object_pairs_hook=strict_pairs)
    fields(value, 'version network_id chain_id settlement_contract checkpoint_registry vault custody_reference treasury human deposit_authority_key_file')
    require(type(value['version']) is int and value['version'] == 1, 'authorization configuration version')
    for name, maximum in (('network_id', 2**32), ('chain_id', 2**64)):
        require(type(value[name]) is int and 0 < value[name] < maximum, 'authorization network domain')
    for name in ('settlement_contract', 'checkpoint_registry', 'vault'):
        unhex(value[name], 20)
    unhex(value['custody_reference'], 32)
    fields(value['treasury'], 'socket peer_uid peer_gid public_key asset_id recipient')
    endpoint(value['treasury'])
    for name, length in (('public_key', 32), ('asset_id', 32), ('recipient', 20)):
        unhex(value['treasury'][name], length)
    fields(value['human'], 'socket peer_uid peer_gid')
    endpoint(value['human'])
    key = Path(value['deposit_authority_key_file'])
    require(key.is_absolute() and key.parent.resolve() == key.parent, 'deposit authority key path')
    return value


def registered_checkpoint(api, publication, rpc, request, policy):
    header = api.values(api.HEADER_TYPES, request['header'])
    digest = publication.raw(request['checkpoint_id'], 32)
    require(header[1] == policy['network_id'] and request['chain_id'] == policy['chain_id']
            and type(request['chain_id']) is int, 'authorization checkpoint network mismatch')
    require(int(rpc.call('eth_chainId', []), 16) == policy['chain_id'], 'authorization RPC chain mismatch')
    for name in ('settlement_contract', 'checkpoint_registry'):
        require(publication.raw(request[name], 20) == unhex(policy[name], 20), 'authorization contract mismatch')
    proof = publication.raw(request['validity_proof'])
    require(len(proof) <= 1_048_576 and api.checkpoint_hash(header, proof) == digest,
            'authorization checkpoint hash mismatch')
    registry = request['checkpoint_registry']
    require(rpc.view(registry, 'guarantorEligibility()', outputs=('address',))[0].lower()
            == request['settlement_contract'].lower(), 'authorization registry bond mismatch')
    require(rpc.view(registry, 'isCanonicalCheckpoint(bytes32)', ('bytes32',), (digest,), ('bool',))[0],
            'authorization checkpoint is not canonical')
    require(rpc.view(registry, 'checkpointHash(' + api.HEADER + ',bytes)', (api.HEADER, 'bytes'),
                     (header, proof), ('bytes32',))[0] == digest, 'authorization registered header mismatch')
    require(type(request['attestations']) is list and 0 < len(request['attestations']) <= 4096,
            'authorization certificate bounds')
    attestations = [api.values(api.ATTESTATION_TYPES, item) for item in request['attestations']]
    require(rpc.view(registry, 'isRecordedCertificate(bytes32,' + api.ATTESTATION + '[])',
                     ('bytes32', api.ATTESTATION + '[]'), (digest, attestations), ('bool',))[0],
            'authorization certificate differs')
    return header, digest


def principal(publication, fact, root):
    _, _, value, _ = publication.witness(publication.hx(fact['witness']), root)
    length = int.from_bytes(value[:2], 'big')
    name = value[2:2 + length]
    require(publication.sha(b'LX:ACCOUNT:v1' + len(name).to_bytes(4, 'big') + name) == fact['account'],
            'authorization account name mismatch')
    matched = re.fullmatch(rb'agent:did:layerx:([a-z0-9_-]{1,128}):main', name)
    require(matched is not None, 'authorization owner namespace unavailable')
    return matched[1].decode('ascii')


def recipient_binding(publication, policy, fact, header, digest):
    owner = principal(publication, fact, header[7])
    treasury = policy['treasury']
    public = unhex(treasury['public_key'], 32)
    if owner == treasury['public_key']:
        from signer.client import SignerClient
        require(fact['authority'] == public and fact['asset'] == unhex(treasury['asset_id'], 32),
                'treasury native authority or asset differs')
        recipient = unhex(treasury['recipient'], 20)
        client = SignerClient(treasury['socket'], expected_peer_uid=treasury['peer_uid'],
                              expected_peer_gid=treasury['peer_gid'], expected_public_key=public)
        signed = client.bind(header[1], fact['account'], fact['asset'], recipient, digest)
        publication.signature(public, b'LX:SETTLE:RECIPIENT:v1\0' + header[1].to_bytes(4, 'big')
                              + fact['account'] + fact['asset'] + recipient + digest, signed)
        return dict(account=publication.hx(fact['account']), asset=publication.hx(fact['asset']),
                    recipient=publication.hx(recipient), request_anchor=publication.hx(digest),
                    signature=publication.hx(signed))
    from recipient_binding import sign_binding
    return sign_binding(policy['human'], owner, fact, header[1], digest)


def deposit_message(publication, deposits, header, digest, reference):
    require(deposits and len(deposits) <= 4096, 'deposit authorization fact bounds')
    level = []
    previous = None
    for fact in deposits:
        require(previous is None or previous < fact['identity'], 'deposit authorization ordering')
        previous = fact['identity']
        leaf = (b'LX:PAXEER:DEPOSIT:LEAF:v1' + fact['identity'] + reference + fact['asset']
                + fact['amount'] + digest + header[1].to_bytes(4, 'big') + header[0].to_bytes(2, 'big'))
        level.append(publication.sha(b'LXP/v1/merkle-leaf\0' + leaf))
    while len(level) > 1:
        level = [publication.sha(b'LXP/v1/merkle-internal\0' + level[index]
                 + level[min(index + 1, len(level) - 1)]) for index in range(0, len(level), 2)]
    return (b'LX:PAXEER:DEPOSIT:ROOT:v1' + digest + header[7] + level[0] + reference
            + header[1].to_bytes(4, 'big') + header[0].to_bytes(2, 'big'))


def sign_deposit(publication, policy, expected_public, message):
    domain = b'LX:PAXEER:DEPOSIT:ROOT:v1'
    require(type(message) is bytes and len(message) == len(domain) + 134
            and message.startswith(domain) and int.from_bytes(message[-6:-2], 'big') == policy['network_id']
            and int.from_bytes(message[-2:], 'big') in (2, 3), 'deposit signing role or network differs')
    require(all(any(message[start:start + 32]) for start in range(len(domain), len(domain) + 128, 32)),
            'deposit signing commitment absent')
    descriptor = protected_descriptor(policy['deposit_authority_key_file'], 4096)
    payload = None
    try:
        payload = os.memfd_create('layerx-deposit-registration', os.MFD_CLOEXEC)
        public = subprocess.run(['openssl', 'pkey', '-in', '/proc/self/fd/' + str(descriptor),
                                 '-pubout', '-outform', 'DER'], pass_fds=(descriptor,),
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, check=True, timeout=10).stdout
        require(len(public) == 44 and public[:12] == bytes.fromhex('302a300506032b6570032100')
                and public[12:] == expected_public, 'deposit signer differs from vault authority')
        os.write(payload, message)
        os.lseek(payload, 0, os.SEEK_SET)
        signed = subprocess.run(['openssl', 'pkeyutl', '-sign', '-rawin', '-inkey',
                                 '/proc/self/fd/' + str(descriptor), '-in', '/proc/self/fd/' + str(payload)],
                                pass_fds=(descriptor, payload), stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, check=True, timeout=10).stdout
        publication.signature(expected_public, message, signed)
        return signed
    finally:
        if payload is not None:
            os.close(payload)
        os.close(descriptor)


def authorize(api, publication, rpc, request, source, policy_path):
    policy = configuration(policy_path)
    header, digest = registered_checkpoint(api, publication, rpc, request, policy)
    balances, _, deposits, profile = publication.native_request(api, request, header, digest)
    require(balances, 'authorization requires native owner balances')
    info = source.parent.lstat()
    require(source.parent.is_absolute() and source.parent.resolve() == source.parent
            and stat.S_ISDIR(info.st_mode) and stat.S_IMODE(info.st_mode) == 0o700
            and info.st_uid == os.geteuid(), 'authorization output directory is not private')
    require(source.name == digest.hex() + '.json', 'authorization output checkpoint mismatch')
    bindings = [recipient_binding(publication, policy, fact, header, digest) for fact in balances]
    deposit = None
    if deposits:
        vault = unhex(policy['vault'], 20)
        require(profile is not None and profile[13:33] == vault, 'authorization native vault mismatch')
        reference = unhex(policy['custody_reference'], 32)
        message = deposit_message(publication, deposits, header, digest, reference)
        public = rpc.view(publication.hx(vault), 'depositRootAuthority()', outputs=('bytes32',))[0]
        signed = sign_deposit(publication, policy, public, message)
        deposit = dict(vault=publication.hx(vault), custody_reference=publication.hx(reference),
                       signature=publication.hx(signed))
    value = dict(version=2, checkpoint_id=publication.hx(digest), recipient_bindings=bindings,
                 deposit_registration=deposit)
    require(not source.exists() and not source.is_symlink(), 'authorization output already exists')
    publication.atomic_json(source, value)
