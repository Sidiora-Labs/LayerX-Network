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
ANCHOR = '0x0000000000000000000000000000000000001014'
UNIT_WEI = 10 ** 12
CHECKPOINT_TUPLE = '(uint64,bytes32,bytes32,uint64,uint64,uint64,bytes32,bytes32,bytes32,bytes32,bytes32,uint64,uint8,uint8,uint8,uint32,uint64,uint64)'
GUARANTOR_TUPLE = '(bytes32,address,address,uint256,uint256,uint8,bool)'
SUBMIT = 'submitCheckpoint(bytes,bytes,bytes)'


def topic(signature):
    return '0x' + keccak(text=signature).hex()


SUBMITTED_EVENT = topic('CheckpointSubmitted(uint64,bytes32,bytes32,bytes32,uint8)')
FINALIZED_EVENT = topic('CheckpointFinalized(uint64,bytes32,bytes32,bytes32)')
REGISTERED_EVENT = topic('GuarantorRegistered(bytes32,address,address,uint256,uint8)')
ACTIVATED_EVENT = topic('GuarantorActivated(bytes32)')
BOND_EVENT = topic('BondIncreased(bytes32,uint256,uint256)')
UNBOND_EVENT = topic('UnbondBegun(bytes32,uint256,uint64)')
SLASHED_EVENT = topic('GuarantorSlashed(bytes32,uint8,uint64,uint256,address,uint256)')
EQUIVOCATION_KIND = 1
MEMBERSHIP_EVENTS = [REGISTERED_EVENT, ACTIVATED_EVENT, BOND_EVENT, UNBOND_EVENT, SLASHED_EVENT]
GUARANTOR_ACTIVE = 2
STATUS_UNKNOWN, STATUS_SUBMITTED, STATUS_FINAL = 0, 1, 2
# A checkpoint whose publication authorization has not arrived yet is not a refused checkpoint: the
# registration stands and the producer asks again. settlement.c maps this exit status to
# LXP_ERR_NOT_YET_VALID so the guarantor waits instead of terminating.
AUTHORIZATION_PENDING_EXIT = 75


class AuthorizationPending(Exception):
    """The owner and checkpoint-authority signatures for this checkpoint are not delivered yet."""


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


def header_encode(header):
    require(header[0] in (2, 3), 'unsupported protocol version')
    encoded = header[0].to_bytes(2, 'big') + bytes.fromhex('17010f')
    for index, (kind, value) in enumerate(zip(HEADER_TYPES, header), 1):
        encoded += bytes([index])
        if kind == 'bytes32':
            encoded += (32).to_bytes(4, 'big') + value
        else:
            encoded += value.to_bytes(int(kind[4:]) // 8, 'big')
    require(len(encoded) == 354, 'canonical header length mismatch')
    return encoded


def checkpoint_hash(header, proof):
    return hashlib.sha256(b'LXP/v2/checkpoint-certificate\0' + header_encode(header) + len(proof).to_bytes(4, 'big') + proof).digest()


def attestation_encode(attestation):
    encoded = encode_packed(ATTESTATION_TYPES, attestation)
    require(len(encoded) == 274, 'attestation wire length mismatch')
    return encoded


def certificate_encode(header, proof, attestations, threshold):
    require(0 < threshold <= len(attestations) <= 32, 'certificate threshold out of range')
    encoded = header_encode(header)
    return ((1).to_bytes(2, 'big') + len(encoded).to_bytes(4, 'big') + encoded + len(proof).to_bytes(4, 'big') + proof
            + bytes([len(attestations)]) + b''.join(attestation_encode(a) for a in attestations) + bytes([threshold]) + (0).to_bytes(2, 'big'))


def submit_calldata(header, header_signature, proof, attestations, threshold):
    require(len(header_signature) == 64 and any(header_signature), 'sequencer header signature invalid')
    return calldata(SUBMIT, ('bytes', 'bytes', 'bytes'), (header_encode(header), header_signature, certificate_encode(header, proof, attestations, threshold)))


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
        self.timings = {}

    def report_timing(self, stage, started):
        if os.environ.get('LAYERX_GUARANTOR_TIMING') == '1':
            metrics = ','.join(f'{name}:{count}:{elapsed:.3f}' for name, (count, elapsed) in sorted(self.timings.items()))
            print(f'settlement timing stage={stage} seconds={time.monotonic() - started:.3f} rpc={metrics}', file=sys.stderr, flush=True)

    def call(self, method, params):
        started = time.monotonic()
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
            refusal = 'RPC rejected ' + method
            error = result.get('error')
            if isinstance(error, dict):
                if isinstance(error.get('code'), int) and not isinstance(error['code'], bool):
                    refusal += ' code=' + str(error['code'])
                data = error.get('data')
                if isinstance(data, str) and data.startswith('0x') and len(data) >= 10 and all(
                    character in '0123456789abcdefABCDEF' for character in data[2:10]
                ):
                    refusal += ' selector=' + data[:10]
            require(result.get('jsonrpc') == '2.0' and result.get('id') == self.counter and 'error' not in result and 'result' in result, refusal)
            return result['result']
        finally:
            conn.close()
            count, elapsed = self.timings.get(method, (0, 0.0))
            self.timings[method] = count + 1, elapsed + time.monotonic() - started

    def view(self, address, signature, inputs=(), args=(), outputs=('uint256',), block='latest'):
        return decode(outputs, raw(self.call('eth_call', [{'to': address, 'data': calldata(signature, inputs, args)}, block])))


def require_anchor(request):
    require(raw(request['settlement_contract'], 20) == raw(ANCHOR, 20), 'settlement contract is not the anchor precompile')
    require(raw(request['checkpoint_registry'], 20) == raw(ANCHOR, 20), 'checkpoint registry is not the anchor precompile')


def anchor_domain(request):
    require(isinstance(request.get('settlement_file'), str) and isinstance(request.get('settlement_domain'), str), 'anchor policy unavailable')
    document = json.loads(Path(request['settlement_file']).read_text())
    require(document['schema'] == 'layerx/checkpoint-settlement/1', 'settlement file schema mismatch')
    domain = document['settlement_domains'][request['settlement_domain']]
    require(raw(domain['settlement_contract'], 20) == raw(ANCHOR, 20) and raw(domain['guarantor_bond'], 20) == raw(ANCHOR, 20), 'settlement domain is not the anchor precompile')
    for key in ('minimum_bond', 'maximum_attestation_delay_ms'):
        require(isinstance(domain.get(key), int) and not isinstance(domain[key], bool), 'anchor policy field missing')
    require(0 < domain['minimum_bond'] < 2 ** 128 and 0 < domain['maximum_attestation_delay_ms'] < 2 ** 64, 'anchor policy out of range')
    return domain


def canonical_block(rpc, number):
    block = rpc.call('eth_getBlockByNumber', [hex(number), False])
    require(isinstance(block, dict) and int(block['number'], 16) == number and any(raw(block['hash'], 32)), 'block unavailable')
    return block


def membership_events(rpc, begin, end):
    count = 0
    while begin <= end:
        last = min(begin + 255, end)
        logs = rpc.call('eth_getLogs', [{'address': ANCHOR, 'fromBlock': hex(begin), 'toBlock': hex(last), 'topics': [MEMBERSHIP_EVENTS]}])
        require(isinstance(logs, list), 'membership log query failed')
        for log in logs:
            require(log['address'].lower() == ANCHOR and log['removed'] is False and log['topics'][0] in MEMBERSHIP_EVENTS
                    and begin <= int(log['blockNumber'], 16) <= last, 'membership log mismatch')
        count += len(logs)
        begin = last + 1
    return count


def membership_version(rpc, request, block_number):
    first = rpc.call('eth_getBlockByNumber', ['earliest', False])
    require(isinstance(first, dict), 'initial block unavailable')
    begin, count = int(first['number'], 16), 0
    require(0 <= begin <= block_number < 2 ** 64, 'membership block out of range')
    cursor_path = None
    if isinstance(request.get('state_dir'), str):
        cursor_path = Path(request['state_dir']) / 'anchor-membership-cursor.json'
        if cursor_path.is_file():
            cursor = json.loads(cursor_path.read_text())
            if (cursor.get('chain_id') == request['chain_id'] and isinstance(cursor.get('block'), int) and begin <= cursor['block'] <= block_number
                    and raw(canonical_block(rpc, cursor['block'])['hash'], 32) == raw(cursor['hash'], 32)):
                begin, count = cursor['block'] + 1, cursor['count']
    count += membership_events(rpc, begin, block_number)
    require(count < 2 ** 64 - 1, 'membership version overflow')
    if cursor_path is not None and block_number >= begin:
        temporary = cursor_path.with_suffix('.json.%d.tmp' % os.getpid())
        temporary.write_text(json.dumps({'chain_id': request['chain_id'], 'block': block_number, 'hash': canonical_block(rpc, block_number)['hash'], 'count': count}))
        os.replace(temporary, cursor_path)
    return count + 1


def guarantor_record(rpc, identity, block='latest'):
    record = rpc.view(ANCHOR, 'guarantor(bytes32)', ('bytes32',), (identity,), (GUARANTOR_TUPLE,), block)[0]
    require(record[0] == identity, 'guarantor is not registered')
    require(record[3] < 2 ** 128 and record[4] < 2 ** 128, 'bond amount exceeds native uint128')
    return record


def membership(rpc, request, block='latest'):
    if block == 'latest':
        block = rpc.call('eth_blockNumber', [])
    block_number = int(block, 16)
    require(block_number > 0, 'membership block number invalid')
    chain = int(rpc.call('eth_chainId', []), 16)
    require(chain == request['chain_id'], 'chain id mismatch')
    require_anchor(request)
    domain = anchor_domain(request)
    require(domain['paxeer_chain_id'] == chain, 'settlement domain chain mismatch')
    minimum_bond = domain['minimum_bond']
    threshold = rpc.view(ANCHOR, 'threshold()', outputs=('uint32',), block=block)[0]
    require(0 < threshold <= 32, 'anchor threshold out of range')
    members = []
    for member in request['guarantors']:
        record = guarantor_record(rpc, raw(member['guarantor_id'], 32), block)
        require(record[1].lower() == member['signer'].lower() and record[5] == GUARANTOR_ACTIVE and record[6] is True, 'unbonded guarantor')
        require(record[3] >= minimum_bond, 'guarantor bond below the settlement domain minimum')
        members.append(dict(member, bonded_active=True, bond_amount=record[3], joined_epoch=1, authorization_version=1))
    version = membership_version(rpc, request, block_number)
    return {'minimum_bond': minimum_bond, 'version': version, 'threshold': threshold, 'maximum_attestation_delay_ms': domain['maximum_attestation_delay_ms'], 'block_number': block_number, 'governance_sequence': 0, 'custodied_value': minimum_bond, 'minimum_bond_bps': 10_000, 'members': members}


def anchor_checkpoint(rpc, batch, block='latest'):
    status = rpc.view(ANCHOR, 'statusOf(uint64)', ('uint64',), (batch,), ('uint8',), block)[0]
    require(status in (STATUS_UNKNOWN, STATUS_SUBMITTED, STATUS_FINAL), 'anchor checkpoint status invalid')
    if status == STATUS_UNKNOWN:
        return status, None
    record = rpc.view(ANCHOR, 'checkpoint(uint64)', ('uint64',), (batch,), (CHECKPOINT_TUPLE,), block)[0]
    require(record[0] == batch and record[12] == status, 'anchor checkpoint record mismatch')
    return status, record


def require_checkpoint(record, digest, header, signers):
    require(record is not None and record[1] == digest and record[3] == header[2] and record[4] == header[4] and record[5] == header[5]
            and record[6] == header[6] and record[7] == header[7] and record[8] == header[9] and record[9] == header[11]
            and record[10] == header[14] and record[11] == header[13] and record[13] == signers, 'anchor checkpoint differs')


def validate_receipt(receipt, transaction, digest, header, signers):
    require(receipt and int(receipt['status'], 16) == 1, 'registration transaction failed')
    require(raw(receipt['transactionHash'], 32) == raw(transaction, 32) and receipt['to'].lower() == ANCHOR, 'receipt transaction mismatch')
    block = int(receipt['blockNumber'], 16)
    block_hash = raw(receipt['blockHash'], 32)
    require(block > 0 and any(block_hash), 'receipt block invalid')
    indexed = ['0x' + encode(['uint64'], [header[3]]).hex(), '0x' + digest.hex()]
    expected = {SUBMITTED_EVENT: encode(['bytes32', 'bytes32', 'uint8'], [header[7], header[9], signers]),
                FINALIZED_EVENT: encode(['bytes32', 'bytes32'], [header[7], header[9]])}
    found = {SUBMITTED_EVENT: 0, FINALIZED_EVENT: 0}
    for log in receipt['logs']:
        if log['address'].lower() == ANCHOR and log['topics'] and log['topics'][0] in expected and log['topics'][1:] == indexed:
            require(log['removed'] is False and raw(log['data']) == expected[log['topics'][0]], 'registration event content mismatch')
            require(raw(log['transactionHash'], 32) == raw(transaction, 32) and raw(log['blockHash'], 32) == block_hash and int(log['blockNumber'], 16) == block, 'registration event receipt mismatch')
            found[log['topics'][0]] += 1
    require(found[SUBMITTED_EVENT] == 1 and found[FINALIZED_EVENT] <= 1, 'registration event count mismatch')
    return block, found[FINALIZED_EVENT] == 1


def bounded_event_logs(rpc, target, topics):
    first = rpc.call('eth_getBlockByNumber', ['earliest', False])
    require(isinstance(first, dict), 'initial block unavailable')
    begin = int(first['number'], 16)
    end = int(rpc.call('eth_blockNumber', []), 16)
    require(0 <= begin <= end < 2 ** 64 and any(raw(first['hash'], 32)), 'initial block invalid')
    found = []
    while begin <= end:
        last = min(begin + 255, end)
        logs = rpc.call('eth_getLogs', [{'address': target, 'fromBlock': hex(begin),
            'toBlock': hex(last), 'topics': topics}])
        require(isinstance(logs, list) and len(found) + len(logs) <= 1, 'matching event count mismatch')
        if logs:
            require(begin <= int(logs[0]['blockNumber'], 16) <= last, 'matching event block mismatch')
            found.extend(logs)
        begin = last + 1
    return found


def registered_transaction(rpc, record, digest):
    height = record[16]
    require(height > 0, 'anchor submission height invalid')
    topics = [SUBMITTED_EVENT, '0x' + encode(['uint64'], [record[0]]).hex(), '0x' + digest.hex()]
    logs = rpc.call('eth_getLogs', [{'address': ANCHOR, 'fromBlock': hex(height), 'toBlock': hex(height), 'topics': topics}])
    require(isinstance(logs, list) and len(logs) >= 1, 'existing registration event count mismatch')
    log = logs[-1]
    require(log['removed'] is False and log['topics'] == topics and int(log['blockNumber'], 16) == height, 'existing registration event mismatch')
    raw(log['transactionHash'], 32)
    return log['transactionHash']


def submitter(request):
    key_path = Path(request['submitter_key_file'])
    require(key_path.is_file() and (key_path.stat().st_mode & 0o077) == 0, 'submitter key permissions must be private')
    try:
        return Account.from_key(key_path.read_text().strip())
    except Exception:
        raise ValueError('submitter key invalid') from None


def send(rpc, request, data, value=0):
    account = submitter(request)
    tx = {'chainId': request['chain_id'], 'nonce': int(rpc.call('eth_getTransactionCount', [account.address, 'pending']), 16), 'to': to_checksum_address(ANCHOR), 'data': data, 'value': value, 'gasPrice': int(rpc.call('eth_gasPrice', []), 16)}
    tx['gas'] = int(rpc.call('eth_estimateGas', [dict(tx, **{'from': account.address, 'nonce': hex(tx['nonce']), 'value': hex(value), 'gasPrice': hex(tx['gasPrice']), 'chainId': hex(tx['chainId'])})]), 16)
    signed = account.sign_transaction(tx)
    transaction = rpc.call('eth_sendRawTransaction', ['0x' + bytes(signed.raw_transaction).hex()])
    require(raw(transaction, 32) == bytes(signed.hash), 'submitted transaction hash mismatch')
    return transaction


def wait_receipt(rpc, transaction, refusal):
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        receipt = rpc.call('eth_getTransactionReceipt', [transaction])
        if receipt is not None:
            return receipt
        time.sleep(0.5)
    raise ValueError(refusal)


def anchor_receipt(rpc, transaction, refusal):
    receipt = wait_receipt(rpc, transaction, refusal + ' receipt timeout')
    require(int(receipt['status'], 16) == 1 and receipt['to'].lower() == ANCHOR and raw(receipt['transactionHash'], 32) == raw(transaction, 32), refusal + ' transaction failed')
    block = int(receipt['blockNumber'], 16)
    require(raw(canonical_block(rpc, block)['hash'], 32) == raw(receipt['blockHash'], 32), refusal + ' block is not canonical')
    return receipt, block


def register(rpc, request):
    h = values(HEADER_TYPES, request['header'])
    attestations = [values(ATTESTATION_TYPES, a) for a in request['attestations']]
    proof = raw(request['validity_proof'])
    digest = checkpoint_hash(h, proof)
    require(digest == raw(request['checkpoint_id'], 32), 'checkpoint id mismatch')
    require_anchor(request)
    threshold = request['threshold']
    require(isinstance(threshold, int) and not isinstance(threshold, bool), 'certificate threshold invalid')
    query = dict(request, epoch=h[2], guarantors=[{'guarantor_id': '0x' + a[7].hex(), 'signer': a[14]} for a in attestations])
    state = membership(rpc, query)
    require(threshold == state['threshold'] and threshold <= len(attestations) <= 32, 'attestation threshold mismatch')
    previous = bytes(32)
    for a in attestations:
        require(a[7] > previous, 'guarantor ids not strictly ascending')
        validate_attestation(a, h, digest, request['chain_id'], ANCHOR, state['maximum_attestation_delay_ms'])
        previous = a[7]
    data = submit_calldata(h, raw(request['header_signature'], 64), proof, attestations, threshold)
    require(raw(request['submit_calldata']) == raw(data), 'native submit calldata differs')
    status, record = anchor_checkpoint(rpc, h[3])
    registered = status != STATUS_UNKNOWN and record[1] == digest
    if registered:
        transaction = registered_transaction(rpc, record, digest)
    else:
        transaction = send(rpc, request, data)
    receipt = wait_receipt(rpc, transaction, 'registration receipt timeout')
    block, finalized = validate_receipt(receipt, transaction, digest, h, len(attestations))
    chain_block = canonical_block(rpc, block)
    require(raw(chain_block['hash'], 32) == raw(receipt['blockHash'], 32), 'receipt block is not canonical')
    observed_at_ms = int(chain_block['timestamp'], 16) * 1000
    status, record = anchor_checkpoint(rpc, h[3])
    require_checkpoint(record, digest, h, len(attestations))
    require(record[16] == block and (not finalized or status == STATUS_FINAL), 'anchor checkpoint height mismatch')
    finalize_transaction = None
    if status == STATUS_SUBMITTED:
        finalize_data = calldata('finalize(uint64)', ('uint64',), (h[3],))
        try:
            rpc.call('eth_call', [{'from': submitter(request).address, 'to': ANCHOR, 'data': finalize_data}, 'latest'])
            finalizable = True
        except ValueError as refusal:
            require(str(refusal).startswith('RPC rejected eth_call'), str(refusal))
            finalizable = False
        if finalizable:
            finalize_transaction = send(rpc, request, finalize_data)
            anchor_receipt(rpc, finalize_transaction, 'finalization')
            status, record = anchor_checkpoint(rpc, h[3])
            require_checkpoint(record, digest, h, len(attestations))
    state = membership(rpc, query, hex(block))
    return {'already_registered': registered, 'checkpoint_id': '0x' + digest.hex(), 'transaction_id': transaction, 'observed_block_number': block, 'paxeer_chain_id': request['chain_id'], 'settlement_contract': ANCHOR, 'set_version': state['version'], 'observed_at_ms': observed_at_ms, 'members': state['members'], 'anchor_status': status, 'finalize_transaction': finalize_transaction}


def deposit(rpc, request):
    chain = int(rpc.call('eth_chainId', []), 16)
    require(chain == request['chain_id'], 'chain id mismatch')
    require_anchor(request)
    identity = raw(request['guarantor_id'], 32)
    transaction = request['transaction_id']
    raw(transaction, 32)
    receipt = rpc.call('eth_getTransactionReceipt', [transaction])
    require(receipt and int(receipt['status'], 16) == 1, 'bond deposit transaction failed')
    require(raw(receipt['transactionHash'], 32) == raw(transaction, 32) and receipt['to'].lower() == ANCHOR, 'bond deposit receipt mismatch')
    block = int(receipt['blockNumber'], 16)
    block_hash = raw(receipt['blockHash'], 32)
    require(block > 0 and any(block_hash), 'bond deposit block invalid')
    matches = []
    for log in receipt['logs']:
        if log['address'].lower() == ANCHOR and len(log['topics']) >= 2 and log['topics'][0] in (BOND_EVENT, REGISTERED_EVENT) and raw(log['topics'][1], 32) == identity:
            require(log['removed'] is False and len(log['topics']) == (2 if log['topics'][0] == BOND_EVENT else 3), 'bond deposit event content mismatch')
            require(raw(log['transactionHash'], 32) == raw(transaction, 32) and raw(log['blockHash'], 32) == block_hash and int(log['blockNumber'], 16) == block, 'bond deposit event receipt mismatch')
            matches.append(log)
    require(len(matches) == 1, 'bond deposit event count mismatch')
    if matches[0]['topics'][0] == BOND_EVENT:
        amount, total_bond = decode(['uint256', 'uint256'], raw(matches[0]['data']))
    else:
        _, amount, _ = decode(['address', 'uint256', 'uint8'], raw(matches[0]['data']))
        total_bond = amount
    require(0 < amount < 2 ** 128 and amount <= total_bond < 2 ** 128, 'bond deposit amount exceeds native uint128')
    chain_block = canonical_block(rpc, block)
    require(raw(chain_block['hash'], 32) == block_hash, 'bond deposit block is not canonical')
    observed_at_ms = int(chain_block['timestamp'], 16) * 1000
    require(observed_at_ms > 0, 'bond deposit block timestamp invalid')
    record = guarantor_record(rpc, identity, hex(block))
    require(record[5] in (1, GUARANTOR_ACTIVE), 'bond record is not an active guarantor')
    require(record[3] == total_bond, 'bond record total differs from deposit event')
    version = membership_version(rpc, request, block)
    return {'guarantor_id': '0x' + identity.hex(), 'transaction_id': transaction, 'observed_block_number': block, 'observed_at_ms': observed_at_ms, 'membership_version': version, 'amount': amount, 'total_bond': total_bond, 'paxeer_chain_id': request['chain_id'], 'settlement_contract': ANCHOR}


def bond_value(request):
    units = request['bond_units']
    require(isinstance(units, int) and not isinstance(units, bool) and 0 < units < 2 ** 128, 'bond units invalid')
    return units * UNIT_WEI


def bond_transaction(rpc, request, mode):
    chain = int(rpc.call('eth_chainId', []), 16)
    require(chain == request['chain_id'], 'chain id mismatch')
    identity = raw(request['guarantor_id'], 32)
    require(any(identity), 'guarantor id invalid')
    if mode == 'register-guarantor':
        signer = request['signer']
        require(any(raw(signer, 20)), 'guarantor signer invalid')
        data = calldata('registerGuarantor(bytes32,address)', ('bytes32', 'address'), (identity, signer))
    else:
        data = calldata('increaseBond(bytes32)', ('bytes32',), (identity,))
    transaction = send(rpc, request, data, bond_value(request))
    anchor_receipt(rpc, transaction, 'bond')
    return deposit(rpc, dict(request, settlement_contract=ANCHOR, checkpoint_registry=ANCHOR, transaction_id=transaction))


def equivocation(rpc, request):
    chain = int(rpc.call('eth_chainId', []), 16)
    require(chain == request['chain_id'], 'chain id mismatch')
    pair = [values(ATTESTATION_TYPES, a) for a in request['attestations']]
    require(len(pair) == 2 and pair[0][7] == pair[1][7] and pair[0][8] == pair[1][8] and pair[0][:14] != pair[1][:14], 'equivocation requires two conflicting attestations of one guarantor and batch')
    for a in pair:
        require(a[2] == chain and a[3].lower() == ANCHOR, 'attestation domain mismatch')
    data = calldata('submitEquivocation(bytes,bytes)', ('bytes', 'bytes'), tuple(attestation_encode(a) for a in pair))
    transaction = send(rpc, request, data)
    receipt, block = anchor_receipt(rpc, transaction, 'equivocation')
    slashes = [log for log in receipt['logs'] if log['address'].lower() == ANCHOR and log['topics'][:2] == [SLASHED_EVENT, '0x' + pair[0][7].hex()]]
    require(len(slashes) == 1, 'equivocation slash event count mismatch')
    kind, batch, slashed, reporter, reward = decode(['uint8', 'uint64', 'uint256', 'address', 'uint256'], raw(slashes[0]['data']))
    require(kind == EQUIVOCATION_KIND and batch == pair[0][8], 'equivocation slash event mismatch')
    return {'guarantor_id': '0x' + pair[0][7].hex(), 'transaction_id': transaction, 'observed_block_number': block, 'batch_number': batch, 'slashed': slashed, 'reporter': reporter, 'reward': reward}


def challenge(rpc, request):
    chain = int(rpc.call('eth_chainId', []), 16)
    require(chain == request['chain_id'], 'chain id mismatch')
    batch, kind = request['batch_number'], request['kind']
    require(isinstance(batch, int) and not isinstance(batch, bool) and 0 < batch < 2 ** 64 and kind in (0, 1), 'challenge target invalid')
    evidence = raw(request['evidence_hash'], 32)
    require(any(evidence), 'challenge evidence hash invalid')
    status, _ = anchor_checkpoint(rpc, batch)
    require(status == STATUS_SUBMITTED, 'only a submitted checkpoint can be challenged')
    data = calldata('openChallenge(uint64,uint8,bytes32)', ('uint64', 'uint8', 'bytes32'), (batch, kind, evidence))
    transaction = send(rpc, request, data, bond_value(request))
    receipt, block = anchor_receipt(rpc, transaction, 'challenge')
    opened = [log for log in receipt['logs'] if log['address'].lower() == ANCHOR and len(log['topics']) == 3
              and log['topics'][0] == topic('ChallengeOpened(uint64,uint64,uint8,bytes32,address)') and int(log['topics'][2], 16) == batch]
    require(len(opened) == 1, 'challenge event count mismatch')
    logged_kind, logged_evidence, challenger = decode(['uint8', 'bytes32', 'address'], raw(opened[0]['data']))
    require(logged_kind == kind and logged_evidence == evidence, 'challenge event mismatch')
    return {'challenge_id': int(opened[0]['topics'][1], 16), 'batch_number': batch, 'kind': kind, 'transaction_id': transaction, 'observed_block_number': block, 'challenger': challenger}


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
    if mode in ('deposit', 'register-guarantor', 'increase-bond'):
        return raw(result['guarantor_id'], 32) + raw(result['transaction_id'], 32) + struct.pack('>QQQ', result['observed_block_number'], result['observed_at_ms'], result['membership_version']) + result['amount'].to_bytes(16, 'big') + result['total_bond'].to_bytes(16, 'big')
    output = struct.pack('>QIQI', result['version'], result['threshold'], result['maximum_attestation_delay_ms'], len(result['members'])) + result['minimum_bond'].to_bytes(16, 'big') + struct.pack('>QQ', result['block_number'], result['governance_sequence']) + result['custodied_value'].to_bytes(16, 'big') + struct.pack('>I', result['minimum_bond_bps'])
    for member in result['members']:
        output += raw(member['guarantor_id'], 32) + raw(member['signer'], 20) + bytes([int(member['bonded_active'])]) + member['bond_amount'].to_bytes(16, 'big') + struct.pack('>QQ', member['joined_epoch'], member['authorization_version'])
    return output


def register_with_race_recovery(rpc, request):
    try:
        return register(rpc, request)
    except ValueError:
        status, record = anchor_checkpoint(rpc, values(HEADER_TYPES, request['header'])[3])
        if status == STATUS_UNKNOWN or record[1] != raw(request['checkpoint_id'], 32):
            raise
        return register(rpc, request)


def configuration(request):
    domain = anchor_domain(request)
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


MODES = ('config', 'membership', 'register', 'deposit', 'register-guarantor', 'increase-bond', 'equivocation', 'challenge')


def main():
    require(len(sys.argv) == 4 and sys.argv[1] in MODES, 'usage: settlement.py ' + '|'.join(MODES) + ' INPUT.json OUTPUT.json')
    request = json.loads(Path(sys.argv[2]).read_text())
    started = time.monotonic()
    if sys.argv[1] == 'config':
        result = configuration(request)
    elif sys.argv[1] == 'membership':
        rpc = RPC(request['rpc_url'])
        block = 'latest'
        if 'observed_block_number' in request:
            observed = request['observed_block_number']
            require(isinstance(observed, int) and not isinstance(observed, bool)
                    and 0 < observed < 2 ** 64, 'membership observation block invalid')
            block = hex(observed)
        result = membership(rpc, request, block)
        rpc.report_timing('membership', started)
    elif sys.argv[1] == 'deposit':
        result = deposit(RPC(request['rpc_url']), request)
    elif sys.argv[1] in ('register-guarantor', 'increase-bond', 'equivocation', 'challenge'):
        lock_path = request.get('submitter_lock_file', request['submitter_key_file'] + '.lock')
        lock_fd = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o660)
        with os.fdopen(lock_fd, 'r+') as lock:
            rpc = RPC(request['rpc_url'])
            fcntl.flock(lock, fcntl.LOCK_EX)
            if sys.argv[1] == 'equivocation':
                result = equivocation(rpc, request)
            elif sys.argv[1] == 'challenge':
                result = challenge(rpc, request)
            else:
                result = bond_transaction(rpc, request, sys.argv[1])
    else:
        lock_path = request.get('submitter_lock_file', request['submitter_key_file'] + '.lock')
        lock_fd = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o660)
        with os.fdopen(lock_fd, 'r+') as lock:
            rpc = RPC(request['rpc_url'])
            fcntl.flock(lock, fcntl.LOCK_EX)
            rpc.report_timing('submitter-lock', started)
            started = time.monotonic()
            result = register_with_race_recovery(rpc, request)
            rpc.report_timing('register', started)
            if 'native_facts' in request:
                rpc = RPC(request['rpc_url'])
                started = time.monotonic()
                result['publication'] = publish_native(rpc, request)
                rpc.report_timing('publication', started)
    if 'wire_output' in request and sys.argv[1] not in ('equivocation', 'challenge'):
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
    except AuthorizationPending as error:
        print('settlement publication pending: ' + str(error), file=sys.stderr)
        sys.exit(AUTHORIZATION_PENDING_EXIT)
    except Exception as error:
        print('settlement refusal: ' + (str(error) if isinstance(error, ValueError) else type(error).__name__), file=sys.stderr)
        sys.exit(1)
