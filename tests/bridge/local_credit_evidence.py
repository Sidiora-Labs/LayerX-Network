import argparse
import json
import os
import socket
import subprocess
import time
from pathlib import Path
from types import SimpleNamespace

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from custody_credit import Rpc, attest, create_profile, read_key, require, sha, unhex, write_new
from deploy_local_custody import ROOT, command, disposable_rpc


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


def existing_evidence(args, directory):
    require(len(args.rpc) == 2 and args.ca_bundle and args.disposable_identity,
            'two trusted RPC origins, CA bundle and disposable identity required')
    require(args.custody and args.asset and args.attestor_key and args.attestor_public_key
            and args.beneficiary_key and args.network_id == 402 and args.confirmations
            and args.confirmations > 0, 'explicit cluster custody configuration required')
    rpcs = [disposable_rpc(url, args.ca_bundle, args.disposable_identity) for url in args.rpc]
    require(rpcs[0].identity != rpcs[1].identity, 'distinct trusted RPC origins required')
    custody = json.loads(Path(args.custody).read_text())
    require(unhex(custody['asset'], 32) == unhex(args.asset, 32), 'configured asset binding')
    public = unhex(args.beneficiary_key, 32)
    did = 'did:layerx:' + public.hex()
    name = ('agent:' + did + ':main').encode()
    beneficiary = '0x' + sha(b'LX:ACCOUNT:v1' + len(name).to_bytes(4, 'big') + name).hex()
    require(unhex(custody['beneficiary'], 32) == unhex(beneficiary, 32), 'configured beneficiary binding')
    authority = read_key(args.attestor_key).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    require(authority == unhex(args.attestor_public_key, 32), 'configured attestor authority')
    identity = json.loads(Path(args.disposable_identity).read_text())
    require(custody['chain_id'] == identity['chain_id']
            and custody['comet_chain_id'] == identity['comet_chain_id']
            and unhex(custody['genesis_sha256'], 32) == unhex(identity['genesis_sha256'], 32),
            'deployment chain binding')
    profile = str(directory / 'custody.profile')
    previous_ca = os.environ.get('SSL_CERT_FILE')
    os.environ['SSL_CERT_FILE'] = str(Path(args.ca_bundle).resolve())
    try:
        create_profile(SimpleNamespace(rpc=args.rpc, ca_bundle=args.ca_bundle,
                       disposable_identity=args.disposable_identity,
                       chain_id=identity['chain_id'], network_id=args.network_id,
                       vault=custody['vault'], runtime_sha256=custody['runtime_sha256'], asset=args.asset,
                       confirmations=args.confirmations, attestor_key=args.attestor_key, output=profile))
        encoded = Path(profile).read_bytes()
        require(encoded[169:201] == unhex(identity['genesis_sha256'], 32), 'profile disposable genesis binding')
        attest(SimpleNamespace(rpc=args.rpc, ca_bundle=args.ca_bundle,
               disposable_identity=args.disposable_identity, profile=profile, network_id=args.network_id,
               transaction=custody['transaction'], beneficiary=beneficiary, beneficiary_key=args.beneficiary_key,
               attestor_key=args.attestor_key, expected_amount=int(custody['amount']),
               output=str(directory / 'custody.credit')))
    finally:
        if previous_ca is None:
            del os.environ['SSL_CERT_FILE']
        else:
            os.environ['SSL_CERT_FILE'] = previous_ca
    write_new(directory / 'identity.json', json.dumps({
        'did': did, 'public_key': public.hex(), 'beneficiary': beneficiary,
        'asset': args.asset, 'amount': custody['amount'], 'network_id': args.network_id,
        'protocol_version': 3, 'genesis_sha256': identity['genesis_sha256'],
        'comet_chain_id': identity['comet_chain_id'], 'genesis_source': identity['genesis_source'],
        'rpc_origins': args.rpc, 'attestor_public_key': args.attestor_public_key,
    }).encode())
    print('Existing TLS-verified custody observations, profile and credit verified')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', required=True)
    parser.add_argument('--rpc', action='append')
    parser.add_argument('--ca-bundle')
    parser.add_argument('--disposable-identity')
    parser.add_argument('--custody')
    parser.add_argument('--asset')
    parser.add_argument('--attestor-key')
    parser.add_argument('--attestor-public-key')
    parser.add_argument('--beneficiary-key')
    parser.add_argument('--network-id', type=int)
    parser.add_argument('--confirmations', type=int)
    args = parser.parse_args()
    directory = Path(args.output).resolve()
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    if args.rpc:
        existing_evidence(args, directory)
        return
    require(not any((args.ca_bundle, args.disposable_identity, args.custody, args.asset,
                     args.attestor_key, args.attestor_public_key, args.beneficiary_key,
                     args.network_id, args.confirmations)), 'cluster inputs require explicit RPC origins')
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
