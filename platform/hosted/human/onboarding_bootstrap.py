import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from types import SimpleNamespace

from onboarding_material import prepare as prepare_material, write
from owner_kms import prepare as prepare_owner
from onboarding_socket import serve
from provision import protected_bytes, protected_json, require


def main():
    parser = argparse.ArgumentParser()
    for name in ('network-id', 'chain-id', 'initial-funding'):
        parser.add_argument('--' + name, required=True, type=int)
    parser.add_argument('--tenant', required=True)
    args = parser.parse_args()
    state = Path('/var/lib/layerx/human')
    root = state / 'onboarding'
    inputs = root / 'human-evidence-input'
    configuration = state / 'onboarding-config'
    for directory in (root, inputs, configuration):
        directory.mkdir(mode=0o700, exist_ok=True)
    source = Path('/run/onboarding-input')
    for name in ('owner-request.json', 'recovery-policy.json', 'module-registry.json'):
        original = source / name
        with original.open('rb') as file:
            value = file.read(1048577)
        require(0 < len(value) <= 1048576, original, 'mounted input bound')
        write(inputs / name, value)
    binary = '/usr/local/bin/layerx-human-onboarding'
    prepare_material(SimpleNamespace(configuration=str(configuration), state=str(state),
        runtime='/run/layerx/human', registry=str(inputs / 'module-registry.json'),
        tls='/run/human-private/components', tenant=args.tenant, kms_address='127.0.0.1:9450',
        kms_server_name='layerx-human-kms', executable=binary, network_id=args.network_id,
        chain_id=args.chain_id, initial_funding=args.initial_funding))
    for path in configuration.glob('LAYERX_HUMAN_*'):
        os.environ[path.name] = protected_bytes(path, 1048576).decode()
    deadline = time.monotonic() + 30
    while True:
        try:
            with socket.create_connection(('127.0.0.1', 9450), timeout=1):
                pass
            result = subprocess.run(['/usr/local/bin/layerx-human-identity-provider', 'probe'],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=6,
                env=dict(os.environ, LAYERX_HUMAN_IDENTITY_PROVIDER_SOCKET='/run/layerx/human/identity.sock',
                    LAYERX_HUMAN_IDENTITY_PROVIDER_DEADLINE_SECONDS='5'))
            if result.returncode == 0:
                break
        except (OSError, subprocess.SubprocessError):
            pass
        require(time.monotonic() < deadline, root, 'real KMS and identity startup')
        time.sleep(0.1)
    prepare_owner(root, binary, configuration)
    owner = protected_json(inputs / 'owner-kms.json')
    signer_config = root / 'signer.json'
    write(signer_config, json.dumps(dict(socket='/run/layerx/human/onboarding-signer.sock',
        client_uid=4021, client_gid=4020, executable=binary, principal=owner['principal']),
        sort_keys=True, separators=(',', ':')).encode())
    serve(signer_config)


if __name__ == '__main__':
    main()
