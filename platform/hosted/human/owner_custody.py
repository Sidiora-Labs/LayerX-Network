import argparse
import hashlib
from pathlib import Path
import sys
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / 'tests/bridge'))
from comet_credit import CUSTODY_ADDRESS, module_identity
from custody_credit import create_profile, attest, identity_rpcs, quantity, unhex, write_new
from deploy_local_custody import calldata, deploy, govern, send, signer
from provision import protected_bytes, protected_json, require, write_json, h32

WEI_PER_BASE_UNIT = 10 ** 12


def native_bootstrap(args, root, rpc, account):
    # Paxeer custody is the layerxcustody module behind the precompile: the asset map, the sequencer
    # authorization and the delays are chain genesis state, so the bring-up deploys nothing for it.
    require(quantity(rpc.call('eth_chainId', [])) == 125, root, 'native custody chain')
    identity = module_identity().hex()
    create_profile(SimpleNamespace(**{**vars(args), 'chain_id': 125, 'vault': CUSTODY_ADDRESS,
                   'runtime_sha256': '0x' + identity, 'asset': '0x' + args.asset,
                   'output': str(root / 'custody.profile')}))
    write_json(root / 'owner-custody.json', dict(vault=CUSTODY_ADDRESS, asset=args.asset,
               runtime_sha256=identity, payer=account))


def bootstrap(args):
    root = Path(args.work_dir) / 'human-evidence-input'
    root.mkdir(mode=0o700, exist_ok=True)
    h32(args.asset, root, 'native asset')
    require(args.network_id > 0, root, 'network id')
    rpcs, _ = identity_rpcs(args)
    rpc = rpcs[0]
    account = signer(rpc, args.key_file)
    write_new(root / 'custody-bootstrap.started', b'preserve all artifacts; reconcile before retry\n')
    if getattr(rpc, 'disposable', False):
        return native_bootstrap(args, root, rpc, account)
    config = '0x' + hashlib.sha256(b'LayerX/local-custody/real-weth/v1').hexdigest()
    beta = getattr(rpc, 'disposable', False)
    timelock = deploy(rpc, account, 'contracts/governance/' +
                      ('LayerXBetaTimelock.sol:LayerXBetaTimelock' if beta else 'LayerXTimelock.sol:LayerXTimelock'),
                      0 if beta else 86400, 172800, account, account, account, 0, config, 1)
    registry = deploy(rpc, account, 'contracts/custody/AssetRegistry.sol:AssetRegistry', timelock, account, config, 1)
    token = deploy(rpc, account, 'loadtest/contracts/evm/lib/solmate/src/tokens/WETH.sol:WETH')
    vault = deploy(rpc, account, 'contracts/custody/LayerXVault.sol:LayerXVault', registry, timelock, account, config, 1)
    register = calldata('registerAsset(bytes32,address,uint8,uint128,uint128)', '0x' + args.asset, token, 18, 1, 2 ** 128 - 1)
    govern(rpc, account, timelock, timelock, calldata('setCallPermission(address,bytes4,bool)', registry, register[:10], 'true'))
    govern(rpc, account, timelock, registry, register)
    runtime = hashlib.sha256(unhex(rpc.call('eth_getCode', [vault, 'latest']))).hexdigest()
    if not beta:
        rpc.call('anvil_mine', ['0x80'], allow_missing=True)
    create_profile(SimpleNamespace(**{**vars(args), 'chain_id': quantity(rpc.call('eth_chainId', [])),
                   'vault': vault, 'runtime_sha256': '0x' + runtime, 'asset': '0x' + args.asset,
                   'output': str(root / 'custody.profile')}))
    write_json(root / 'owner-custody.json', dict(vault=vault, token=token, registry=registry,
               timelock=timelock, asset=args.asset, runtime_sha256=runtime, payer=account))


def credit_material_name(transaction):
    return 'credit-' + unhex(transaction, 32).hex() + '.bin'


def publish_credit_material(root, transaction):
    source = root / 'custody-credit.bin'
    credit = protected_bytes(source)
    require(len(credit) > 363 and credit[:5] == b'LXDC3'
            and credit[327:359] == hashlib.sha256(credit[363:]).digest(), source, 'light-client custody credit layout')
    target = root / credit_material_name(transaction)
    write_new(target, credit)
    return target


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
    require(len(profile) == 207 and profile[:5] == b'LXBC3' and profile[13:33] == unhex(config['vault'], 20)
            and profile[97:129] == bytes.fromhex(args.asset) and profile[201:207] == args.network_id.to_bytes(4, 'big') + b'\0\3',
            root, 'immutable custody profile binding')
    native = getattr(rpc, 'disposable', False)
    if native:
        require(unhex(config['vault'], 20) == unhex(CUSTODY_ADDRESS, 20) and profile[33:65] == module_identity(),
                root, 'native custody module pin')
    else:
        for endpoint in rpcs:
            require(hashlib.sha256(unhex(endpoint.call('eth_getCode', [config['vault'], 'latest']))).digest() == profile[33:65],
                    root, 'vault runtime pin')
    write_new(root / 'custody-deposit.started', b'preserve all artifacts; never blindly repeat this deposit\n')
    if native:
        # --amount is in the custody asset's base units; the precompile takes the payment in wei and
        # refuses a remainder below one base unit.
        result = send(rpc, account, CUSTODY_ADDRESS, calldata('deposit(bytes32)', '0x' + owner['owner_account']),
                      args.amount * WEI_PER_BASE_UNIT)
    else:
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
    publish_credit_material(root, result['transactionHash'])


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('mode', choices=['bootstrap', 'deposit'])
    parser.add_argument('--work-dir', required=True)
    parser.add_argument('--rpc', action='append', required=True)
    parser.add_argument('--ca-bundle')
    parser.add_argument('--disposable-identity')
    parser.add_argument('--vault-artifact')
    parser.add_argument('--comet-rpc', required=True)
    parser.add_argument('--trusted-height', type=int)
    parser.add_argument('--key-file', required=True)
    parser.add_argument('--network-id', type=int, required=True)
    parser.add_argument('--asset', required=True)
    parser.add_argument('--amount', type=int, default=1000000000000000000)
    args = parser.parse_args()
    try:
        (bootstrap if args.mode == 'bootstrap' else deposit)(args)
    except (OSError, ValueError, KeyError) as error:
        raise SystemExit(f'{args.work_dir}/human-evidence-input: custody {args.mode} refused; retain artifacts for reconciliation') from None
