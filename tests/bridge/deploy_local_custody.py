import argparse
import hashlib
import json
import os
import selectors
import signal
import subprocess
import time
from pathlib import Path
import urllib.parse

from custody_credit import Rpc, quantity, require, unhex, write_new


ROOT = Path(__file__).resolve().parents[2]


def command(*args):
    with subprocess.Popen(args, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          start_new_session=True) as process:
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
    transaction = rpc.call("eth_sendTransaction", [{"from": account, "to": target,
                            "data": data, "value": hex(value), "gas": hex(6000000)}])
    return receipt(rpc, transaction)


def deploy(rpc, account, contract, *args):
    invocation = ["forge", "create", contract, "--broadcast", "--unlocked", "--from", account,
                  "--rpc-url", rpc.url, "--json"]
    if args:
        invocation += ["--constructor-args", *map(str, args)]
    result = json.loads(command(*invocation))
    address = result["deployedTo"]
    require(len(unhex(address, 20)) == 20, "deployment address")
    deployed = receipt(rpc, result["transactionHash"])
    require(unhex(deployed["contractAddress"], 20) == unhex(address, 20),
            "deployment receipt contract identity")
    return address


def govern(rpc, account, timelock, target, data):
    nonce = int(rpc.call("eth_call", [{"to": timelock, "data": calldata("operationNonce()")}, "latest"]), 16)
    salt = "0x" + hashlib.sha256((target + data + str(nonce)).encode()).hexdigest()
    send(rpc, account, timelock, calldata("schedule(address,uint256,bytes,bytes32,uint64)",
                                       target, 0, data, salt, 86400))
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
    args = parser.parse_args()
    rpc = Rpc(args.rpc)
    parsed = urllib.parse.urlsplit(args.rpc)
    require(parsed.hostname in ("127.0.0.1", "::1") and parsed.port != 18545, "isolated loopback chain only")
    require("anvil" in rpc.call("web3_clientVersion", []).lower(), "Anvil required")
    require(quantity(rpc.call("eth_chainId", [])) != 125, "persistent chain ID refused")
    require(0 < args.amount < 2 ** 128, "amount bound")
    unhex(args.asset, 32)
    unhex(args.beneficiary, 32)
    account = rpc.call("eth_accounts", [])[0]
    config = "0x" + hashlib.sha256(b"LayerX/local-custody/real-weth/v1").hexdigest()
    timelock = deploy(rpc, account, "contracts/governance/LayerXTimelock.sol:LayerXTimelock",
                      86400, 172800, account, account, account, 0, config, 1)
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
    write_new(args.output, json.dumps(result, indent=2).encode() + b"\n")


if __name__ == "__main__":
    main()
