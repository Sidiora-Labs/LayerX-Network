#!/usr/bin/env python3
"""Disposable Paxeer node for the human withdrawal journey.

Nothing about custody or settlement is simulated here. The chain is a real
``paxd`` started from genesis produced by ``platform/hosted/paxeer/custody-genesis.py``
and ``platform/hosted/paxeer/anchor-genesis.py`` and merged by
``platform/hosted/paxeer/init-chain.sh``, so the custody precompile at
``0x…1013`` is the native ``layerxcustody`` module and the anchor precompile at
``0x…1014`` is the native ``layerxanchor`` module.

The guarantor set is anchor genesis state: the bring-up's
``registerGuarantor``/``activateGuarantor`` pair is not used, the signer this
process generates is written into the genesis section as an already active
guarantor. Checkpoints are submitted through ``submitCheckpoint`` with a real
guarantor attestation encoded by ``cmd/layerx-guarantor/settlement.py``, which
is also what the production guarantor uses.

The process speaks one JSON object per line on stdin and answers one JSON
object per line on stdout:

``start``       initialise genesis, launch paxd, custody the vault float
``checkpoint``  submit and finalize one sequencer-signed batch header
``send``        send one signed EVM transaction to the custody precompile
``cancel``      the custody authority cancels a pending claim (a module message)
``stop``        terminate the node
"""
import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[5]
CUSTODY = '0x0000000000000000000000000000000000001013'
ANCHOR = '0x0000000000000000000000000000000000001014'
CHAIN_ID = 125
COSMOS_CHAIN_ID = 'hyperpax_125-1'
BOND_DENOM = 'uhpx'
UNIT_WEI = 10 ** 12
AVAILABILITY_ALL = 31
FORBIDDEN_PORTS = {18545, 19443, 6379}


def settlement_module():
    """The production guarantor's own checkpoint encoder."""
    path = ROOT / 'cmd/layerx-guarantor/settlement.py'
    spec = importlib.util.spec_from_file_location('layerx_guarantor_settlement', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SETTLEMENT = settlement_module()


def command(*args, **kwargs):
    return subprocess.run([str(argument) for argument in args], cwd=ROOT, check=True, **kwargs)


def free_port():
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        port = reservation.getsockname()[1]
    assert port not in FORBIDDEN_PORTS, 'reserved port'
    return port


def unhex(text, length=None):
    assert isinstance(text, str) and text.startswith('0x'), 'hex encoding required'
    value = bytes.fromhex(text[2:])
    assert length is None or len(value) == length, 'hex length mismatch'
    return value


def enhex(value):
    return '0x' + bytes(value).hex()


def header_decode(encoded):
    """The inverse of ``settlement.header_encode`` for a canonical batch header."""
    assert len(encoded) == 354, 'canonical header length mismatch'
    assert int.from_bytes(encoded[:2], 'big') in (2, 3), 'unsupported protocol version'
    assert encoded[2:5] == bytes.fromhex('17010f'), 'canonical header magic'
    offset, fields = 5, []
    for index, kind in enumerate(SETTLEMENT.HEADER_TYPES, 1):
        assert encoded[offset] == index, 'canonical header field order'
        offset += 1
        if kind == 'bytes32':
            assert int.from_bytes(encoded[offset:offset + 4], 'big') == 32, 'canonical header word'
            offset += 4
            fields.append(encoded[offset:offset + 32])
            offset += 32
        else:
            width = int(kind[4:]) // 8
            fields.append(int.from_bytes(encoded[offset:offset + width], 'big'))
            offset += width
    assert offset == len(encoded), 'canonical header trailing bytes'
    return tuple(fields)


class Node:
    def __init__(self):
        self.process = None
        self.work = None
        self.home = None
        self.url = None
        self.account = None
        self.guarantor_id = None
        self.guarantor_key = None
        self.web3_account = None
        self.rpc_module = None

    # -- chain access ----------------------------------------------------
    def rpc(self, method, params):
        import urllib.request
        payload = json.dumps({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params}).encode()
        request = urllib.request.Request(self.url, payload, {'Content-Type': 'application/json'})
        with urllib.request.urlopen(request, timeout=30) as response:
            answer = json.load(response)
        if 'error' in answer:
            raise RuntimeError('rpc ' + method + ': ' + json.dumps(answer['error']))
        return answer['result']

    def transaction(self, to, data, value=0, success=True):
        nonce = int(self.rpc('eth_getTransactionCount', [self.account.address, 'pending']), 16)
        signed = self.account.sign_transaction({
            'chainId': CHAIN_ID, 'nonce': nonce, 'to': to, 'data': data,
            'gas': 15_000_000, 'gasPrice': int(self.rpc('eth_gasPrice', []), 16), 'value': value,
        })
        digest = self.rpc('eth_sendRawTransaction', [enhex(signed.raw_transaction)])
        receipt = self.await_receipt(digest)
        if success:
            assert int(receipt['status'], 16) == 1, self.revert_reason(receipt, to, data, value)
        return receipt

    def revert_reason(self, receipt, to, data, value):
        # A reverted module call carries its error only in the return data, so the
        # failing transaction is replayed at the block that rejected it.
        call = {'from': self.account.address, 'to': to, 'data': data, 'value': hex(value)}
        try:
            self.rpc('eth_call', [call, receipt['blockNumber']])
        except RuntimeError as failure:
            return str(failure)
        return json.dumps(receipt)

    def await_receipt(self, digest):
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            receipt = self.rpc('eth_getTransactionReceipt', [digest])
            if receipt is not None:
                return receipt
            time.sleep(.1)
        raise RuntimeError('transaction receipt deadline: ' + digest)

    # -- lifecycle -------------------------------------------------------
    def start(self, request):
        from eth_account import Account

        self.work = Path(request['work'])
        self.work.mkdir(parents=True, exist_ok=True)
        self.home = self.work / 'paxd-home'
        self.account = Account.create()
        key_file = self.work / 'deployer.key'
        descriptor = os.open(key_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'wb') as output:
            output.write(bytes(self.account.key))
        guarantor = Account.create()
        self.guarantor_key = guarantor.key
        self.guarantor_id = os.urandom(32)

        sequencer_public = unhex(request['sequencer_public_key'], 32)
        import hashlib
        # lxp_handover_sequencer_id: the identifier every batch header carries.
        sequencer_id = hashlib.sha256(b'layerx-sequencer:' + sequencer_public.hex().encode()).digest()
        asset = unhex(request['asset'], 32)

        custody_genesis = self.work / 'custody-genesis.json'
        command('python3', ROOT / 'platform/hosted/paxeer/custody-genesis.py',
                '--network-id', request['network_id'],
                '--sequencer-id', enhex(sequencer_id),
                '--sequencer-public-key', enhex(sequencer_public),
                '--withdrawal-delay-seconds', request['withdrawal_delay_seconds'],
                '--asset', enhex(asset) + ':' + BOND_DENOM,
                '--authority', self.bech32(self.account.address),
                '--output', custody_genesis)
        anchor_genesis = self.work / 'anchor-genesis.json'
        command('python3', ROOT / 'platform/hosted/paxeer/anchor-genesis.py',
                '--authority-evm', self.account.address,
                '--paxeer-chain-id', CHAIN_ID,
                '--network-id', request['network_id'],
                '--threshold', '1',
                '--challenge-window-seconds', '0',
                '--sequencer-id', enhex(sequencer_id),
                '--sequencer-public-key', enhex(sequencer_public),
                '--guarantor', ':'.join((
                    self.guarantor_id.hex(),
                    guarantor.address.removeprefix('0x').lower(),
                    self.account.address.removeprefix('0x').lower(),
                    str(request['guarantor_bond']))),
                '--output', anchor_genesis)

        ports = {name: free_port() for name in
                 ('EVM', 'EVM_WS', 'RPC', 'P2P', 'GRPC', 'GRPC_WEB', 'API')}
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith('LAYERX_PAXEER_')}
        environment.update(
            LAYERX_PAXEER_HOME=str(self.home), LAYERX_PAXEER_CHAIN_ID=str(CHAIN_ID),
            LAYERX_PAXEER_COMMIT_TIMEOUT_NANOSECONDS='1000000000',
            LAYERX_PAXEER_DEPLOYER_ADDRESS=self.account.address,
            LAYERX_PAXEER_USDL_RUNTIME=str(ROOT / 'platform/hosted/paxeer/contracts/BetaUsdl.runtime.hex'),
            LAYERX_PAXEER_CUSTODY_GENESIS_FILE=str(custody_genesis),
            LAYERX_PAXEER_ANCHOR_GENESIS_FILE=str(anchor_genesis),
            GOMAXPROCS=str(min(4, int(os.environ.get('GOMAXPROCS', '4')))))
        for name, port in ports.items():
            environment['LAYERX_PAXEER_' + name + '_PORT'] = str(port)
        with (self.work / 'paxd-init.log').open('w') as log:
            command('bash', ROOT / 'platform/hosted/paxeer/init-chain.sh',
                    env=environment, stdout=log, stderr=log)
        self.check_escrow_genesis_bond(int(request['guarantor_bond']))
        binary = environment.get('PAXD', 'paxd')
        self.rpc_port = ports['RPC']
        self.custody_authority = self.validator_custody_authority(binary)
        with (self.work / 'paxd.log').open('w') as log:
            self.process = subprocess.Popen(
                [binary, 'start', '--home', str(self.home),
                 '--consensus.create-empty-blocks-interval=1s'],
                cwd=ROOT, env=environment, stdout=log, stderr=log)
        self.url = 'http://127.0.0.1:' + str(ports['EVM'])
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            assert self.process.poll() is None, 'disposable paxd exited; inspect paxd.log'
            try:
                if (int(self.rpc('eth_chainId', []), 16) == CHAIN_ID
                        and int(self.rpc('eth_blockNumber', []), 16) > 0):
                    break
            except (OSError, ValueError, RuntimeError):
                pass
            time.sleep(.2)
        else:
            raise RuntimeError('disposable paxd readiness deadline')

        native = self.rpc('eth_call', [{'to': CUSTODY, 'data': SETTLEMENT.calldata('nativeAssetId()')}, 'latest'])
        assert unhex(native, 32) == asset, 'custody genesis maps another native asset'
        assert int(self.rpc('eth_call', [{'to': ANCHOR, 'data': SETTLEMENT.calldata('threshold()')}, 'latest']), 16) == 1

        beneficiary = unhex(request['beneficiary'], 32)
        deposit = SETTLEMENT.calldata('deposit(bytes32)', ('bytes32',), (beneficiary,))
        self.transaction(CUSTODY, deposit, value=int(request['vault_base_units']) * UNIT_WEI)
        return {'url': self.url, 'chain_id': CHAIN_ID, 'custody': CUSTODY, 'anchor': ANCHOR,
                'comet_chain_id': COSMOS_CHAIN_ID, 'home': str(self.home),
                'deployer': self.account.address, 'guarantor_id': enhex(self.guarantor_id),
                'sequencer_id': enhex(sequencer_id)}

    def validator_custody_authority(self, binary):
        # A custody authority message is a Cosmos transaction, which only a key-derived
        # account can sign; the cast account of an EVM address never can. The validator
        # key init-chain.sh creates is the one funded account this keyring can sign for.
        shown = subprocess.run(
            [binary, 'keys', 'show', 'validator', '-a', '--keyring-backend', 'test', '--home', str(self.home)],
            cwd=ROOT, capture_output=True, text=True, timeout=60, check=True)
        authority = shown.stdout.strip()
        assert authority.startswith('pax1'), 'validator account unavailable'
        genesis_file = self.home / 'config' / 'genesis.json'
        genesis = json.loads(genesis_file.read_text())
        genesis['app_state']['layerxcustody']['params']['authority'] = authority
        genesis_file.write_text(json.dumps(genesis))
        return authority

    def check_escrow_genesis_bond(self, bond):
        # layerxanchor refuses to initialise when its module account does not already
        # hold every genesis bond, so init-chain.sh brings the escrow with a genesis
        # guarantor set. This anchor genesis carries one, so the escrow is the bond.
        import hashlib

        address = self.bech32(hashlib.sha256(b'layerxanchor').digest()[:20].hex())
        genesis_file = self.home / 'config' / 'genesis.json'
        genesis = json.loads(genesis_file.read_text())
        balances = genesis['app_state']['bank']['balances']
        escrow = [entry for entry in balances if entry['address'] == address]
        assert len(escrow) == 1, 'init-chain.sh did not fund the anchor module account'
        assert escrow[0]['coins'] == [{'denom': BOND_DENOM, 'amount': str(bond)}], \
            f'the anchor escrow holds {escrow[0]["coins"]}, not the {bond} {BOND_DENOM} genesis bond'

    @staticmethod
    def bech32(address):
        module = {}
        source = (ROOT / 'platform/hosted/paxeer/anchor-genesis.py').read_text()
        exec(compile(source.split('def main(')[0], 'anchor-genesis', 'exec'), module)
        return module['bech32']('pax', bytes.fromhex(address.removeprefix('0x').lower()))

    # -- settlement ------------------------------------------------------
    def checkpoint(self, request):
        """Submits one sequencer-signed header with a real guarantor attestation."""
        from eth_abi.packed import encode_packed
        from eth_keys import keys
        import hashlib

        encoded = unhex(request['header'])
        signature = unhex(request['header_signature'], 64)
        header = header_decode(encoded)
        checkpoint_id = SETTLEMENT.checkpoint_hash(header, b'')
        attestation = (header[0], header[1], CHAIN_ID, ANCHOR, header[2], checkpoint_id,
                       checkpoint_id, self.guarantor_id, header[3], header[11], True, True,
                       AVAILABILITY_ALL, header[13] + 1)
        digest = hashlib.sha256(b'LXP/v2/guarantor-attestation\0'
                                + encode_packed(list(SETTLEMENT.ATTESTATION_TYPES[:14]), list(attestation))).digest()
        signed = keys.PrivateKey(bytes(self.guarantor_key)).sign_msg_hash(digest)
        signer = keys.PrivateKey(bytes(self.guarantor_key)).public_key.to_checksum_address()
        attestation = attestation + (signer, signed.r.to_bytes(32, 'big'),
                                     signed.s.to_bytes(32, 'big'), signed.v + 27)
        data = SETTLEMENT.submit_calldata(header, signature, b'', [attestation], 1)
        receipt = self.transaction(ANCHOR, data)
        batch = header[3]
        root = self.rpc('eth_call', [{'to': ANCHOR, 'data': SETTLEMENT.calldata(
            'finalizedStateRoot(uint64)', ('uint64',), (batch,))}, 'latest'])
        finalized = unhex(root)
        return {'batch_number': batch, 'transaction': receipt['transactionHash'],
                'state_root': enhex(finalized[:32]), 'final': bool(int.from_bytes(finalized[32:], 'big'))}

    # -- custody ---------------------------------------------------------
    def send(self, request):
        receipt = self.transaction(CUSTODY, request['calldata'], success=False)
        return {'transaction': receipt['transactionHash'], 'status': int(receipt['status'], 16)}

    def cancel(self, request):
        """The custody authority's cancellation: a module message, never an EVM call."""
        binary = os.environ.get('PAXD', 'paxd')
        node = 'tcp://127.0.0.1:' + str(self.rpc_port)
        unsigned = self.work / ('cancel-' + request['claim_id'].removeprefix('0x')[:16] + '.json')
        unsigned.write_text(json.dumps({
            'body': {'messages': [{'@type': '/paxprotocol.paxchain.layerxcustody.MsgCancelClaim',
                                   'authority': self.custody_authority,
                                   'claim_id': request['claim_id'].removeprefix('0x').lower()}],
                     'memo': '', 'timeout_height': '0', 'extension_options': [],
                     'non_critical_extension_options': []},
            'auth_info': {'signer_infos': [], 'fee': {'amount': [{'denom': BOND_DENOM, 'amount': '20000'}],
                                                      'gas_limit': '1000000', 'payer': '', 'granter': ''}},
            'signatures': []}))
        common = ['--home', str(self.home), '--chain-id', COSMOS_CHAIN_ID, '--keyring-backend', 'test', '--node', node]
        signed = subprocess.run([binary, 'tx', 'sign', str(unsigned), '--from', 'validator'] + common,
                                cwd=ROOT, capture_output=True, text=True, timeout=120, check=False)
        if signed.returncode != 0:
            return {'exit_code': signed.returncode, 'stdout': signed.stdout[-4096:], 'stderr': signed.stderr[-4096:]}
        signed_file = unsigned.with_suffix('.signed.json')
        signed_file.write_text(signed.stdout if signed.stdout.strip() else signed.stderr)
        result = subprocess.run(
            [binary, 'tx', 'broadcast', str(signed_file), '--broadcast-mode', 'block', '--node', node,
             '--home', str(self.home), '--output', 'json'],
            cwd=ROOT, capture_output=True, text=True, timeout=120, check=False)
        code = result.returncode
        if code == 0:
            try:
                code = int(json.loads(result.stdout)['code'])
            except (ValueError, KeyError):
                code = 1
        return {'exit_code': code, 'stdout': result.stdout[-4096:], 'stderr': result.stderr[-4096:]}

    def stop(self, _request):
        if self.process is not None and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        return {'stopped': True}


def main():
    node = Node()
    handlers = {'start': node.start, 'checkpoint': node.checkpoint, 'send': node.send,
                'cancel': node.cancel, 'stop': node.stop}
    try:
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            request = json.loads(line)
            try:
                answer = {'ok': True, 'result': handlers[request['command']](request)}
            except Exception as error:  # noqa: BLE001 - reported to the Rust harness verbatim
                answer = {'ok': False, 'error': f'{type(error).__name__}: {error}'}
            sys.stdout.write(json.dumps(answer) + '\n')
            sys.stdout.flush()
            if request['command'] == 'stop':
                break
    finally:
        node.stop({})


if __name__ == '__main__':
    main()
