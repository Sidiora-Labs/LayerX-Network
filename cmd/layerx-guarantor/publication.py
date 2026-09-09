import hashlib
import json
import os
from pathlib import Path
import stat
import time
import tempfile

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey


def require(value, message):
    if not value:
        raise ValueError(message)


def sha(value):
    return hashlib.sha256(value).digest()


def raw(value, length=None):
    require(isinstance(value, str) and value.startswith('0x'), 'publication hex encoding')
    value = bytes.fromhex(value[2:])
    require(length is None or len(value) == length, 'publication hex length')
    return value


def hx(value):
    return '0x' + value.hex()


class Reader:
    def __init__(self, value):
        self.value, self.at = value, 0

    def take(self, length):
        require(0 <= length <= len(self.value) - self.at, 'native witness truncated')
        result = self.value[self.at:self.at + length]
        self.at += length
        return result

    def integer(self, length):
        return int.from_bytes(self.take(length), 'big')

    def vector(self, maximum):
        length = self.integer(4)
        require(length <= maximum, 'native witness length bound')
        return self.take(length)


def state_leaf(key, value):
    return sha(b'LXP/v1/state-leaf\0' + len(key).to_bytes(4, 'big') + len(value).to_bytes(4, 'big') + key + value)


def fold(reader, node, index, count):
    depth = reader.integer(1)
    require(depth <= 32 and 0 <= index < count, 'native witness path bounds')
    for _ in range(depth):
        sibling = reader.take(32)
        require(count > 1 and ((index ^ 1) < count or sibling == node), 'native witness odd sibling')
        node = sha(b'LXP/v1/state-node\0' + (sibling + node if index & 1 else node + sibling))
        index, count = index // 2, (count + 1) // 2
    require(count == 1, 'native witness path depth')
    return node


def witness(encoded, root):
    wire = raw(encoded)
    r = Reader(wire)
    require(r.integer(2) == 2, 'native witness version')
    module = r.integer(2)
    require(module <= 9, 'native witness module')
    key, value = r.vector(129), r.vector(1_048_576)
    require(key, 'native witness empty key')
    node = state_leaf(key, value)
    if module == 0 and len(key) == 33 and key[0] == 4:
        node = fold(r, node, r.integer(4), r.integer(4))
        node = state_leaf(b'account-tree', node)
    node = fold(r, node, r.integer(4), r.integer(4))
    node = state_leaf(module.to_bytes(2, 'big'), node)
    count = r.integer(4)
    require(9 <= count <= 10, 'native witness module count')
    node = fold(r, node, module, count)
    require(r.at == len(wire) and node == root, 'native witness root or trailing data')
    return module, key, value, wire


def balance_fact(encoded, root):
    module, key, value, wire = witness(encoded, root)
    require(module == 0 and len(key) == 33 and key[0] == 4, 'balance account witness')
    n = int.from_bytes(value[:2], 'big')
    at = n + 2
    require(0 < n <= 512 and len(value) == n + 103, 'balance account encoding')
    require(value[at] == 1 and value[at + 49] == 1 and value[at + 100] == 1, 'balance owner authority absent')
    require(value[at + 66] <= 1 and value[at + 67] <= 1, 'balance account flags')
    return dict(account=key[1:], asset=value[at + 17:at + 49], amount=value[at + 1:at + 17], authority=value[at + 68:at + 100], witness=wire)


def withdrawal_fact(encoded, root, network):
    module, key, value, wire = witness(encoded, root)
    require(module == 1 and len(key) == 43 and key[:11] == b'withdrawal:' and len(value) == 182, 'withdrawal native record')
    require(value[:2] == b'\0\2' and int.from_bytes(value[2:6], 'big') == network, 'withdrawal domain')
    identity, account, asset, amount, recipient, anchor = value[6:38], value[38:70], value[70:102], value[102:118], value[118:150], value[150:182]
    require(recipient[:12] == bytes(12) and any(recipient[12:]) and any(anchor) and any(amount), 'withdrawal economic fields')
    nullifier = sha(b'LX:WITHDRAWAL:v1' + network.to_bytes(4, 'big') + identity + account + asset + amount + anchor)
    require(key[11:] == nullifier, 'withdrawal nullifier mismatch')
    leaf = sha(b'LXP/v1/merkle-leaf\0' + identity + account + asset + amount + recipient)
    return dict(identity=identity, account=account, asset=asset, amount=amount, recipient=recipient[12:], anchor=anchor, leaf=leaf, witness=wire)


def signature(key, message, signed):
    require(len(key) == 32 and any(key) and len(signed) == 64, 'publication signature length')
    try:
        Ed25519PublicKey.from_public_bytes(key).verify(signed, message)
    except (InvalidSignature, ValueError):
        raise ValueError('publication signature invalid') from None


def atomic_json(path, value):
    data = (json.dumps(value, sort_keys=True) + '\n').encode()
    fd, temporary_name = tempfile.mkstemp(prefix=path.name + '.', dir=path.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(fd, 'wb') as out:
            out.write(data)
            out.flush()
            os.fsync(out.fileno())
        os.replace(temporary, path)
        parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(parent)
        finally:
            os.close(parent)
    finally:
        if temporary.exists():
            temporary.unlink()


def read_authorizations(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_size <= 1_000_000 and info.st_nlink == 1, 'publication authorization file bounds')
        data = stream.read(1_000_001)
        require(len(data) <= 1_000_000, 'publication authorization file bounds')
    return json.loads(data)


def native_request(api, request, header, checkpoint):
    facts = request['native_facts']
    require(set(facts) == {'balances', 'withdrawals', 'deposits', 'profile'}, 'native facts fields')
    for name in ('balances', 'withdrawals', 'deposits'):
        require(isinstance(facts[name], list) and len(facts[name]) <= 4096, 'native facts bound')
    balances = sorted([balance_fact(v, header[7]) for v in facts['balances']], key=lambda v: (v['account'], v['asset']))
    withdrawals = sorted([withdrawal_fact(v, header[7], header[1]) for v in facts['withdrawals']], key=lambda v: v['identity'])
    require(len({(v['account'], v['asset']) for v in balances}) == len(balances), 'duplicate balance leaf')
    require(len({v['identity'] for v in withdrawals}) == len(withdrawals), 'duplicate withdrawal leaf')
    deposits, profile = [], None
    if facts['profile'] is not None:
        module, profile_key, profile, _ = witness(facts['profile'], header[7])
        require(module == 8 and profile_key == b'custody-credit-profile/v1'.ljust(32, b'\0') and len(profile) == 207 and profile[:5] == b'LXBC1', 'native custody profile')
        require(int.from_bytes(profile[5:13], 'big') == request['chain_id'], 'native custody chain')
    for encoded in facts['deposits']:
        require(profile is not None, 'native custody profile absent')
        module, key, credit, _ = witness(encoded, header[7])
        require(module == 8 and len(key) == 50 and key[:18] == b'deposit-nullifier:' and len(credit) == 427 and credit[:5] == b'LXDC1', 'native custody credit')
        require(credit[5:37] == sha(profile) and credit[37:43] == header[1].to_bytes(4, 'big') + header[0].to_bytes(2, 'big'), 'native custody domain')
        require(key[18:] == sha(b'LX:DEPOSIT:NULLIFIER:v1' + credit[43:75]), 'native deposit nullifier')
        signature(profile[65:97], b'LX:CUSTODY:CREDIT:v1' + credit[:363], credit[363:])
        deposits.append(dict(identity=credit[43:75], asset=credit[75:107], amount=credit[191:207], beneficiary=credit[107:139], payer=credit[171:191], nonce=int.from_bytes(credit[207:215], 'big')))
    deposits.sort(key=lambda v: v['identity'])
    require(len({v['identity'] for v in deposits}) == len(deposits), 'duplicate deposit leaf')
    return balances, withdrawals, deposits, profile


def checkpoint_wire(api, request, h, digest, anchor, proof, signed):
    attestations = b''.join(api.encode_packed(api.ATTESTATION_TYPES, api.values(api.ATTESTATION_TYPES, a)) for a in request['attestations'])
    return b'\x02\x02' + digest + h[7] + h[2].to_bytes(8, 'big') + h[3].to_bytes(8, 'big') + h[11] + bytes(10) + len(request['attestations']).to_bytes(4, 'big') + attestations + anchor + digest + h[1].to_bytes(4, 'big') + len(proof).to_bytes(4, 'big') + proof + len(signed).to_bytes(2, 'big') + signed


def vector(domain, items):
    return domain + len(items).to_bytes(4, 'big') + b''.join(len(v).to_bytes(4, 'big') + v for v in items)


def transact(api, rpc, request, target, data):
    path = Path(request['submitter_key_file'])
    require(path.is_file() and path.stat().st_mode & 0o077 == 0, 'submitter key permissions')
    try:
        account = api.Account.from_key(path.read_text().strip())
    except Exception:
        raise ValueError('submitter key invalid') from None
    tx = {'chainId': request['chain_id'], 'nonce': int(rpc.call('eth_getTransactionCount', [account.address, 'pending']), 16), 'to': api.to_checksum_address(target), 'data': data, 'value': 0, 'gasPrice': int(rpc.call('eth_gasPrice', []), 16)}
    estimate = dict(tx, **{'from': account.address, 'nonce': hex(tx['nonce']), 'value': '0x0', 'gasPrice': hex(tx['gasPrice']), 'chainId': hex(tx['chainId'])})
    tx['gas'] = int(rpc.call('eth_estimateGas', [estimate]), 16)
    signed = account.sign_transaction(tx)
    transaction = rpc.call('eth_sendRawTransaction', [hx(bytes(signed.raw_transaction))])
    require(raw(transaction, 32) == bytes(signed.hash), 'publication transaction mismatch')
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        receipt = rpc.call('eth_getTransactionReceipt', [transaction])
        if receipt is not None:
            require(int(receipt['status'], 16) == 1 and receipt['to'].lower() == target.lower() and raw(receipt['transactionHash'], 32) == raw(transaction, 32), 'publication receipt failed')
            block = rpc.call('eth_getBlockByNumber', [receipt['blockNumber'], False])
            require(block and block['hash'] == receipt['blockHash'], 'publication receipt displaced')
            return transaction
        time.sleep(.5)
    raise ValueError('publication receipt timeout')


def publication_transaction(api, rpc, target, digest, commitment, deposit_root=None):
    event = 'CheckpointWitnessesPublished(bytes32,uint16,bytes32)' if deposit_root is None else 'DepositRootRegistered(bytes32,bytes32,bytes32,uint16)'
    topics = [hx(api.keccak(text=event)), hx(digest)]
    if deposit_root is not None:
        topics.append(hx(deposit_root))
    logs = rpc.call('eth_getLogs', [{'address': target, 'fromBlock': '0x0', 'toBlock': 'latest', 'topics': topics}])
    require(len(logs) == 1, 'publication event count mismatch')
    log = logs[0]
    data = api.encode(['uint16', 'bytes32'], [2, commitment]) if deposit_root is None else api.encode(['bytes32', 'uint16'], [commitment, 2])
    require(log['removed'] is False and log['topics'] == topics and raw(log['data']) == data, 'publication event mismatch')
    receipt = rpc.call('eth_getTransactionReceipt', [log['transactionHash']])
    require(receipt and int(receipt['status'], 16) == 1 and receipt['to'].lower() == target.lower() and receipt['blockHash'] == log['blockHash'] and receipt['blockNumber'] == log['blockNumber'], 'publication event receipt mismatch')
    block = rpc.call('eth_getBlockByNumber', [log['blockNumber'], False])
    require(block and block['hash'] == log['blockHash'], 'publication event displaced')
    return log['transactionHash']


def publish(api, rpc, request):
    h = api.values(api.HEADER_TYPES, request['header'])
    digest = raw(request['checkpoint_id'], 32)
    registry = request['checkpoint_registry']
    balances, withdrawals, deposits, profile = native_request(api, request, h, digest)
    directory = Path(request['publication_state_dir'])
    if not balances and not withdrawals and not deposits:
        evidence = {'version': 2, 'checkpoint_id': hx(digest), 'status': 'no_native_settlement_facts', 'balance_count': 0, 'withdrawal_count': 0, 'deposit_count': 0}
        atomic_json(directory / (digest.hex() + '.evidence.json'), evidence)
        return evidence
    require(balances, 'owner balance leaves required for nonempty settlement publication')
    manifest = directory / (digest.hex() + '.publication-request.json')
    atomic_json(manifest, {k: v for k, v in request.items() if k not in ('submitter_key_file', 'submitter_lock_file', 'wire_output')})
    require(request.get('publication_inputs_dir'), 'publication authorization directory required')
    source = Path(request['publication_inputs_dir']) / (digest.hex() + '.json')
    deadline = time.monotonic() + 30
    while not source.exists() and time.monotonic() < deadline:
        time.sleep(.1)
    require(source.exists(), 'owner and checkpoint-authority signatures unavailable')
    authorization = read_authorizations(source)
    require(authorization['version'] == 2 and raw(authorization['checkpoint_id'], 32) == digest, 'publication authorization version or checkpoint')
    require(len(authorization['recipient_bindings']) == len(balances), 'complete recipient bindings required')
    bound = {}
    for v in authorization['recipient_bindings']:
        key = raw(v['account'], 32), raw(v['asset'], 32)
        require(key not in bound, 'duplicate recipient binding')
        bound[key] = v
    balance_items, withdrawal_items = [], []
    for fact in balances:
        v = bound.get((fact['account'], fact['asset']))
        require(v is not None, 'owner recipient binding absent')
        recipient, anchor, signed = raw(v['recipient'], 20), raw(v['request_anchor'], 32), raw(v['signature'], 64)
        require(any(recipient) and any(anchor), 'recipient binding empty field')
        signature(fact['authority'], b'LX:SETTLE:RECIPIENT:v1\0' + h[1].to_bytes(4, 'big') + fact['account'] + fact['asset'] + recipient + anchor, signed)
        require(rpc.view(registry, 'isRecordedAncestor(bytes32,bytes32)', ('bytes32', 'bytes32'), (anchor, digest), ('bool',))[0], 'recipient anchor not recorded ancestor')
        balance_items.append(fact['account'] + fact['asset'] + fact['amount'] + recipient + checkpoint_wire(api, request, h, digest, anchor, fact['witness'], signed))
    for fact in withdrawals:
        require(rpc.view(registry, 'isRecordedAncestor(bytes32,bytes32)', ('bytes32', 'bytes32'), (fact['anchor'], digest), ('bool',))[0], 'withdrawal anchor not recorded ancestor')
        withdrawal_items.append(fact['identity'] + fact['leaf'] + checkpoint_wire(api, request, h, digest, fact['anchor'], fact['witness'], b''))
    withdrawal_vector = vector(b'LXP/Paxeer/withdrawal-witnesses/v2\0', withdrawal_items)
    balance_vector = vector(b'LXP/Paxeer/balance-witnesses/v2\0', balance_items)
    require(len(withdrawal_vector) + len(balance_vector) <= 1_000_000, 'publication vector bounds')
    deposit_registration, deposit_signature, ordering, vault = None, None, [], None
    if deposits:
        v = authorization['deposit_registration']
        vault = v['vault']
        require(profile is not None and raw(vault, 20) == profile[13:33], 'deposit vault mismatch')
        reference = raw(v['custody_reference'], 32)
        require(any(reference), 'deposit custody reference empty')
        for fact in deposits:
            leaf = b'LX:PAXEER:DEPOSIT:LEAF:v1' + fact['identity'] + reference + fact['asset'] + fact['amount'] + digest + h[1].to_bytes(4, 'big') + h[0].to_bytes(2, 'big')
            ordering.append(sha(b'LXP/v1/merkle-leaf\0' + leaf))
        level = list(ordering)
        while len(level) > 1:
            level = [sha(b'LXP/v1/merkle-internal\0' + level[i] + level[min(i + 1, len(level) - 1)]) for i in range(0, len(level), 2)]
        deposit_registration = b'LX:PAXEER:DEPOSIT:ROOT:v1' + digest + h[7] + level[0] + reference + h[1].to_bytes(4, 'big') + h[0].to_bytes(2, 'big')
        deposit_signature = raw(v['signature'], 64)
        authority = rpc.view(vault, 'depositRootAuthority()', outputs=('bytes32',))[0]
        signature(authority, deposit_registration, deposit_signature)
    else:
        require(authorization.get('deposit_registration') is None, 'deposit registration without replayed deposits')
    commitment = sha(api.encode(['bytes32', 'uint16', 'bytes', 'bytes'], [digest, 2, withdrawal_vector, balance_vector]))
    published = rpc.view(registry, 'witnessesPublished(bytes32)', ('bytes32',), (digest,), ('bool',))[0]
    witness_tx = None
    if not published:
        witness_tx = transact(api, rpc, request, registry, api.calldata('publishCheckpointWitnesses(bytes32,bytes,bytes)', ('bytes32', 'bytes', 'bytes'), (digest, withdrawal_vector, balance_vector)))
    require(rpc.view(registry, 'witnessesDigest(bytes32)', ('bytes32',), (digest,), ('bytes32',))[0] == commitment, 'published witnesses differ')
    witness_tx = publication_transaction(api, rpc, registry, digest, commitment)
    deposit_tx = None
    if deposits:
        commitment_deposit = sha(api.encode(['uint16', 'bytes', 'bytes', 'bytes32[]'], [2, deposit_registration, deposit_signature, ordering]))
        published_deposit = rpc.view(vault, 'depositRootRegistered(bytes32)', ('bytes32',), (digest,), ('bool',))[0]
        if not published_deposit:
            deposit_tx = transact(api, rpc, request, vault, api.calldata('registerDepositRoot(bytes,bytes,bytes32[])', ('bytes', 'bytes', 'bytes32[]'), (deposit_registration, deposit_signature, ordering)))
        require(rpc.view(vault, 'depositRegistrationDigest(bytes32)', ('bytes32',), (digest,), ('bytes32',))[0] == commitment_deposit, 'published deposit registration differs')
        deposit_tx = publication_transaction(api, rpc, vault, digest, commitment_deposit, level[0])
    evidence = {'version': 2, 'checkpoint_id': hx(digest), 'withdrawal_witnesses': hx(withdrawal_vector), 'balance_witnesses': hx(balance_vector), 'withdrawal_count': len(withdrawals), 'balance_count': len(balances), 'deposit_count': len(deposits), 'witness_transaction': witness_tx, 'deposit_transaction': deposit_tx, 'deposit_registration': None if deposit_registration is None else hx(deposit_registration), 'deposit_signature': None if deposit_signature is None else hx(deposit_signature), 'leaf_ordering': [hx(v) for v in ordering]}
    atomic_json(directory / (digest.hex() + '.evidence.json'), evidence)
    return evidence
