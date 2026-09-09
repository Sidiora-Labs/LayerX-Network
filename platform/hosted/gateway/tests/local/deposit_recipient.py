import json
import sys
from pathlib import Path
from urllib.parse import urlsplit

sys.path.insert(0, str(Path(__file__).resolve().parents[5] / "tests/bridge"))
from deploy_local_custody import Rpc, calldata, quantity, require, send, write_new

url, deployment_path, beneficiary, output = sys.argv[1:]
parsed = urlsplit(url)
require(parsed.hostname == "127.0.0.1" and parsed.port != 18545, "disposable RPC required")
rpc = Rpc(url)
require(quantity(rpc.call("eth_chainId", [])) == 31337, "disposable chain required")
require("anvil" in rpc.call("web3_clientVersion", []).lower(), "Anvil required")
deployment = json.loads(Path(deployment_path).read_text())
account = rpc.call("eth_accounts", [])[0]
send(rpc, account, deployment["token"], calldata("deposit()"), 1)
send(rpc, account, deployment["token"], calldata("approve(address,uint256)", deployment["vault"], 1))
receipt = send(rpc, account, deployment["vault"], calldata("deposit(bytes32,uint256,bytes32)", deployment["asset"], 1, beneficiary))
rpc.call("anvil_mine", ["0x80"], allow_missing=True)
write_new(output, json.dumps({"transaction": receipt["transactionHash"], "fork_block": quantity(rpc.call("eth_blockNumber", []))}).encode())
