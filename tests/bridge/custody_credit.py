import argparse
import hashlib
import ipaddress
import json
import os
import stat
import urllib.parse
import urllib.request

from Crypto.Hash import keccak
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat


MAX_RESPONSE = 16 * 1024 * 1024
MAX_TRANSACTIONS = 4096
MAX_ANCESTRY = 8192
MAX_TOTAL_RESPONSE = 128 * 1024 * 1024
MAX_RPC_CALLS = 20000
PROFILE_BYTES = 207
DEPOSIT_TOPIC = "0x" + keccak.new(
    digest_bits=256,
    data=b"CustodyDeposit(bytes32,bytes32,address,bytes32,uint256,uint64)",
).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unhex(value, length=None):
    require(isinstance(value, str) and value.startswith("0x"), "hex prefix")
    result = bytes.fromhex(value[2:])
    require(length is None or len(result) == length, "hex length")
    return result


def quantity(value):
    require(isinstance(value, str) and value.startswith("0x"), "quantity prefix")
    require(value == "0x0" or not value.startswith("0x0"), "quantity leading zero")
    return int(value, 16)


def big(value, length):
    return value.to_bytes(length, "big")


def sha(value):
    return hashlib.sha256(value).digest()


def eth_hash(value):
    return keccak.new(digest_bits=256, data=value).digest()


def rlp(value):
    if isinstance(value, int):
        require(value >= 0, "negative RLP integer")
        value = big(value, (value.bit_length() + 7) // 8)
    if isinstance(value, list):
        payload = b"".join(rlp(item) for item in value)
        short, long = 0xc0, 0xf7
    else:
        payload = value
        if len(payload) == 1 and payload[0] < 128:
            return payload
        short, long = 0x80, 0xb7
    if len(payload) < 56:
        return bytes([short + len(payload)]) + payload
    length = big(len(payload), (len(payload).bit_length() + 7) // 8)
    return bytes([long + len(length)]) + length + payload


def compact(path, terminal):
    flag = 2 if terminal else 0
    digits = [flag + 1] + path if len(path) % 2 else [flag, 0] + path
    return bytes(digits[index] * 16 + digits[index + 1] for index in range(0, len(digits), 2))


def trie_root(values):
    def reference(node):
        encoded = rlp(node)
        return node if len(encoded) < 32 else eth_hash(encoded)

    def node(entries):
        if len(entries) == 1:
            path, value = entries[0]
            return [compact(path, True), value]
        shared = 0
        shortest = min(len(path) for path, _ in entries)
        while shared < shortest and all(path[shared] == entries[0][0][shared] for path, _ in entries):
            shared += 1
        if shared:
            return [compact(entries[0][0][:shared], False),
                    reference(node([(path[shared:], value) for path, value in entries]))]
        branches = [b""] * 17
        for digit in range(16):
            matching = [(path[1:], value) for path, value in entries if path and path[0] == digit]
            if matching:
                branches[digit] = reference(node(matching))
        endings = [value for path, value in entries if not path]
        require(len(endings) <= 1, "duplicate trie key")
        if endings:
            branches[16] = endings[0]
        return branches

    if not values:
        return eth_hash(rlp(b""))
    entries = []
    for index, value in enumerate(values):
        path = [digit for byte in rlp(index) for digit in (byte >> 4, byte & 15)]
        entries.append((path, value))
    return eth_hash(rlp(node(entries)))


def decode_rlp(encoded):
    def item(offset, limit, depth):
        require(depth <= 64 and offset < limit, "RLP depth/truncation")
        prefix = encoded[offset]
        offset += 1
        if prefix < 128:
            return bytes([prefix]), offset
        is_list = prefix >= 192
        short = 192 if is_list else 128
        long = 247 if is_list else 183
        if prefix <= long:
            length = prefix - short
        else:
            width = prefix - long
            require(offset + width <= limit and encoded[offset] != 0, "RLP length")
            length = int.from_bytes(encoded[offset:offset + width], "big")
            offset += width
            require(length >= 56, "RLP nonminimal length")
        end = offset + length
        require(end <= limit, "RLP truncated item")
        if not is_list:
            require(length != 1 or encoded[offset] >= 128, "RLP nonminimal byte")
            return encoded[offset:end], end
        values = []
        while offset < end:
            value, offset = item(offset, end, depth + 1)
            values.append(value)
        return values, end

    result, consumed = item(0, len(encoded), 0)
    require(consumed == len(encoded), "RLP trailing bytes")
    return result


def account_from_proof(state_root, address, proof):
    require(isinstance(proof, list) and 0 < len(proof) <= 128, "account proof node bound")
    nodes = {}
    for raw in proof:
        encoded = unhex(raw)
        require(0 < len(encoded) <= 2048, "account proof node byte bound")
        digest = eth_hash(encoded)
        require(digest not in nodes, "duplicate account proof node")
        nodes[digest] = decode_rlp(encoded)
    path = [digit for byte in eth_hash(address) for digit in (byte >> 4, byte & 15)]
    reference = state_root
    for _ in range(129):
        if isinstance(reference, bytes):
            require(len(reference) == 32 and reference in nodes, "missing account proof node")
            current = nodes[reference]
        else:
            current = reference
            require(len(rlp(current)) < 32, "noncanonical inline trie node")
        require(isinstance(current, list), "account trie node")
        if len(current) == 17:
            if not path:
                value = current[16]
                break
            reference, path = current[path[0]], path[1:]
        else:
            require(len(current) == 2 and isinstance(current[0], bytes) and current[0], "account path node")
            digits = [digit for byte in current[0] for digit in (byte >> 4, byte & 15)]
            flag = digits[0]
            require(flag <= 3 and (flag & 1 or digits[1] == 0), "compact trie path")
            segment = digits[1:] if flag & 1 else digits[2:]
            require(path[:len(segment)] == segment, "account proof path mismatch")
            path = path[len(segment):]
            if flag & 2:
                require(not path, "account proof incomplete leaf")
                value = current[1]
                break
            require(segment, "empty trie extension")
            reference = current[1]
    else:
        raise ValueError("account proof work bound")
    require(isinstance(value, bytes) and value, "account non-inclusion")
    account = decode_rlp(value)
    require(isinstance(account, list) and len(account) == 4 and
            all(isinstance(field, bytes) for field in account), "account encoding")
    require(len(account[0]) <= 8 and len(account[1]) <= 32 and
            all(not field or field[0] != 0 for field in account[:2]) and
            len(account[2]) == 32 and len(account[3]) == 32, "account fields")
    return account


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("RPC redirects refused")


class Rpc:
    def __init__(self, url, budget=None):
        parsed = urllib.parse.urlsplit(url)
        require(parsed.scheme in ("http", "https") and parsed.hostname, "RPC URL")
        require(not parsed.username and not parsed.password and not parsed.fragment, "RPC URL authority")
        if parsed.scheme == "http":
            require(ipaddress.ip_address(parsed.hostname).is_loopback, "HTTP requires literal loopback")
        self.url = url
        self.identity = (parsed.scheme, parsed.hostname, parsed.port or (443 if parsed.scheme == "https" else 80))
        self.opener = urllib.request.build_opener(NoRedirect())
        self.budget = budget if budget is not None else {"calls": 0, "bytes": 0}

    def call(self, method, params, allow_missing=False):
        require(self.budget["calls"] < MAX_RPC_CALLS, "aggregate RPC work bound")
        self.budget["calls"] += 1
        remaining = MAX_TOTAL_RESPONSE - self.budget["bytes"]
        require(remaining > 0, "aggregate RPC response bound")
        body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
        request = urllib.request.Request(self.url, body, {"Content-Type": "application/json"})
        with self.opener.open(request, timeout=20) as response:
            raw = response.read(min(MAX_RESPONSE, remaining) + 1)
        self.budget["bytes"] += len(raw)
        require(self.budget["bytes"] <= MAX_TOTAL_RESPONSE, "aggregate RPC response bound")
        require(len(raw) <= MAX_RESPONSE, "RPC response bound")
        result = json.loads(raw)
        require(result.get("jsonrpc") == "2.0" and result.get("id") == 1 and "error" not in result,
                "RPC failure")
        require(allow_missing or result.get("result") is not None, "RPC missing result")
        return result["result"]


def header(block):
    fields = [("parentHash", 32), ("sha3Uncles", 32), ("miner", 20),
              ("stateRoot", 32), ("transactionsRoot", 32), ("receiptsRoot", 32),
              ("logsBloom", 256), ("difficulty", None), ("number", None),
              ("gasLimit", None), ("gasUsed", None), ("timestamp", None),
              ("extraData", -1), ("mixHash", 32), ("nonce", 8)]
    optional = [("baseFeePerGas", None), ("withdrawalsRoot", 32),
                ("blobGasUsed", None), ("excessBlobGas", None),
                ("parentBeaconBlockRoot", 32), ("requestsHash", 32)]
    ended = False
    for field, size in optional:
        if field not in block or block[field] is None:
            ended = True
        else:
            require(not ended, "noncontiguous header fork fields")
            fields.append((field, size))
    encoded = rlp([quantity(block[field]) if size is None else
                   unhex(block[field], None if size == -1 else size) for field, size in fields])
    require(eth_hash(encoded) == unhex(block["hash"], 32), "block header hash")
    return encoded


def agreed_block(rpcs, identifier, by_hash=False):
    method = "eth_getBlockByHash" if by_hash else "eth_getBlockByNumber"
    blocks = [rpc.call(method, [identifier, False]) for rpc in rpcs]
    encoded = [header(block) for block in blocks]
    for block in blocks:
        require(unhex(block["hash"], 32) == unhex(identifier, 32) if by_hash else
                quantity(block["number"]) == quantity(identifier), "requested block binding")
    require(all(value == encoded[0] for value in encoded), "block quorum disagreement")
    return blocks[0]


def common_finalized(rpcs):
    observed = [rpc.call("eth_getBlockByNumber", ["finalized", False]) for rpc in rpcs]
    for block in observed:
        header(block)
    common_height = min(quantity(block["number"]) for block in observed)
    common = agreed_block(rpcs, hex(common_height))
    for rpc, tip in zip(rpcs, observed):
        require(quantity(tip["number"]) - common_height <= MAX_ANCESTRY, "finalized quorum ancestry bound")
        while quantity(tip["number"]) > common_height:
            parent = agreed_block([rpc], tip["parentHash"], True)
            require(unhex(parent["hash"], 32) == unhex(tip["parentHash"], 32) and
                    quantity(parent["number"]) + 1 == quantity(tip["number"]), "finalized parent binding")
            tip = parent
        require(unhex(tip["hash"], 32) == unhex(common["hash"], 32), "finalized quorum ancestry disagreement")
    return common


def verified_code(rpcs, address, block):
    selector = {"blockHash": block["hash"], "requireCanonical": True}
    codes = []
    for rpc in rpcs:
        proof = rpc.call("eth_getProof", [address, [], selector])
        require(unhex(proof["address"], 20) == unhex(address, 20), "proof address binding")
        account = account_from_proof(unhex(block["stateRoot"], 32), unhex(address, 20), proof["accountProof"])
        require(account[3] == unhex(proof["codeHash"], 32) and
                account[2] == unhex(proof["storageHash"], 32) and
                int.from_bytes(account[0], "big") == quantity(proof["nonce"]) and
                int.from_bytes(account[1], "big") == quantity(proof["balance"]), "account proof fields")
        code = unhex(rpc.call("eth_getCode", [address, selector]))
        require(code and eth_hash(code) == account[3], "runtime account code hash")
        codes.append(code)
    require(all(code == codes[0] for code in codes), "runtime code quorum disagreement")
    return codes[0]


def receipt_bytes(receipt):
    status = quantity(receipt["status"])
    require(status in (0, 1), "receipt status")
    kind = quantity(receipt.get("type", "0x0"))
    require(0 <= kind <= 4, "unsupported receipt type")
    logs = []
    for log in receipt["logs"]:
        require(not log.get("removed", False), "removed log")
        logs.append([unhex(log["address"], 20), [unhex(topic, 32) for topic in log["topics"]],
                     unhex(log["data"])])
    payload = rlp([status, quantity(receipt["cumulativeGasUsed"]), unhex(receipt["logsBloom"], 256), logs])
    return (bytes([kind]) if kind else b"") + payload


def verified_receipts(rpcs, block):
    transactions = block["transactions"]
    require(0 < len(transactions) <= MAX_TRANSACTIONS, "transaction bound")
    groups = [rpc.call("eth_getBlockReceipts", [block["hash"]]) for rpc in rpcs]
    encodings = []
    for group in groups:
        require(len(group) == len(transactions), "receipt count")
        for index, receipt in enumerate(group):
            require(quantity(receipt["transactionIndex"]) == index and
                    unhex(receipt["blockHash"], 32) == unhex(block["hash"], 32) and
                    quantity(receipt["blockNumber"]) == quantity(block["number"]) and
                    unhex(receipt["transactionHash"], 32) == unhex(transactions[index], 32), "receipt binding")
        encodings.append([receipt_bytes(receipt) for receipt in group])
    require(all(values == encodings[0] for values in encodings), "receipt quorum disagreement")
    require(trie_root(encodings[0]) == unhex(block["receiptsRoot"], 32), "receipt inclusion root")
    raw_transactions = []
    total_bytes = 0
    for transaction in transactions:
        raw = [unhex(rpc.call("eth_getRawTransactionByHash", [transaction])) for rpc in rpcs]
        require(all(value == raw[0] for value in raw) and eth_hash(raw[0]) == unhex(transaction, 32),
                "transaction hash/quorum")
        total_bytes += len(raw[0])
        require(total_bytes <= MAX_RESPONSE, "aggregate transaction byte bound")
        raw_transactions.append(raw[0])
    require(trie_root(raw_transactions) == unhex(block["transactionsRoot"], 32), "transaction inclusion root")
    return groups[0]


def read_key(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        info = os.fstat(descriptor)
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_mode & 0o077 == 0,
                "key must be a private regular file")
        data = os.read(descriptor, 33)
        require(len(data) == 32, "key seed length")
        return Ed25519PrivateKey.from_private_bytes(data)
    finally:
        os.close(descriptor)


def write_new(path, data):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "wb") as output:
        output.write(data)
        output.flush()
        os.fsync(output.fileno())


def rpc_pair(urls):
    require(len(urls) == 2, "exactly two distinct RPC origins required; backend independence is operator-verified")
    budget = {"calls": 0, "bytes": 0}
    rpcs = [Rpc(url, budget) for url in urls]
    require(rpcs[0].identity != rpcs[1].identity, "distinct endpoint origins required")
    return rpcs


def create_profile(args):
    rpcs = rpc_pair(args.rpc)
    chain = [quantity(rpc.call("eth_chainId", [])) for rpc in rpcs]
    require(chain == [args.chain_id, args.chain_id] and args.chain_id > 0, "chain identity")
    genesis = agreed_block(rpcs, "0x0")
    tip = common_finalized(rpcs)
    code = verified_code(rpcs, args.vault, tip)
    require(sha(code) == unhex(args.runtime_sha256, 32), "vault runtime pin")
    public = read_key(args.attestor_key).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    name = b"system:paxeer-reserve"
    reserve = sha(b"LX:ACCOUNT:v1" + big(len(name), 4) + name)
    require(args.confirmations > 0 and args.network_id > 0, "confirmation/network bound")
    profile = (b"LXBC1" + big(args.chain_id, 8) + unhex(args.vault, 20) + sha(code) + public +
               unhex(args.asset, 32) + reserve + big(args.confirmations, 8) + unhex(genesis["hash"], 32) +
               big(args.network_id, 4) + big(3, 2))
    require(len(profile) == PROFILE_BYTES, "profile layout")
    write_new(args.output, profile)


def attest(args):
    rpcs = rpc_pair(args.rpc)
    with open(args.profile, "rb") as source:
        profile = source.read(PROFILE_BYTES + 1)
    require(len(profile) == PROFILE_BYTES and profile[:5] == b"LXBC1", "profile layout")
    chain = int.from_bytes(profile[5:13], "big")
    require(all(quantity(rpc.call("eth_chainId", [])) == chain for rpc in rpcs), "chain identity")
    genesis = agreed_block(rpcs, "0x0")
    require(unhex(genesis["hash"], 32) == profile[169:201], "chain genesis identity")
    require(args.network_id > 0 and profile[201:207] == big(args.network_id, 4) + big(3, 2), "network/protocol binding")
    transaction = "0x" + unhex(args.transaction, 32).hex()
    observations = [rpc.call("eth_getTransactionReceipt", [transaction]) for rpc in rpcs]
    require(all(unhex(value["transactionHash"], 32) == unhex(transaction, 32) for value in observations),
            "transaction receipt binding")
    require(all(value["blockHash"] == observations[0]["blockHash"] for value in observations), "inclusion quorum")
    block = agreed_block(rpcs, observations[0]["blockHash"], True)
    final = common_finalized(rpcs)
    height, final_height = quantity(block["number"]), quantity(final["number"])
    confirmations = int.from_bytes(profile[161:169], "big")
    require(confirmations > 0 and height > 0 and final_height >= height and
            confirmations <= final_height - height + 1 <= MAX_ANCESTRY, "finality/ancestry bound")
    ancestor = final
    while quantity(ancestor["number"]) > height:
        parent = agreed_block(rpcs, ancestor["parentHash"], True)
        require(unhex(parent["hash"], 32) == unhex(ancestor["parentHash"], 32) and
                quantity(parent["number"]) + 1 == quantity(ancestor["number"]), "ancestry parent binding")
        ancestor = parent
    require(unhex(ancestor["hash"], 32) == unhex(block["hash"], 32), "finalized ancestry")
    vault = "0x" + profile[13:33].hex()
    for point in (block, final):
        code = verified_code(rpcs, vault, point)
        require(sha(code) == profile[33:65], "vault runtime hash")
    receipts = verified_receipts(rpcs, block)
    index = quantity(observations[0]["transactionIndex"])
    require(index < len(receipts) and receipts[index]["transactionHash"].lower() == transaction and
            quantity(receipts[index]["status"]) == 1, "successful transaction required")
    matches = [(index, log) for index, log in enumerate(receipts[index]["logs"])
               if unhex(log["address"], 20) == profile[13:33] and log["topics"] and
               log["topics"][0].lower() == DEPOSIT_TOPIC]
    require(len(matches) == 1, "exactly one custody deposit required")
    log_index, log = matches[0]
    require(len(log["topics"]) == 4, "deposit topics")
    deposit_id, asset, payer_word = [unhex(value, 32) for value in log["topics"][1:]]
    data = unhex(log["data"], 96)
    beneficiary, amount_word, nonce_word = data[:32], data[32:64], data[64:]
    amount, nonce = int.from_bytes(amount_word, "big"), int.from_bytes(nonce_word, "big")
    require(asset == profile[97:129] and payer_word[:12] == bytes(12) and
            0 < amount < 2 ** 128 and 0 < nonce < 2 ** 64, "custody fields")
    require(0 < args.expected_amount < 2 ** 128 and amount == args.expected_amount, "expected amount binding")
    require(beneficiary == unhex(args.beneficiary, 32), "beneficiary binding")
    owner = unhex(args.beneficiary_key, 32)
    deposit_domain = b"LXP/Paxeer/custody-deposit/v1"
    preimage = (big(256, 32) + big(chain, 32) + bytes(12) + profile[13:33] + payer_word + asset +
                beneficiary + amount_word + nonce_word + big(len(deposit_domain), 32) +
                deposit_domain.ljust(32, b"\0"))
    require(sha(preimage) == deposit_id, "deposit ID preimage")
    unsigned = (b"LXDC1" + sha(profile) + big(args.network_id, 4) + big(3, 2) + deposit_id + asset +
                beneficiary + owner + payer_word[12:] + big(amount, 16) + big(nonce, 8) +
                big(height, 8) + unhex(block["hash"], 32) + unhex(block["receiptsRoot"], 32) +
                big(final_height, 8) + unhex(final["hash"], 32) + unhex(transaction, 32) + big(log_index, 4))
    require(len(unsigned) == 363, "credit layout")
    key = read_key(args.attestor_key)
    require(key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw) == profile[65:97], "attestor authority")
    payload = unsigned + key.sign(b"LX:CUSTODY:CREDIT:v1" + unsigned)
    nullifier = sha(b"LX:DEPOSIT:NULLIFIER:v1" + deposit_id)
    write_new(args.output, payload)
    write_new(args.output + ".nullifier", nullifier.hex().encode() + b"\n")


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    profile = commands.add_parser("profile")
    profile.add_argument("--chain-id", type=int, required=True)
    profile.add_argument("--network-id", type=int, required=True)
    profile.add_argument("--vault", required=True)
    profile.add_argument("--runtime-sha256", required=True)
    profile.add_argument("--asset", required=True)
    profile.add_argument("--confirmations", type=int, required=True)
    credit = commands.add_parser("attest")
    credit.add_argument("--profile", required=True)
    credit.add_argument("--network-id", type=int, required=True)
    credit.add_argument("--transaction", required=True)
    credit.add_argument("--beneficiary", required=True)
    credit.add_argument("--beneficiary-key", required=True)
    credit.add_argument("--expected-amount", type=int, required=True)
    for command in (profile, credit):
        command.add_argument("--rpc", action="append", required=True)
        command.add_argument("--attestor-key", required=True)
        command.add_argument("--output", required=True)
    args = parser.parse_args()
    (create_profile if args.command == "profile" else attest)(args)


if __name__ == "__main__":
    main()
