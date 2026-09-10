#!/usr/bin/env python3
import fcntl
import importlib.util
import hashlib
import http.client
import json
import os
from pathlib import Path
import sys
import struct
import time
from urllib.parse import urlsplit

from eth_abi import encode, decode
from eth_abi.packed import encode_packed
from eth_account import Account
from eth_keys import keys
from eth_keys.constants import SECPK1_N
from eth_utils import keccak, to_checksum_address

HEADER_TYPES = ['uint16', 'uint32'] + ['uint64'] * 4 + ['bytes32'] * 7 + ['uint64', 'bytes32']
ATTESTATION_TYPES = ['uint16', 'uint32', 'uint64', 'address', 'uint64'] + ['bytes32'] * 3 + ['uint64', 'bytes32', 'bool', 'bool', 'uint8', 'uint64', 'address', 'bytes32', 'bytes32', 'uint8']
HEADER = '(' + ','.join(HEADER_TYPES) + ')'
ATTESTATION = '(' + ','.join(ATTESTATION_TYPES) + ')'
EVENT = '0x' + keccak(text='CheckpointRegistered(bytes32,uint64,uint64,uint64,uint64,bytes32,bytes32,bytes32,uint64)').hex()
DEPOSIT_EVENT = '0x' + keccak(text='BondDeposited(bytes32,address,uint256,uint256)').hex()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def raw(value, length=None):
    require(isinstance(value, str) and value.startswith('0x'), 'hex encoding required')
    result = bytes.fromhex(value[2:])
    require(length is None or len(result) == length, 'hex length mismatch')
    return result


def values(types, data):
    require(len(types) == len(data), 'ABI field count mismatch')
    result = tuple(raw(v, int(t[5:])) if t.startswith('bytes') else v for t, v in zip(types, data))
    encode(types, result)
    return result


def calldata(signature, types=(), args=()):
    return '0x' + (keccak(text=signature)[:4] + encode(types, args)).hex()


def checkpoint_hash(header, proof):
    require(header[0] in (2, 3), 'unsupported protocol version')
    encoded = header[0].to_bytes(2, 'big') + bytes.fromhex('17010f')
    for index, (kind, value) in enumerate(zip(HEADER_TYPES, header), 1):
        encoded += bytes([index])
        if kind == 'bytes32':
            encoded += (32).to_bytes(4, 'big') + value
        else:
            encoded += value.to_bytes(int(kind[4:]) // 8, 'big')
    require(len(encoded) == 354, 'canonical header length mismatch')
    return hashlib.sha256(b'LXP/v2/checkpoint-certificate\0' + encoded + len(proof).to_bytes(4, 'big') + proof).digest()


def validate_attestation(a, h, digest, chain_id, settlement, maximum_delay):
    require(a[:2] == h[:2] and a[2] == chain_id and a[3].lower() == settlement.lower(), 'attestation domain mismatch')
    require(a[4] == h[2] and a[5] == digest and a[6] == digest and a[8] == h[3] and a[9] == h[11], 'attestation header mismatch')
    require(a[10] is True and a[11] is True and a[12] == 31, 'attestation duties incomplete')
    require(h[13] <= a[13] <= h[13] + maximum_delay, 'stale attestedAt')
    require(a[17] in (27, 28), 'signature recovery id invalid')
    signature = keys.Signature(vrs=(a[17] - 27, int.from_bytes(a[15], 'big'), int.from_bytes(a[16], 'big')))
    require(0 < signature.s <= SECPK1_N // 2, 'signature not low-s')
    digest_a = hashlib.sha256(b'LXP/v2/guarantor-attestation\0' + encode_packed(ATTESTATION_TYPES[:14], a[:14])).digest()
    require(signature.recover_public_key_from_msg_hash(digest_a).to_checksum_address().lower() == a[14].lower(), 'attestation signature mismatch')


class RPC:
    def __init__(self, url):
        self.url = urlsplit(url)
        require(self.url.scheme in ('http', 'https') and self.url.hostname and not self.url.username and not self.url.password, 'invalid RPC URL')
        require(self.url.scheme == 'https' or self.url.hostname == '127.0.0.1', 'plain RPC must use local relay')
        self.counter = 0

    def call(self, method, params):
        self.counter += 1
        cls = http.client.HTTPSConnection if self.url.scheme == 'https' else http.client.HTTPConnection
        conn = cls(self.url.hostname, self.url.port, timeout=30)
        try:
            conn.request('POST', self.url.path or '/', json.dumps({'jsonrpc': '2.0', 'id': self.counter, 'method': method, 'params': params}), {'Content-Type': 'application/json'})
            response = conn.getresponse()
            require(response.status == 200, 'RPC HTTP failure')
            body = response.read(4_000_001)
            require(len(body) <= 4_000_000, 'RPC response too large')
            result = json.loads(body)
            require(result.get('jsonrpc') == '2.0' and result.get('id') == self.counter and 'error' not in result and 'result' in result, 'RPC rejected ' + method)
            return result['result']
        finally:
            conn.close()

    def view(self, address, signature, inputs=(), args=(), outputs=('uint256',), block='latest'):
        return decode(outputs, raw(self.call('eth_call', [{'to': address, 'data': calldata(signature, inputs, args)}, block])))


def membership(rpc, request, block='latest'):
    if block == 'latest':
        block = rpc.call('eth_blockNumber', [])
    block_number = int(block, 16)
    require(block_number > 0, 'membership block number invalid')
    chain = int(rpc.call('eth_chainId', []), 16)
    require(chain == request['chain_id'], 'chain id mismatch')
    bond, registry = request['settlement_contract'], request['checkpoint_registry']
    raw(bond, 20)
    raw(registry, 20)
    require(rpc.view(registry, 'guarantorEligibility()', outputs=('address',), block=block)[0].lower() == bond.lower(), 'registry settlement binding mismatch')
    minimum_bond = rpc.view(bond, 'minimumBond()', block=block)[0]
    require(minimum_bond < 2 ** 128, 'minimum bond exceeds native uint128')
    custodied_value = rpc.view(bond, 'custodiedValue()', block=block)[0]
    require(0 < custodied_value < 2 ** 128, 'custodied value exceeds native uint128')
    minimum_bond_bps = rpc.view(bond, 'minimumBondBps()', outputs=('uint32',), block=block)[0]
    require(0 < minimum_bond_bps <= 10_000, 'minimum bond basis points out of range')
    governance_sequence = rpc.view(bond, 'lastGovernanceSequence()', outputs=('uint64',), block=block)[0]
    members = []
    for member in request['guarantors']:
        active = rpc.view(bond, 'bondedActive(bytes32,address,uint64)', ('bytes32', 'address', 'uint64'), (raw(member['guarantor_id'], 32), member['signer'], request['epoch']), ('bool',), block)[0]
        require(active, 'unbonded guarantor')
        record = rpc.view(bond, 'bondRecord(bytes32)', ('bytes32',), (raw(member['guarantor_id'], 32),), ('(address,address,uint256,uint64,uint64,uint64,uint64,uint256,bool,bool)',), block)[0]
        authorization = rpc.view(bond, 'signerAuthorization(bytes32,address)', ('bytes32', 'address'), (raw(member['guarantor_id'], 32), member['signer']), ('uint64', 'uint64', 'uint64'), block)
        require(record[0].lower() == member['signer'].lower() and record[4] == 0 and record[5] == 0 and not record[8] and not record[9], 'bond record is not active current signer')
        require(record[2] < 2 ** 128, 'bond amount exceeds native uint128')
        require(authorization[0] == record[3] and authorization[1] == 0, 'rotated signer requires complete authority history')
        members.append(dict(member, bonded_active=active, bond_amount=record[2], joined_epoch=record[3], authorization_version=authorization[2]))
    version = rpc.view(bond, 'membershipVersion()', block=block)[0]
    require(governance_sequence <= version, 'governance sequence exceeds membership version')
    return {'minimum_bond': minimum_bond, 'version': version, 'threshold': rpc.view(registry, 'threshold()', block=block)[0], 'maximum_attestation_delay_ms': rpc.view(registry, 'maximumAttestationDelayMilliseconds()', block=block)[0], 'block_number': block_number, 'governance_sequence': governance_sequence, 'custodied_value': custodied_value, 'minimum_bond_bps': minimum_bond_bps, 'members': members}


def validate_receipt(receipt, registry, transaction, digest, header, version):
    require(receipt and int(receipt['status'], 16) == 1, 'registration transaction failed')
    require(raw(receipt['transactionHash'], 32) == raw(transaction, 32) and receipt['to'].lower() == registry.lower(), 'receipt transaction mismatch')
    block = int(receipt['blockNumber'], 16)
    block_hash = raw(receipt['blockHash'], 32)
    require(block > 0 and any(block_hash), 'receipt block invalid')
    topics = [EVENT, '0x' + digest.hex(), '0x' + encode(['uint64'], [header[2]]).hex(), '0x' + encode(['uint64'], [header[3]]).hex()]
    data = encode(['uint64', 'uint64', 'bytes32', 'bytes32', 'bytes32', 'uint64'], [header[4], header[5], header[6], header[7], header[11], version])
    matches = []
    for log in receipt['logs']:
        if log['address'].lower() == registry.lower() and log['topics'] == topics:
            require(log['removed'] is False and raw(log['data']) == data, 'registration event content mismatch')
            require(raw(log['transactionHash'], 32) == raw(transaction, 32) and raw(log['blockHash'], 32) == block_hash and int(log['blockNumber'], 16) == block, 'registration event receipt mismatch')
            matches.append(log)
    require(len(matches) == 1, 'registration event count mismatch')
    return block


def register(rpc, request):
    h = values(HEADER_TYPES, request['header'])
    attestations = [values(ATTESTATION_TYPES, a) for a in request['attestations']]
    proof = raw(request['validity_proof'])
    digest = checkpoint_hash(h, proof)
    require(digest == raw(request['checkpoint_id'], 32), 'checkpoint id mismatch')
    registry = request['checkpoint_registry']
    query = dict(request, epoch=h[2], guarantors=[{'guarantor_id': '0x' + a[7].hex(), 'signer': a[14]} for a in attestations])
    state = membership(rpc, query)
    require(len(attestations) >= state['threshold'] and len(attestations) <= rpc.view(registry, 'maximumAttestations()')[0], 'attestation threshold mismatch')
    previous = bytes(32)
    for a in attestations:
        require(a[7] > previous, 'guarantor ids not strictly ascending')
        validate_attestation(a, h, digest, request['chain_id'], request['settlement_contract'], state['maximum_attestation_delay_ms'])
        previous = a[7]
    require(rpc.view(registry, 'checkpointHash(' + HEADER + ',bytes)', (HEADER, 'bytes'), (h, proof), ('bytes32',))[0] == digest, 'contract checkpoint hash mismatch')
    registered = rpc.view(registry, 'registeredAt(bytes32)', ('bytes32',), (digest,))[0] != 0
    if registered:
        require(rpc.view(registry, 'isRecordedCertificate(bytes32,' + ATTESTATION + '[])', ('bytes32', ATTESTATION + '[]'), (digest, attestations), ('bool',))[0], 'registered certificate differs')
        logs = rpc.call('eth_getLogs', [{'address': registry, 'fromBlock': '0x0', 'toBlock': 'latest', 'topics': [EVENT, '0x' + digest.hex()]}])
        require(len(logs) == 1, 'existing registration event count mismatch')
        transaction = logs[0]['transactionHash']
    else:
        key_path = Path(request['submitter_key_file'])
        require(key_path.is_file() and (key_path.stat().st_mode & 0o077) == 0, 'submitter key permissions must be private')
        try:
            account = Account.from_key(key_path.read_text().strip())
        except Exception:
            raise ValueError('submitter key invalid') from None
        data = calldata('registerCheckpoint(' + HEADER + ',bytes,' + ATTESTATION + '[])', (HEADER, 'bytes', ATTESTATION + '[]'), (h, proof, attestations))
        tx = {'chainId': request['chain_id'], 'nonce': int(rpc.call('eth_getTransactionCount', [account.address, 'pending']), 16), 'to': to_checksum_address(registry), 'data': data, 'value': 0, 'gasPrice': int(rpc.call('eth_gasPrice', []), 16)}
        tx['gas'] = int(rpc.call('eth_estimateGas', [dict(tx, **{'from': account.address, 'nonce': hex(tx['nonce']), 'value': '0x0', 'gasPrice': hex(tx['gasPrice']), 'chainId': hex(tx['chainId'])})]), 16)
        signed = account.sign_transaction(tx)
        transaction = rpc.call('eth_sendRawTransaction', ['0x' + bytes(signed.raw_transaction).hex()])
        require(raw(transaction, 32) == bytes(signed.hash), 'submitted transaction hash mismatch')
    deadline = time.monotonic() + 120
    receipt = None
    while time.monotonic() < deadline:
        receipt = rpc.call('eth_getTransactionReceipt', [transaction])
        if receipt is not None:
            break
        time.sleep(0.5)
    require(receipt is not None, 'registration receipt timeout')
    version = rpc.view(registry, 'checkpointGuarantorSetVersion(bytes32)', ('bytes32',), (digest,))[0]
    block = validate_receipt(receipt, registry, transaction, digest, h, version)
    require(rpc.view(registry, 'isCanonicalCheckpoint(bytes32)', ('bytes32',), (digest,), ('bool',))[0], 'registered checkpoint invalidated')
    require(rpc.view(registry, 'isRecordedCertificate(bytes32,' + ATTESTATION + '[])', ('bytes32', ATTESTATION + '[]'), (digest, attestations), ('bool',))[0], 'registered certificate differs')
    chain_block = rpc.call('eth_getBlockByNumber', [hex(block), False])
    require(chain_block and raw(chain_block['hash'], 32) == raw(receipt['blockHash'], 32), 'receipt block is not canonical')
    observed_at_ms = int(chain_block['timestamp'], 16) * 1000
    state = membership(rpc, query, hex(block))
    require(state['version'] == version, 'registered membership version mismatch')
    return {'already_registered': registered, 'checkpoint_id': '0x' + digest.hex(), 'transaction_id': transaction, 'observed_block_number': block, 'paxeer_chain_id': request['chain_id'], 'settlement_contract': request['settlement_contract'], 'set_version': version, 'observed_at_ms': observed_at_ms, 'members': state['members']}


def deposit(rpc, request):
    chain = int(rpc.call('eth_chainId', []), 16)
    require(chain == request['chain_id'], 'chain id mismatch')
    bond = request['settlement_contract']
    raw(bond, 20)
    identity = raw(request['guarantor_id'], 32)
    transaction = request['transaction_id']
    raw(transaction, 32)
    receipt = rpc.call('eth_getTransactionReceipt', [transaction])
    require(receipt and int(receipt['status'], 16) == 1, 'bond deposit transaction failed')
    require(raw(receipt['transactionHash'], 32) == raw(transaction, 32) and receipt['to'].lower() == bond.lower(), 'bond deposit receipt mismatch')
    block = int(receipt['blockNumber'], 16)
    block_hash = raw(receipt['blockHash'], 32)
    require(block > 0 and any(block_hash), 'bond deposit block invalid')
    topics = [DEPOSIT_EVENT, '0x' + identity.hex()]
    matches = []
    for log in receipt['logs']:
        if log['address'].lower() == bond.lower() and log['topics'][:2] == topics:
            require(log['removed'] is False and len(log['topics']) == 3, 'bond deposit event content mismatch')
            require(raw(log['transactionHash'], 32) == raw(transaction, 32) and raw(log['blockHash'], 32) == block_hash and int(log['blockNumber'], 16) == block, 'bond deposit event receipt mismatch')
            matches.append(log)
    require(len(matches) == 1, 'bond deposit event count mismatch')
    amount, total_bond = decode(['uint256', 'uint256'], raw(matches[0]['data']))
    require(0 < amount < 2 ** 128 and amount <= total_bond < 2 ** 128, 'bond deposit amount exceeds native uint128')
    chain_block = rpc.call('eth_getBlockByNumber', [hex(block), False])
    require(chain_block and raw(chain_block['hash'], 32) == block_hash, 'bond deposit block is not canonical')
    observed_at_ms = int(chain_block['timestamp'], 16) * 1000
    require(observed_at_ms > 0, 'bond deposit block timestamp invalid')
    record = rpc.view(bond, 'bondRecord(bytes32)', ('bytes32',), (identity,), ('(address,address,uint256,uint64,uint64,uint64,uint64,uint256,bool,bool)',), hex(block))[0]
    require(int(record[0], 16) != 0 and record[4] == 0 and record[5] == 0, 'bond record is not an active guarantor')
    require(record[2] == total_bond, 'bond record total differs from deposit event')
    version = rpc.view(bond, 'membershipVersion()', block=hex(block))[0]
    require(version > 0, 'bond deposit membership version invalid')
    return {'guarantor_id': '0x' + identity.hex(), 'transaction_id': transaction, 'observed_block_number': block, 'observed_at_ms': observed_at_ms, 'membership_version': version, 'amount': amount, 'total_bond': total_bond, 'paxeer_chain_id': request['chain_id'], 'settlement_contract': bond}


def publish_native(rpc, request):
    module_spec = importlib.util.spec_from_file_location('guarantor_publication', Path(__file__).with_name('publication.py'))
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    from types import SimpleNamespace
    return module.publish(SimpleNamespace(**globals()), rpc, request)


def wire_encode(mode, result):
    if mode == 'config':
        output = struct.pack('>QI', result['chain_id'], result['network_id']) + raw(result['settlement_contract'], 20) + raw(result['checkpoint_registry'], 20) + struct.pack('>I', len(result['members']))
        for member in result['members']:
            output += raw(member['guarantor_id'], 32) + raw(member['public_key'], 33)
        return output
    if mode == 'register':
        return bytes([int(result['already_registered'])]) + raw(result['transaction_id'], 32) + struct.pack('>QQQ', result['observed_block_number'], result['observed_at_ms'], result['set_version'])
    if mode == 'deposit':
        return raw(result['guarantor_id'], 32) + raw(result['transaction_id'], 32) + struct.pack('>QQQ', result['observed_block_number'], result['observed_at_ms'], result['membership_version']) + result['amount'].to_bytes(16, 'big') + result['total_bond'].to_bytes(16, 'big')
    output = struct.pack('>QIQI', result['version'], result['threshold'], result['maximum_attestation_delay_ms'], len(result['members'])) + result['minimum_bond'].to_bytes(16, 'big') + struct.pack('>QQ', result['block_number'], result['governance_sequence']) + result['custodied_value'].to_bytes(16, 'big') + struct.pack('>I', result['minimum_bond_bps'])
    for member in result['members']:
        output += raw(member['guarantor_id'], 32) + raw(member['signer'], 20) + bytes([int(member['bonded_active'])]) + member['bond_amount'].to_bytes(16, 'big') + struct.pack('>QQ', member['joined_epoch'], member['authorization_version'])
    return output


def register_with_race_recovery(rpc, request):
    try:
        return register(rpc, request)
    except ValueError:
        digest = raw(request['checkpoint_id'], 32)
        registered = rpc.view(request['checkpoint_registry'], 'registeredAt(bytes32)', ('bytes32',), (digest,))[0]
        if registered == 0:
            raise
        return register(rpc, request)


def configuration(request):
    document = json.loads(Path(request['settlement_file']).read_text())
    require(document['schema'] == 'layerx/checkpoint-settlement/1', 'settlement file schema mismatch')
    domain = document['settlement_domains'][request['settlement_domain']]
    result = {'chain_id': domain['paxeer_chain_id'], 'network_id': domain['network_id'], 'settlement_contract': domain['guarantor_bond'], 'checkpoint_registry': domain['settlement_contract'], 'members': domain['guarantor_set']}
    require(int(os.environ['LAYERX_NODE_PAXEER_CHAIN_ID']) == result['chain_id'], 'settlement chain environment mismatch')
    require(raw(os.environ['LAYERX_NODE_SETTLEMENT_CONTRACT'], 20) == raw(result['settlement_contract'], 20), 'settlement bond environment mismatch')
    require(raw(os.environ['LAYERX_NODE_CHECKPOINT_REGISTRY'], 20) == raw(result['checkpoint_registry'], 20), 'settlement registry environment mismatch')
    require(0 < len(result['members']) <= 32, 'settlement member count mismatch')
    previous = bytes(32)
    for member in result['members']:
        identity = raw(member['guarantor_id'], 32)
        require(identity > previous, 'settlement identities not strictly ascending')
        previous = identity
        public = keys.PublicKey.from_compressed_bytes(raw(member['public_key'], 33))
        require(public.to_checksum_address().lower() == member['signer'].lower(), 'settlement public key signer mismatch')
    return result


def main():
    require(len(sys.argv) == 4 and sys.argv[1] in ('membership', 'register', 'config', 'deposit'), 'usage: settlement.py config|membership|register|deposit INPUT.json OUTPUT.json')
    request = json.loads(Path(sys.argv[2]).read_text())
    if sys.argv[1] == 'config':
        result = configuration(request)
    elif sys.argv[1] == 'membership':
        result = membership(RPC(request['rpc_url']), request)
    elif sys.argv[1] == 'deposit':
        result = deposit(RPC(request['rpc_url']), request)
    else:
        lock_path = request.get('submitter_lock_file', request['submitter_key_file'] + '.lock')
        lock_fd = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o660)
        with os.fdopen(lock_fd, 'r+') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            result = register_with_race_recovery(RPC(request['rpc_url']), request)
            if 'native_facts' in request:
                result['publication'] = publish_native(RPC(request['rpc_url']), request)
    if 'wire_output' in request:
        descriptor = os.open(request['wire_output'], os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'wb') as wire:
            wire.write(wire_encode(sys.argv[1], result))
            wire.flush()
            os.fsync(wire.fileno())
    destination = Path(sys.argv[3])
    temporary = destination.with_suffix(destination.suffix + '.tmp')
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as stream:
        json.dump(result, stream, sort_keys=True)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, destination)


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print('settlement refusal: ' + (str(error) if isinstance(error, ValueError) else type(error).__name__), file=sys.stderr)
        sys.exit(1)
