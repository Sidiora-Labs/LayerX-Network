import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace

root = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(root / 'platform/hosted/human'))
from guardians import generate
from owner_native import prepare_admission
from owner_custody import bootstrap, deposit
from provision import write_json
from custody_credit import Rpc
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

with tempfile.TemporaryDirectory(prefix='owner-custody-') as directory:
    work = Path(directory)
    os.umask(0o077)
    with socket.socket() as reserved:
        reserved.bind(('127.0.0.1', 0))
        port = reserved.getsockname()[1]
    config = work / 'anvil.json'
    with open(work / 'anvil.log', 'wb') as log:
        process = subprocess.Popen(['anvil', '--host', '127.0.0.1,127.0.0.2', '--port', str(port),
                                    '--chain-id', '31337', '--mnemonic-random', '--config-out', str(config)],
                                   stdout=log, stderr=log)
        try:
            for _ in range(100):
                assert process.poll() is None, 'real Anvil did not start'
                if config.exists():
                    break
                time.sleep(0.1)
            state = json.loads(config.read_text())
            keyfile = work / 'payer.key'
            keyfile.write_text(state['private_keys'][0])
            args = SimpleNamespace(work_dir=str(work), rpc=[f'http://127.0.0.{i}:{port}' for i in (1, 2)],
                ca_bundle=None, disposable_identity=None, key_file=str(keyfile), attestor_key=str(work / 'attestor.seed'),
                network_id=77, asset='01' * 32, amount=1000000000000000000)
            bootstrap(args)
            generate(work, work, [bytes([i]).hex() * 32 for i in (1, 2, 3)])
            request = dict(email='owner@example.com', display_name='Owner', idempotency_key='custody-owner', now=1)
            env = dict(os.environ, LAYERX_HUMAN_IDENTITY_PROVIDER_STATE_ROOT=str(work / 'lxip'),
                       LAYERX_HUMAN_IDENTITY_PROVIDER_RECOVERY_POLICY_FILE=str(work / 'human-evidence-input/recovery-policy.json'))
            result = subprocess.run([str(root / 'human/target/debug/layerx-human-identity-provider'), 'provision-owner'],
                                    input=json.dumps(request).encode(), capture_output=True, env=env, check=True)
            write_json(work / 'human-owner-result.json', json.loads(result.stdout))
            prepare_admission(work, work)
            deposit(args)
            inputs = work / 'human-evidence-input'
            credit = (inputs / 'custody-credit.bin').read_bytes()
            profile = (inputs / 'custody.profile').read_bytes()
            owner = json.loads((inputs / 'owner-admission.json').read_text())
            assert len(credit) == 427 and credit[:5] == b'LXDC1'
            assert credit[107:139].hex() == owner['owner_account']
            assert credit[139:171].hex() == owner['public_key']
            assert int.from_bytes(credit[191:207], 'big') == args.amount
            Ed25519PublicKey.from_public_bytes(profile[65:97]).verify(credit[363:], b'LX:CUSTODY:CREDIT:v1' + credit[:363])
            rpc = Rpc(args.rpc[0])
            before = rpc.call('eth_blockNumber', [])
            try:
                deposit(args)
                raise AssertionError('duplicate deposit must be refused')
            except FileExistsError:
                pass
            assert rpc.call('eth_blockNumber', []) == before
            print('real vault deployment, LXIP owner deposit, custody attestation and duplicate-deposit refusal passed')
        finally:
            process.terminate()
            process.wait(timeout=10)
