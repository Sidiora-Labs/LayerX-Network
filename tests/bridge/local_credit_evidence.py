import argparse
import json
import socket
import subprocess
import time
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from custody_credit import Rpc, require, sha, write_new
from deploy_local_custody import ROOT, command


def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def launch(directory, name, extra):
    selected = port()
    log = open(directory / (name + '.log'), 'wb')
    process = subprocess.Popen(['anvil', '--silent', '--host', '127.0.0.1',
                                '--port', str(selected), '--chain-id', '31337', '--mnemonic-random', '12', *extra],
                               stdout=log, stderr=log)
    log.close()
    rpc = Rpc('http://127.0.0.1:' + str(selected))
    try:
        for _ in range(200):
            require(process.poll() is None, 'Anvil exited before readiness')
            try:
                require(int(rpc.call('eth_chainId', []), 16) == 31337, 'chain identity')
                return process, rpc
            except (OSError, ValueError):
                time.sleep(0.05)
        raise ValueError('Anvil readiness timeout')
    except BaseException:
        process.terminate()
        process.wait(timeout=10)
        raise


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    directory = Path(args.output).resolve()
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    actor = Ed25519PrivateKey.generate()
    public = actor.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    did = 'did:layerx:' + public.hex()
    name = ('agent:' + did + ':main').encode()
    beneficiary = '0x' + sha(b'LX:ACCOUNT:v1' + len(name).to_bytes(4, 'big') + name).hex()
    asset = '0x' + sha(b'LayerX/local-custody/fee-asset/v1').hex()
    write_new(directory / 'actor.key', actor.private_bytes_raw())
    write_new(directory / 'attestor.key', Ed25519PrivateKey.generate().private_bytes_raw())
    amount = 1000000000000000000
    processes = []
    try:
        process, primary = launch(directory, 'primary', [])
        processes.append(process)
        command('python3', str(ROOT / 'tests/bridge/deploy_local_custody.py'),
                '--allow-local-chain', '--rpc', primary.url, '--asset', asset,
                '--beneficiary', beneficiary, '--amount', str(amount),
                '--output', str(directory / 'custody.json'))
        custody = json.loads((directory / 'custody.json').read_text())
        process, observer = launch(directory, 'observer', [
            '--fork-url', primary.url, '--fork-block-number', str(custody['fork_block'])])
        processes.append(process)
        script = str(ROOT / 'tests/bridge/custody_credit.py')
        pair = ['--rpc', primary.url, '--rpc', observer.url]
        profile = str(directory / 'custody.profile')
        attestor = str(directory / 'attestor.key')
        command('python3', script, 'profile', *pair, '--chain-id', '31337',
                '--network-id', '17', '--vault', custody['vault'],
                '--runtime-sha256', custody['runtime_sha256'], '--asset', asset,
                '--confirmations', '2', '--attestor-key', attestor, '--output', profile)
        credit = str(directory / 'custody.credit')
        evidence = [*pair, '--profile', profile, '--network-id', '17',
                    '--transaction', custody['transaction'], '--beneficiary', beneficiary,
                    '--beneficiary-key', '0x' + public.hex(), '--attestor-key', attestor,
                    '--expected-amount', str(amount)]
        command('python3', script, 'attest', *evidence, '--output', credit)
        command('python3', str(ROOT / 'tests/bridge/test_evidence.py'), *evidence)
        require(len(Path(profile).read_bytes()) == 207, 'profile length')
        require(len(Path(credit).read_bytes()) == 427, 'credit length')
        write_new(directory / 'identity.json', json.dumps({
            'did': did, 'public_key': public.hex(), 'beneficiary': beneficiary,
            'asset': asset, 'amount': str(amount), 'network_id': 17,
            'protocol_version': 3,
        }).encode())
        print('Real WETH custody, replicated Anvil evidence, profile and credit verified')
    finally:
        for process in reversed(processes):
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


if __name__ == '__main__':
    main()
