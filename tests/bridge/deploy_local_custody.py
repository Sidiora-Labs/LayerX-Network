import argparse
import base64
import hashlib
import json
import os
import selectors
import signal
import ssl
import stat
import subprocess
import time
from pathlib import Path
import urllib.parse
import urllib.request

from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from custody_credit import NoRedirect, Rpc, eth_hash, quantity, require, unhex, write_new


ROOT = Path(__file__).resolve().parents[2]
PERSISTENT_CHAIN_ID = "hyperpax_125-1"
PERSISTENT_GENESIS = bytes.fromhex("07fec8dbcbfdb79b45b9b9a33f5845f8502816b2f9eeebebe2708733c5180b88")
PERSISTENT_BLUEPRINT = "0x64a8d4b74faf4e9a2d18fb5e280623f632a02939"
MAX_GENESIS_BYTES = 64 * 1024 * 1024


def genesis_document(rpc, source):
    def fetch(path, limit):
        with rpc.opener.open(rpc.url.rstrip("/") + path, timeout=20) as response:
            raw = response.read(limit + 1)
        require(len(raw) <= limit, "genesis response bound")
        return raw

    if source == "published":
        return fetch("/genesis.json", MAX_GENESIS_BYTES)
    require(source == "chunked", "explicit genesis source required")
    document = bytearray()
    total = None
    for index in range(64):
        envelope = json.loads(fetch("/genesis_chunked?chunk=" + str(index), 24 * 1024 * 1024))
        if "jsonrpc" in envelope:
            require(envelope["jsonrpc"] == "2.0" and "error" not in envelope, "genesis RPC failure")
            result = envelope["result"]
        else:
            result = envelope
        require(set(result) == {"total", "chunk", "data"}, "genesis chunk response")
        count = int(result["total"])
        require(0 < count <= 64 and int(result["chunk"]) == index
                and (total is None or count == total), "genesis chunk sequence")
        total = count
        document.extend(base64.b64decode(result["data"], validate=True))
        require(len(document) <= MAX_GENESIS_BYTES, "genesis document bound")
        if index + 1 == total:
            return bytes(document)
    raise ValueError("genesis chunk bound")


def command(*args, env=None):
    with subprocess.Popen(args, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          start_new_session=True, env=env) as process:
        output = bytearray()
        total = 0
        deadline = time.monotonic() + 120
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(process.stdout, selectors.EVENT_READ)
                selector.register(process.stderr, selectors.EVENT_READ)
                while selector.get_map():
                    require(time.monotonic() < deadline, "command timeout")
                    for ready, _ in selector.select(min(1, max(0, deadline - time.monotonic()))):
                        chunk = os.read(ready.fileobj.fileno(), 65536)
                        if not chunk:
                            selector.unregister(ready.fileobj)
                            continue
                        total += len(chunk)
                        require(total <= 8 * 1024 * 1024, "command output limit")
                        if ready.fileobj is process.stdout:
                            output.extend(chunk)
                require(process.wait(timeout=max(0.001, deadline - time.monotonic())) == 0,
                        "command failed")
        except BaseException:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
            raise
        return output.decode("utf-8").strip()


def origin(url):
    parsed = urllib.parse.urlsplit(url)
    require(parsed.scheme == "https" and parsed.hostname and not parsed.username
            and not parsed.password and not parsed.fragment and not parsed.query
            and parsed.path in ("", "/"), "disposable RPC must be an HTTPS origin")
    require(parsed.port not in (18545, 19443), "persistent host endpoint refused")
    return (parsed.scheme, parsed.hostname, parsed.port or 443)


def disposable_rpc(url, ca_bundle, identity_file):
    identity = json.loads(Path(identity_file).read_text())
    selected = origin(url)
    require(selected in [origin(value) for value in identity["rpc_origins"]],
            "RPC origin not authorized by disposable identity")
    genesis = unhex(identity["genesis_sha256"], 32)
    comet_chain = identity["comet_chain_id"]
    require(isinstance(comet_chain, str) and comet_chain and comet_chain != PERSISTENT_CHAIN_ID,
            "persistent Comet chain ID refused")
    require(genesis != bytes(32) and genesis != PERSISTENT_GENESIS, "persistent chain genesis refused")
    require(type(identity["chain_id"]) is int and identity["chain_id"] > 0, "EVM chain ID required")
    require(hashlib.sha256(Path(ca_bundle).read_bytes()).digest()
            == unhex(identity["ca_sha256"], 32), "disposable CA pin")
    context = ssl.create_default_context(cafile=ca_bundle)
    rpc = Rpc(url)
    rpc.opener = urllib.request.build_opener(NoRedirect(), urllib.request.HTTPSHandler(context=context))
    document = genesis_document(rpc, identity["genesis_source"])
    observed = hashlib.sha256(document).digest()
    observed_chain = json.loads(document)["chain_id"]
    require(observed_chain != PERSISTENT_CHAIN_ID, "persistent Comet chain ID refused")
    require(observed != PERSISTENT_GENESIS, "persistent chain genesis refused")
    require(observed == genesis and observed_chain == comet_chain, "disposable genesis identity")
    require(quantity(rpc.call("eth_chainId", [])) == identity["chain_id"], "disposable chain ID")
    require(not unhex(rpc.call("eth_getCode", [PERSISTENT_BLUEPRINT, "latest"])),
            "persistent blueprint code refused")
    rpc.genesis_sha256 = observed
    rpc.comet_chain_id = observed_chain
    rpc.disposable = True
    rpc.command_env = {**os.environ, "SSL_CERT_FILE": str(Path(ca_bundle).resolve())}
    return rpc


def signer(rpc, path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        info = os.fstat(descriptor)
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_mode & 0o077 == 0
                and 66 <= info.st_size <= 68,
                "signer key must be a private regular file")
        raw = os.read(descriptor, 69).strip()
        require(len(raw) == 66, "signer key length")
        key = unhex(raw.decode("ascii"), 32)
    finally:
        os.close(descriptor)
    private = ec.derive_private_key(int.from_bytes(key, "big"), ec.SECP256K1())
    public = private.public_key().public_bytes(Encoding.X962, PublicFormat.UncompressedPoint)
    rpc.signing_key = "0x" + key.hex()
    return "0x" + eth_hash(public[1:])[-20:].hex()


def calldata(signature, *args):
    return command("cast", "calldata", signature, *map(str, args))


def receipt(rpc, transaction):
    for _ in range(120):
        result = rpc.call("eth_getTransactionReceipt", [transaction], allow_missing=True)
        if result:
            require(unhex(result["transactionHash"], 32) == unhex(transaction, 32),
                    "transaction receipt identity")
            require(quantity(result["status"]) == 1, "local transaction reverted")
            return result
        time.sleep(0.25)
    raise ValueError("local transaction receipt timeout")


def send(rpc, account, target, data, value=0):
    if getattr(rpc, "signing_key", None):
        transaction = command("cast", "send", "--rpc-url", rpc.url, "--private-key", rpc.signing_key,
                              "--async", "--gas-limit", "6000000", "--value", str(value),
                              target, "--data", data, env=getattr(rpc, "command_env", None))
        return receipt(rpc, transaction)
    transaction = rpc.call("eth_sendTransaction", [{"from": account, "to": target,
                            "data": data, "value": hex(value), "gas": hex(6000000)}])
    return receipt(rpc, transaction)


def deploy(rpc, account, contract, *args):
    wallet = (["--private-key", rpc.signing_key] if getattr(rpc, "signing_key", None)
              else ["--unlocked", "--from", account])
    invocation = ["forge", "create", contract, "--broadcast", *wallet,
                  "--rpc-url", rpc.url, "--json"]
    if args:
        invocation += ["--constructor-args", *map(str, args)]
    result = json.loads(command(*invocation, env=getattr(rpc, "command_env", None)))
    address = result["deployedTo"]
    require(len(unhex(address, 20)) == 20, "deployment address")
    deployed = receipt(rpc, result["transactionHash"])
    require(unhex(deployed["contractAddress"], 20) == unhex(address, 20),
            "deployment receipt contract identity")
    return address


def govern(rpc, account, timelock, target, data):
    nonce = int(rpc.call("eth_call", [{"to": timelock, "data": calldata("operationNonce()")}, "latest"]), 16)
    salt = "0x" + hashlib.sha256((target + data + str(nonce)).encode()).hexdigest()
    delay = 0 if getattr(rpc, "disposable", False) else 86400
    send(rpc, account, timelock, calldata("schedule(address,uint256,bytes,bytes32,uint64)",
                                       target, 0, data, salt, delay))
    if not getattr(rpc, "disposable", False):
        rpc.call("evm_increaseTime", [86401])
        rpc.call("evm_mine", [], allow_missing=True)
    send(rpc, account, timelock, calldata("execute(address,uint256,bytes,bytes32,uint256)",
                                       target, 0, data, salt, nonce))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--rpc", required=True)
    parser.add_argument("--asset", required=True)
    parser.add_argument("--beneficiary", required=True)
    parser.add_argument("--amount", type=int, required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--allow-local-chain", action="store_true", required=True)
    parser.add_argument("--ca-bundle")
    parser.add_argument("--disposable-identity")
    parser.add_argument("--key-file")
    args = parser.parse_args()
    if args.disposable_identity:
        require(args.ca_bundle and args.key_file, "disposable deployment requires CA and signer")
        rpc = disposable_rpc(args.rpc, args.ca_bundle, args.disposable_identity)
        require(quantity(rpc.call("eth_chainId", [])) == 125, "beta timelock requires chain 125")
        account = signer(rpc, args.key_file)
    else:
        require(not args.ca_bundle, "CA requires explicit disposable identity")
        parsed = urllib.parse.urlsplit(args.rpc)
        require(parsed.hostname in ("127.0.0.1", "::1") and parsed.port not in (18545, 19443),
                "isolated loopback chain only")
        rpc = Rpc(args.rpc)
        require("anvil" in rpc.call("web3_clientVersion", []).lower(), "Anvil required")
        require(quantity(rpc.call("eth_chainId", [])) != 125, "persistent chain ID refused")
        account = signer(rpc, args.key_file) if args.key_file else rpc.call("eth_accounts", [])[0]
    require(0 < args.amount < 2 ** 128, "amount bound")
    unhex(args.asset, 32)
    unhex(args.beneficiary, 32)
    config = "0x" + hashlib.sha256(b"LayerX/local-custody/real-weth/v1").hexdigest()
    beta = getattr(rpc, "disposable", False)
    contract = ("contracts/governance/LayerXBetaTimelock.sol:LayerXBetaTimelock" if beta
                else "contracts/governance/LayerXTimelock.sol:LayerXTimelock")
    timelock = deploy(rpc, account, contract,
                      0 if beta else 86400, 172800, account, account, account, 0, config, 1)
    registry = deploy(rpc, account, "contracts/custody/AssetRegistry.sol:AssetRegistry",
                      timelock, account, config, 1)
    token = deploy(rpc, account,
                   "paxeer-network/loadtest/contracts/evm/lib/solmate/src/tokens/WETH.sol:WETH")
    vault = deploy(rpc, account, "contracts/custody/LayerXVault.sol:LayerXVault",
                   registry, timelock, account, config, 1)
    register = calldata("registerAsset(bytes32,address,uint8,uint128,uint128)",
                        args.asset, token, 18, 1, 2 ** 128 - 1)
    permission = calldata("setCallPermission(address,bytes4,bool)", registry, register[:10], "true")
    govern(rpc, account, timelock, timelock, permission)
    govern(rpc, account, timelock, registry, register)
    send(rpc, account, token, calldata("deposit()"), args.amount)
    send(rpc, account, token, calldata("approve(address,uint256)", vault, args.amount))
    deposited = send(rpc, account, vault, calldata("deposit(bytes32,uint256,bytes32)",
                                                 args.asset, args.amount, args.beneficiary))
    if not beta:
        rpc.call("anvil_mine", ["0x80"], allow_missing=True)
    wrapped_balance = int(rpc.call("eth_call", [{"to": token,
                           "data": calldata("balanceOf(address)", vault)}, "latest"]), 16)
    native_balance = quantity(rpc.call("eth_getBalance", [token, "latest"]))
    require(wrapped_balance == args.amount and native_balance == args.amount, "real native custody backing")
    code = unhex(rpc.call("eth_getCode", [vault, "latest"]))
    result = {"chain_id": quantity(rpc.call("eth_chainId", [])), "vault": vault,
              "registry": registry, "timelock": timelock, "token": token,
              "asset": args.asset, "amount": str(args.amount), "beneficiary": args.beneficiary,
              "transaction": deposited["transactionHash"], "runtime_sha256": "0x" + hashlib.sha256(code).hexdigest(),
              "fork_block": quantity(rpc.call("eth_blockNumber", []))}
    if beta:
        result.update(genesis_sha256="0x" + rpc.genesis_sha256.hex(), comet_chain_id=rpc.comet_chain_id)
    write_new(args.output, json.dumps(result, indent=2).encode() + b"\n")


if __name__ == "__main__":
    main()
