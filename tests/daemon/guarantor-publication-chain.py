import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from types import SimpleNamespace

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


s = load('publication_settlement', ROOT / 'cmd/layerx-guarantor/settlement.py')
p = load('publication_codec', ROOT / 'cmd/layerx-guarantor/publication.py')
r = load('publication_chain', ROOT / 'tests/daemon/finality-authority-chain.py')


def govern(chain, timelock, target, signature, *args):
    data = r.run('cast', 'calldata', signature, *args)
    def execute(destination, encoded):
        nonce = int(chain.rpc('eth_call', [{'to': timelock, 'data': r.run('cast', 'calldata', 'operationNonce()')}, 'latest']), 16)
        salt = p.hx(p.sha((destination + encoded + str(nonce)).encode()))
        delay = int(chain.rpc('eth_call', [{'to': timelock, 'data': r.run('cast', 'calldata', 'minDelay()')}, 'latest']), 16)
        chain.send(timelock, 'schedule(address,uint256,bytes,bytes32,uint64)', destination, '0', encoded, salt, str(delay))
        chain.rpc('evm_increaseTime', [delay + 1])
        chain.rpc('evm_mine', [])
        chain.send(timelock, 'execute(address,uint256,bytes,bytes32,uint256)', destination, '0', encoded, salt, str(nonce))
    execute(timelock, r.run('cast', 'calldata', 'setCallPermission(address,bytes4,bool)', target, data[:10], 'true'))
    execute(target, data)


def setup(work, url):
    chain = r.Chain(int(url.rsplit(':', 1)[1]))
    artifacts = ROOT / 'build/withdraw-contracts/artifacts'
    custody = json.loads(Path(os.environ['LAYERX_TEST_PUBLICATION_CUSTODY_FILE']).read_text())
    vault = custody['vault']
    token = json.loads((artifacts / 'BetaUsdl.sol/BetaUsdl.json').read_text())
    chain.rpc('anvil_setCode', [r.USDL, token['deployedBytecode']['object']])
    chain.rpc('anvil_setStorageAt', [r.USDL, '0x' + '00' * 32, '0x' + '00' * 12 + r.ADMIN[2:]])
    bond = chain.deploy(json.loads((artifacts / 'GuarantorBond.sol/GuarantorBond.json').read_text()),
        'constructor(address,address,address,address,bytes32,uint16,uint32,uint32,uint64,bytes32,uint192)',
        [r.ADMIN, r.ADMIN, r.USDL, vault, r.run('cast', 'keccak', 'USDL'), '3', '77', '1000', '86400', r.word('a1'), str(1 << 128)])
    chain.send(r.USDL, 'mint(address,uint256)', r.ADMIN, '2000')
    chain.send(r.USDL, 'approve(address,uint256)', bond, '2000')
    for index in (1, 2):
        account = s.Account.from_key(index.to_bytes(32, 'big'))
        identifier = '0x' + index.to_bytes(32, 'big').hex()
        chain.send(bond, 'activateGuarantor(bytes32,address,address,uint64,uint64)', identifier, account.address, r.ADMIN, '1', str(index))
        chain.send(bond, 'depositBond(bytes32,uint256)', identifier, '1000')
    request = (work / 'data/genesis/paxeer-registration-request.lxrr').read_bytes()
    registry = chain.deploy(json.loads((artifacts / 'CheckpointRegistry.sol/CheckpointRegistry.json').read_text()),
        'constructor(address,uint16,uint32,uint16,uint16,uint64,uint64,bytes32,bytes32,bytes32,bytes32,uint192)',
        [bond, '3', '77', '2', '32', '3600', '60', '0x' + hashlib.sha256((work / 'data/genesis/genesis.manifest').read_bytes()).hexdigest(),
         '0x' + request[9:41].hex(), '0x' + request[41:73].hex(), r.word('a2'), str(1 << 128)])
    manager = chain.deploy(json.loads((artifacts / 'CheckpointChallengeManager.sol/CheckpointChallengeManager.json').read_text()),
        'constructor(address,address,address,address,uint64,uint128,bytes32,uint192)',
        [registry, bond, r.ADMIN, r.ADMIN, '3600', '1', r.word('a5'), str(1 << 128)])
    chain.send(bond, 'setSlashingAuthority(address)', manager)
    govern(chain, custody['timelock'], vault, 'setGuarantorBond(address)', bond)
    authority = Ed25519PrivateKey.from_private_bytes(bytes([0x77]) * 32).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    govern(chain, custody['timelock'], vault, 'setDepositRootAuthority(bytes32)', '0x' + authority.hex())
    (work / 'data/genesis/genesis.registration').write_bytes(b'LXGR\x01' + (77).to_bytes(4, 'big') + bytes(8) + request[41:73] * 2 + b'\x01')
    (work / 'publication-chain.json').write_text(json.dumps({'bond': bond, 'registry': registry, 'vault': vault}))


def decode_header(encoded):
    value = s.raw(encoded)
    assert len(value) == 354
    out, at = [], 5
    for index, kind in enumerate(s.HEADER_TYPES, 1):
        assert value[at] == index
        at += 1
        if kind == 'bytes32':
            assert int.from_bytes(value[at:at + 4], 'big') == 32
            at += 4
            out.append(value[at:at + 32])
            at += 32
        else:
            n = int(kind[4:]) // 8
            out.append(int.from_bytes(value[at:at + n], 'big'))
            at += n
    assert at == len(value)
    return out


def replay(native, work, count):
    state = work / f'replay-{count}'
    exports = work / f'exports-{count}'
    state.mkdir(mode=0o700)
    exports.mkdir(mode=0o755)
    with (work / f'replay-{count}.log').open('w') as log:
        subprocess.run(['bash', '-c', 'set -a\nsource "$1"\nexec "$2" "$3" "$4" "$5" "$6"', 'replay',
            str(native / 'data/sequencer.env'), str(ROOT / 'build/tests/lxp_test_guarantor_runtime'),
            str(native / 'data/sequencer.conf'), str(state), str(native / 'data/checkpoints/da-bodies.log'), str(count)],
            cwd=ROOT, env=os.environ | {'LAYERX_TEST_PUBLICATION_EXPORT_DIR': str(exports)}, stdout=log, stderr=log, check=True, timeout=120)
    return json.loads((exports / f'{count}.json').read_text())


def certificate(export, chain_config, url, submitter, state, inputs):
    h = decode_header(export['canonical_header'])
    digest = s.checkpoint_hash(h, b'')
    attestations = []
    for index in (1, 2):
        account = s.Account.from_key(index.to_bytes(32, 'big'))
        fields = [h[0], h[1], 31337, chain_config['bond'], h[2], digest, digest, index.to_bytes(32, 'big'), h[3], h[11], True, True, 31, h[13]]
        sig = s.keys.PrivateKey(bytes(account.key)).sign_msg_hash(s.hashlib.sha256(b'LXP/v2/guarantor-attestation\0' + s.encode_packed(s.ATTESTATION_TYPES[:14], fields)).digest())
        attestations.append(fields + [account.address, sig.r.to_bytes(32, 'big'), sig.s.to_bytes(32, 'big'), sig.v + 27])
    def encoded(values):
        return [p.hx(v) if isinstance(v, bytes) else v for v in values]
    return dict(rpc_url=url, chain_id=31337, settlement_contract=chain_config['bond'], checkpoint_registry=chain_config['registry'],
        header=encoded(h), checkpoint_id=p.hx(digest), validity_proof='0x', attestations=[encoded(a) for a in attestations],
        submitter_key_file=str(submitter), publication_state_dir=str(state), publication_inputs_dir=str(inputs), native_facts=export['native_facts'])


def authorize(request, inputs, vault):
    h = s.values(s.HEADER_TYPES, request['header'])
    digest = s.raw(request['checkpoint_id'], 32)
    balances, _, deposits, _ = p.native_request(s, request, h, digest)
    owner = Ed25519PrivateKey.from_private_bytes(bytes([0x11]) * 32)
    bindings = []
    for fact in balances:
        recipient = bytes([0x31]) * 20
        message = b'LX:SETTLE:RECIPIENT:v1\0' + h[1].to_bytes(4, 'big') + fact['account'] + fact['asset'] + recipient + digest
        bindings.append(dict(account=p.hx(fact['account']), asset=p.hx(fact['asset']), recipient=p.hx(recipient), request_anchor=p.hx(digest), signature=p.hx(owner.sign(message))))
    assert len(deposits) == 1
    reference = s.raw('0x' + '00' * 12 + vault[2:], 32)
    deposit = deposits[0]
    leaf = b'LX:PAXEER:DEPOSIT:LEAF:v1' + deposit['identity'] + reference + deposit['asset'] + deposit['amount'] + digest + h[1].to_bytes(4, 'big') + h[0].to_bytes(2, 'big')
    root = p.sha(b'LXP/v1/merkle-leaf\0' + leaf)
    registration = b'LX:PAXEER:DEPOSIT:ROOT:v1' + digest + h[7] + root + reference + h[1].to_bytes(4, 'big') + h[0].to_bytes(2, 'big')
    authority = Ed25519PrivateKey.from_private_bytes(bytes([0x77]) * 32)
    document = dict(version=2, checkpoint_id=p.hx(digest), recipient_bindings=bindings,
        deposit_registration=dict(vault=vault, custody_reference=p.hx(reference), signature=p.hx(authority.sign(registration))))
    p.atomic_json(inputs / (digest.hex() + '.json'), document)
    return document


def publish(request, work, label):
    path = work / (label + '.request.json')
    path.write_text(json.dumps(request))
    with (work / (label + '.log')).open('w') as log:
        subprocess.run([sys.executable, str(ROOT / 'cmd/layerx-guarantor/settlement.py'), 'register', str(path), str(work / (label + '.result.json'))], cwd=ROOT, stdout=log, stderr=log, check=True, timeout=180)
    return json.loads((work / (label + '.result.json')).read_text())


def drive(work, env, url):
    subprocess.run(['cargo', 'build', '--locked', '--manifest-path', 'human/Cargo.toml', '-p', 'layerx-paxeer-client', '--example', 'native_publication_fetch'], cwd=ROOT, check=True)
    anchor_dir = work / 'anchor'
    anchor_dir.mkdir(mode=0o755)
    os.chown(anchor_dir, 4021, 4021)
    anchor_path = anchor_dir / 'checkpoint'
    env = env | {'LAYERX_TEST_WITHDRAW_ANCHOR_FILE': str(anchor_path), 'LAYERX_TEST_PUBLICATION_CUSTODY_FILE': str(work / 'custody.json')}
    state, inputs = work / 'publication-state', work / 'publication-inputs'
    state.mkdir(mode=0o700)
    inputs.mkdir(mode=0o700)
    submitter = s.Account.create()
    submitter_path = work / 'publication-submitter.key'
    fd = os.open(submitter_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as output:
        output.write('0x' + bytes(submitter.key).hex())
    chain = r.Chain(int(url.rsplit(':', 1)[1]))
    funding = chain.rpc('eth_sendTransaction', [{'from': r.ADMIN, 'to': submitter.address, 'value': hex(10 ** 21)}])
    funding_deadline = time.monotonic() + 30
    while chain.rpc('eth_getTransactionReceipt', [funding]) is None and time.monotonic() < funding_deadline:
        time.sleep(.1)
    assert chain.rpc('eth_getTransactionReceipt', [funding]) is not None
    with (work / 'admission-publication.log').open('w') as log:
        process = subprocess.Popen(['bash', 'tests/daemon/program-admission.sh', 'build', '--withdraw'], cwd=ROOT, env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 90
            while not Path(str(anchor_path) + '.ready').exists():
                assert process.poll() is None, 'withdrawal fixture exited before anchor'
                assert time.monotonic() < deadline, 'credit readiness deadline'
                time.sleep(.1)
            native = next(work.glob('lxp-program-admission-*'))
            chain_config = json.loads((native / 'publication-chain.json').read_text())
            first = certificate(replay(native, work, 1), chain_config, url, submitter_path, state, inputs)
            auth = authorize(first, inputs, chain_config['vault'])
            rpc = s.RPC(url)
            s.register(rpc, first)
            temporary = anchor_path.with_suffix('.tmp')
            temporary.write_bytes(s.raw(first['checkpoint_id']))
            os.replace(temporary, anchor_path)
            for field in ('recipient', 'deposit'):
                altered = json.loads(json.dumps(auth))
                target = altered['recipient_bindings'][0] if field == 'recipient' else altered['deposit_registration']
                signature = bytearray(s.raw(target['signature']))
                signature[0] ^= 1
                target['signature'] = p.hx(signature)
                p.atomic_json(inputs / (s.raw(first['checkpoint_id']).hex() + '.json'), altered)
                before = int(rpc.call('eth_getTransactionCount', [submitter.address, 'latest']), 16)
                try:
                    s.publish_native(rpc, first)
                    raise AssertionError('invalid authorization accepted')
                except ValueError as error:
                    assert str(error) == 'publication signature invalid', str(error)
                assert int(rpc.call('eth_getTransactionCount', [submitter.address, 'latest']), 16) == before
            p.atomic_json(inputs / (s.raw(first['checkpoint_id']).hex() + '.json'), auth)
            first_result = publish(first, work, 'publication-1')
            assert first_result['publication']['withdrawal_count'] == 0
            assert first_result['publication']['balance_count'] == 1
            assert first_result['publication']['deposit_count'] == 1
            h1 = s.values(s.HEADER_TYPES, first['header'])
            balances1, _, deposits1, _ = p.native_request(s, first, h1, s.raw(first['checkpoint_id']))
            b1, d1 = balances1[0], deposits1[0]
            authority_public = p.hx(Ed25519PrivateKey.from_private_bytes(bytes([0x77]) * 32).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw))
            first_fields = [url, chain_config['registry'], chain_config['vault'], first['checkpoint_id'], p.hx(b1['account']), p.hx(b1['asset']), p.hx(bytes([0x31]) * 20), p.hx(d1['identity']), p.hx(d1['payer']), str(d1['nonce']), str(int.from_bytes(d1['amount'], 'big')), str(int.from_bytes(b1['amount'], 'big')), authority_public]
            with (work / 'rust-deposit-balance-fetch.log').open('w') as output:
                subprocess.run([str(ROOT / 'human/target/debug/examples/native_publication_fetch'), '--deposit-balance-only', *first_fields], cwd=ROOT, stdout=output, stderr=output, check=True, timeout=120)
            before = int(rpc.call('eth_getTransactionCount', [submitter.address, 'latest']), 16)
            duplicate_first = publish(first, work, 'publication-1-retry')
            assert duplicate_first['already_registered'] and duplicate_first['publication']['version'] == 2
            assert int(rpc.call('eth_getTransactionCount', [submitter.address, 'latest']), 16) == before
            print('first native checkpoint: real deposit and signed balance Rust fetch passed', flush=True)
            assert process.wait(timeout=120) == 0
            second = certificate(replay(native, work, 2), chain_config, url, submitter_path, state, inputs)
            authorize(second, inputs, chain_config['vault'])
            observations = {'first_epoch': h1[2], 'second_epoch': second['header'][2], 'registered_epoch': rpc.view(chain_config['registry'], 'finalisedEpoch()')[0], 'first_batch': h1[3], 'second_batch': second['header'][3], 'withdrawal_count': len(second['native_facts']['withdrawals']), 'first_checkpoint': first['checkpoint_id'], 'second_checkpoint': second['checkpoint_id']}
            p.atomic_json(work / 'checkpoint-order-observed.json', observations)
            result = publish(second, work, 'publication-2')
            assert result['publication']['withdrawal_count'] == 1
            assert result['publication']['balance_count'] == 1
            assert result['publication']['deposit_count'] == 1
            before = int(rpc.call('eth_getTransactionCount', [submitter.address, 'latest']), 16)
            duplicate = publish(second, work, 'publication-2-retry')
            assert duplicate['already_registered'] and duplicate['publication']['version'] == 2
            assert int(rpc.call('eth_getTransactionCount', [submitter.address, 'latest']), 16) == before
            h = s.values(s.HEADER_TYPES, second['header'])
            balances, withdrawals, deposits, _ = p.native_request(s, second, h, s.raw(second['checkpoint_id']))
            b, w, d = balances[0], withdrawals[0], deposits[0]
            namespace = b'system:paxeer-withdrawals'
            withdrawal_account = p.sha(b'LX:ACCOUNT:v1' + len(namespace).to_bytes(4, 'big') + namespace)
            fields = [url, chain_config['registry'], chain_config['vault'], second['checkpoint_id'], p.hx(b['account']), p.hx(b['asset']), p.hx(w['recipient']), p.hx(w['identity']), p.hx(withdrawal_account), p.hx(d['identity']), p.hx(d['payer']), str(d['nonce']), str(int.from_bytes(d['amount'], 'big')), str(int.from_bytes(w['amount'], 'big')), str(int.from_bytes(b['amount'], 'big')), first['checkpoint_id'], p.hx(Ed25519PrivateKey.from_private_bytes(bytes([0x77]) * 32).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw))]
            with (work / 'rust-native-fetch.log').open('w') as output:
                subprocess.run([str(ROOT / 'human/target/debug/examples/native_publication_fetch'), *fields], cwd=ROOT, stdout=output, stderr=output, check=True, timeout=120)
            print('real custody CREDIT and WITHDRAW independently replayed; owner and separate deposit authority signatures published; all three Rust native v2 fetch consumers passed; retry sent no transactions', flush=True)
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=15)
