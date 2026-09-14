import argparse
import base64
import json
import os
from pathlib import Path
import secrets
import subprocess

from material import component_defaults
from provision import protected_bytes, protected_json, require


def write(path, data):
    if path.exists():
        require(protected_bytes(path, 1048576) == data, path, 'identical retained configuration')
        return
    require(not path.is_symlink(), path, 'regular configuration destination')
    from owner_native import protected_write
    protected_write(path, data)
    descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def prepare(args):
    root, state, runtime = map(Path, (args.configuration, args.state, args.runtime))
    for directory in (root, state, runtime):
        require(directory.is_absolute() and directory.resolve() == directory, directory, 'canonical directory')
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        require(directory.stat().st_uid == os.geteuid(), directory, 'owned directory')
    registry = protected_json(args.registry)
    require(registry.get('schema_version') == 2 and registry.get('assets'), args.registry, 'real module registry')
    modules = []
    for module in registry['modules']:
        require(type(module['module']) is int and 1 <= module['module'] <= 9
                and len(module['ordinals']) == len(set(module['ordinals'])), args.registry, 'canonical supported module')
        modules.append(dict(module_id=module['module'], activity_types=[
            (module['module'] << 16) | ordinal for ordinal in module['ordinals']]))
    registry_file = root / 'registry.json'
    write(registry_file, json.dumps(dict(network_id=args.network_id, protocol_version=3, modules=modules),
        sort_keys=True, separators=(',', ':')).encode())
    config = component_defaults(args.network_id, args.chain_id)
    config.update(STORE_ROOT=str(state / 'store'), CUSTODY_ROOT=str(state / 'custody'),
        AUTH_INDEX_ROOT=str(state / 'auth-index'), IDENTITY_SOCKET=str(runtime / 'identity.sock'),
        IDENTITY_DEADLINE_SECONDS=10, IDENTITY_MAX_FRAME_BYTES=1048576,
        IDENTITY_PEER_UID=os.geteuid(), IDENTITY_PEER_GID=os.getegid(),
        IDENTITY_BINDING_SOCKET=str(runtime / 'identity-binding.sock'), IDENTITY_BINDING_TENANT=args.tenant,
        IDENTITY_BINDING_PEER_UID=os.geteuid(), IDENTITY_BINDING_PEER_GID=os.getegid(),
        IDENTITY_BINDING_DEADLINE_SECONDS=10, ONBOARDING_REGISTRY_FILE=str(registry_file),
        ONBOARDING_INITIAL_FUNDING=args.initial_funding, KMS_ENDPOINT=args.kms_address,
        KMS_SERVER_NAME=args.kms_server_name, KMS_ROOT_CERTIFICATE_DER=str(Path(args.tls) / 'ca.der'),
        KMS_CLIENT_CERTIFICATE_DER=str(Path(args.tls) / 'kms-client.der'),
        KMS_CLIENT_PRIVATE_KEY_DER=str(Path(args.tls) / 'kms-client-key.der'))
    for field, value in dict(MAX_FRAME_BYTES=1048576, MAX_CONNECTIONS=4, MAX_STREAMS=4,
                            MAX_QUEUED_BYTES=4194304, DEADLINE_SECONDS=10).items():
        config['KMS_' + field] = value
    digest = root / 'LAYERX_HUMAN_TENANCY_DIGEST'
    if digest.exists():
        config['TENANCY_DIGEST'] = protected_bytes(digest, 128).decode()
    else:
        require(not digest.is_symlink(), digest, 'regular tenancy digest destination')
        result = subprocess.run([args.executable, 'initialize-store'], input=b'', stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, timeout=30, check=True,
            env=dict(os.environ, LAYERX_HUMAN_STORE_ROOT=config['STORE_ROOT']))
        document = json.loads(result.stdout)
        require(set(document) == {'tenancy_digest'}, digest, 'actual tenancy initialization')
        config['TENANCY_DIGEST'] = document['tenancy_digest']
        write(digest, document['tenancy_digest'].encode())
    for name in ('AUTH_INDEX_KEY', 'STREAM_CURSOR_KEY'):
        path = root / ('LAYERX_HUMAN_' + name)
        config[name] = protected_bytes(path, 128).decode() if path.exists() else base64.urlsafe_b64encode(secrets.token_bytes(32)).decode().rstrip('=')
    require(type(args.initial_funding) is int and 0 < args.initial_funding < 2**128,
            root, 'finite initial funding')
    for name, value in config.items():
        write(root / ('LAYERX_HUMAN_' + name), str(value).encode())


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    for name in ('configuration', 'state', 'runtime', 'registry', 'tls', 'tenant', 'kms-address',
                 'kms-server-name', 'executable'):
        parser.add_argument('--' + name, required=True)
    for name in ('network-id', 'chain-id', 'initial-funding'):
        parser.add_argument('--' + name, required=True, type=int)
    prepare(parser.parse_args())
