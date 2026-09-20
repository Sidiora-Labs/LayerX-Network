import argparse
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import sys
import urllib.parse
import urllib.request

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "platform/hosted/paxeer"))
from evm import keccak


MAX_RESPONSE = 16 * 1024 * 1024
MAX_TRANSACTIONS = 4096
MAX_ANCESTRY = 8192
MAX_TOTAL_RESPONSE = 128 * 1024 * 1024
MAX_RPC_CALLS = 20000
PROFILE_BYTES = 223
DEPOSIT_TOPIC = "0x" + keccak(b"CustodyDeposit(bytes32,bytes32,address,bytes32,uint256,uint64)").hex()


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
    return keccak(value)


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


def identity_rpcs(args):
    if getattr(args, "disposable_identity", None):
        from deploy_local_custody import disposable_rpc
        require(getattr(args, "ca_bundle", None), "disposable identity requires CA")
        require(len(args.rpc) == 2, "exactly two distinct RPC origins required")
        rpcs = [disposable_rpc(url, args.ca_bundle, args.disposable_identity) for url in args.rpc]
        require(rpcs[0].identity != rpcs[1].identity, "distinct endpoint origins required")
        require(rpcs[0].genesis_sha256 == rpcs[1].genesis_sha256
                and rpcs[0].comet_chain_id == rpcs[1].comet_chain_id, "Comet genesis quorum")
        budget = {"calls": 0, "bytes": 0}
        for rpc in rpcs:
            rpc.budget = budget
        return rpcs, rpcs[0].genesis_sha256
    require(not getattr(args, "ca_bundle", None), "CA requires disposable identity")
    require(all(urllib.parse.urlsplit(url).port not in (18545, 19443) for url in args.rpc),
            "persistent host endpoint refused")
    rpcs = rpc_pair(args.rpc)
    require(all(quantity(rpc.call("eth_chainId", [])) != 125 for rpc in rpcs),
            "chain 125 requires verified disposable identity")
    require(all("anvil" in rpc.call("web3_clientVersion", []).lower() for rpc in rpcs),
            "non-Anvil chains require verified disposable identity")
    return rpcs, unhex(agreed_block(rpcs, "0x0")["hash"], 32)


def create_profile(args):
    rpcs, _ = identity_rpcs(args)
    chain = [quantity(rpc.call("eth_chainId", [])) for rpc in rpcs]
    require(chain == [args.chain_id, args.chain_id] and args.chain_id > 0, "chain identity")
    require(args.chain_id == 125, "custody profiles are issued only for the native Paxeer custody module")
    from comet_credit import create_profile as create_comet_profile
    return create_comet_profile(args, rpcs)


def attest(args):
    rpcs, _ = identity_rpcs(args)
    with open(args.profile, "rb") as source:
        profile = source.read(PROFILE_BYTES + 1)
    require(profile[:5] not in (b"LXBC1", b"LXBC2"),
            "attestor-signed custody profiles are retired; an LXBC3 light-client profile is required")
    require(len(profile) == PROFILE_BYTES and profile[:5] == b"LXBC3", "profile layout")
    from comet_credit import attest as attest_comet
    return attest_comet(args, rpcs, profile)


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest="command", required=True)
    profile = commands.add_parser("profile")
    profile.add_argument("--chain-id", type=int, required=True)
    profile.add_argument("--network-id", type=int, required=True)
    profile.add_argument("--vault", required=True)
    profile.add_argument("--runtime-sha256", required=True)
    profile.add_argument("--asset", required=True)
    profile.add_argument("--trusted-height", type=int, required=True)
    profile.add_argument("--trusting-period-seconds", type=int, required=True)
    credit = commands.add_parser("attest")
    credit.add_argument("--profile", required=True)
    credit.add_argument("--network-id", type=int, required=True)
    credit.add_argument("--transaction", required=True)
    credit.add_argument("--beneficiary", required=True)
    credit.add_argument("--beneficiary-key", required=True)
    credit.add_argument("--expected-amount", type=int, required=True)
    for command in (profile, credit):
        command.add_argument("--vault-artifact")
        command.add_argument("--ca-bundle")
        command.add_argument("--disposable-identity")
        command.add_argument("--rpc", action="append", required=True)
        command.add_argument("--comet-rpc", required=True)
        command.add_argument("--output", required=True)
    args = parser.parse_args()
    (create_profile if args.command == "profile" else attest)(args)


if __name__ == "__main__":
    main()
