import argparse
import hashlib
import os
from pathlib import Path
import sys
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tests/bridge'))
from custody_credit import create_profile, attest, identity_rpcs, quantity, unhex, write_new
from deploy_local_custody import calldata, deploy, govern, send, signer
from provision import protected_json, require, write_json, h32


def bootstrap(args):
    root = Path(args.work_dir) / 'human-evidence-input'
    root.mkdir(mode=0o700, exist_ok=True)
    h32(args.asset, root, 'native asset')
    require(args.network_id > 0, root, 'network id')
    rpcs, _ = identity_rpcs(args)
    rpc = rpcs[0]
    account = signer(rpc, args.key_file)
    write_new(root / 'custody-bootstrap.started', b'preserve all artifacts; reconcile before retry\n')
    config = '0x' + hashlib.sha256(b'LayerX/local-custody/real-weth/v1').hexdigest()
    beta = getattr(rpc, 'disposable', False)
    timelock = deploy(rpc, account, 'contracts/governance/' +
                      ('LayerXBetaTimelock.sol:LayerXBetaTimelock' if beta else 'LayerXTimelock.sol:LayerXTimelock'),
                      0 if beta else 86400, 172800, account, account, account, 0, config, 1)
    registry = deploy(rpc, account, 'contracts/custody/AssetRegistry.sol:AssetRegistry', timelock, account, config, 1)
    token = deploy(rpc, account, 'paxeer-network/loadtest/contracts/evm/lib/solmate/src/tokens/WETH.sol:WETH')
    vault = deploy(rpc, account, 'contracts/custody/LayerXVault.sol:LayerXVault', registry, timelock, account, config, 1)
    register = calldata('registerAsset(bytes32,address,uint8,uint128,uint128)', '0x' + args.asset, token, 18, 1, 2 ** 128 - 1)
    govern(rpc, account, timelock, timelock, calldata('setCallPermission(address,bytes4,bool)', registry, register[:10], 'true'))
    govern(rpc, account, timelock, registry, register)
    runtime = hashlib.sha256(unhex(rpc.call('eth_getCode', [vault, 'latest']))).hexdigest()
    write_new(args.attestor_key, os.urandom(32))
    if not beta:
        rpc.call('anvil_mine', ['0x80'], allow_missing=True)
    create_profile(SimpleNamespace(**{**vars(args), 'chain_id': quantity(rpc.call('eth_chainId', [])),
                   'vault': vault, 'runtime_sha256': '0x' + runtime, 'asset': '0x' + args.asset, 'confirmations': 1,
                   'output': str(root / 'custody.profile')}))
    write_json(root / 'owner-custody.json', dict(vault=vault, token=token, registry=registry,
               timelock=timelock, asset=args.asset, runtime_sha256=runtime, payer=account))


def deposit(args):
    root = Path(args.work_dir) / 'human-evidence-input'
    owner = protected_json(root / 'owner-admission.json')
    config = protected_json(root / 'owner-custody.json')
    for name in ('owner_account', 'public_key'):
        h32(owner[name], root, name)
    require(0 < args.amount < 2 ** 128, root, 'deposit amount')
    rpcs, _ = identity_rpcs(args)
    rpc = rpcs[0]
    account = signer(rpc, args.key_file)
    require(account == config['payer'] and args.asset == config['asset'], root, 'custody payer and asset')
    profile = (root / 'custody.profile').read_bytes()
    require(len(profile) == 207 and profile[:5] in (b'LXBC1', b'LXBC2') and profile[13:33] == unhex(config['vault'], 20)
            and profile[97:129] == bytes.fromhex(args.asset) and profile[201:207] == args.network_id.to_bytes(4, 'big') + b'\0\3',
            root, 'immutable custody profile binding')
    for endpoint in rpcs:
        require(hashlib.sha256(unhex(endpoint.call('eth_getCode', [config['vault'], 'latest']))).digest() == profile[33:65],
                root, 'vault runtime pin')
    write_new(root / 'custody-deposit.started', b'preserve all artifacts; never blindly repeat this deposit\n')
    send(rpc, account, config['token'], calldata('deposit()'), args.amount)
    send(rpc, account, config['token'], calldata('approve(address,uint256)', config['vault'], args.amount))
    result = send(rpc, account, config['vault'], calldata('deposit(bytes32,uint256,bytes32)',
                  '0x' + args.asset, args.amount, '0x' + owner['owner_account']))
    write_json(root / 'custody-deposit.json', result)
    if not getattr(rpc, 'disposable', False):
        rpc.call('anvil_mine', ['0x80'], allow_missing=True)
    attest(SimpleNamespace(**{**vars(args), 'profile': str(root / 'custody.profile'),
        'transaction': result['transactionHash'], 'beneficiary': '0x' + owner['owner_account'],
        'beneficiary_key': '0x' + owner['public_key'], 'expected_amount': args.amount,
        'output': str(root / 'custody-credit.bin')}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('mode', choices=['bootstrap', 'deposit'])
    parser.add_argument('--work-dir', required=True)
    parser.add_argument('--rpc', action='append', required=True)
    parser.add_argument('--ca-bundle')
    parser.add_argument('--disposable-identity')
    parser.add_argument('--vault-artifact')
    parser.add_argument('--key-file', required=True)
    parser.add_argument('--attestor-key', required=True)
    parser.add_argument('--network-id', type=int, required=True)
    parser.add_argument('--asset', required=True)
    parser.add_argument('--amount', type=int, default=1000000000000000000)
    args = parser.parse_args()
    try:
        (bootstrap if args.mode == 'bootstrap' else deposit)(args)
    except (OSError, ValueError, KeyError) as error:
        raise SystemExit(f'{args.work_dir}/human-evidence-input: custody {args.mode} refused; retain artifacts for reconciliation') from None
