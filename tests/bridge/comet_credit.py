import os
from pathlib import Path
import subprocess

from custody_credit import DEPOSIT_TOPIC, PROFILE_BYTES, big, quantity, require, sha, unhex, write_new

ROOT = Path(__file__).resolve().parents[2]
PROOF_KIND = 2
CUSTODY_ADDRESS = '0x0000000000000000000000000000000000001013'
CUSTODY_STORE = 'layerxcustody'


def module_identity():
    return sha(b'LX:CUSTODY:MODULE:v1'+CUSTODY_STORE.encode()+unhex(CUSTODY_ADDRESS, 20))


def producer(rpc, *arguments):
    binary = Path(os.environ.get('LAYERX_CUSTODY_PROOF_BIN', ROOT/'build/bin/layerx-custody-proof'))
    completed = subprocess.run([str(binary), *[str(value) for value in arguments]], capture_output=True,
                               timeout=120, check=False, env=getattr(rpc, 'command_env', None))
    require(completed.returncode == 0, 'light-client proof producer refused: ' +
            completed.stderr[:1024].decode('utf-8', errors='replace'))


def comet_rpc(args):
    url = getattr(args, 'comet_rpc', None)
    require(isinstance(url, str) and url, 'explicit Comet RPC URL required')
    return url


def chain_identity(rpc):
    encoded = rpc.comet_chain_id.encode('ascii')
    require(0 < len(encoded) <= 32 and 0 not in encoded, 'Comet chain ID bound')
    return encoded.ljust(32, b'\0')


def create_profile(args, rpcs):
    require(args.chain_id == 125 and args.network_id > 0, 'Paxeer custody profile identity')
    require(unhex(args.vault, 20) == unhex(CUSTODY_ADDRESS, 20)
            and unhex(args.runtime_sha256, 32) == module_identity(), 'Paxeer custody is the native module')
    require(type(args.trusted_height) is int and 0 < args.trusted_height < 2**63, 'trusted Comet height')
    asset = unhex(args.asset, 32)
    scratch = args.output+'.produced'
    producer(rpcs[0], 'light-profile', '--rpc', comet_rpc(args), '--asset', '0x'+asset.hex(),
             '--network-id', args.network_id, '--trusted-height', args.trusted_height, '--output', scratch)
    profile = Path(scratch).read_bytes()
    name = b'system:paxeer-reserve'
    reserve = sha(b'LX:ACCOUNT:v1'+big(len(name), 4)+name)
    require(len(profile) == PROFILE_BYTES and profile[:65] == b'LXBC3'+big(125, 8)+unhex(CUSTODY_ADDRESS, 20)+module_identity()
            and profile[97:169] == asset+reserve+big(args.trusted_height, 8)
            and profile[169:201] == chain_identity(rpcs[0])
            and profile[201:207] == big(args.network_id, 4)+big(3, 2), 'Paxeer custody profile layout')
    write_new(args.output, profile)
    os.unlink(scratch)


def attest(args, rpcs, profile):
    require(len(profile) == PROFILE_BYTES and profile[:5] == b'LXBC3'
            and int.from_bytes(profile[5:13], 'big') == 125 and profile[169:201] == chain_identity(rpcs[0])
            and args.network_id > 0 and profile[201:207] == big(args.network_id, 4)+big(3, 2)
            and profile[13:33] == unhex(CUSTODY_ADDRESS, 20) and profile[33:65] == module_identity(),
            'Paxeer custody profile identity')
    transaction = '0x'+unhex(args.transaction, 32).hex()
    receipts = [rpc.call('eth_getTransactionReceipt', [transaction]) for rpc in rpcs]
    deposits = []
    for receipt in receipts:
        require(receipt is not None and unhex(receipt['transactionHash'], 32) == unhex(transaction, 32)
                and quantity(receipt['status']) == 1, 'custody deposit discovery receipt')
        logs = [value for value in receipt['logs'] if unhex(value['address'], 20) == profile[13:33]
                and value['topics'] and value['topics'][0].lower() == DEPOSIT_TOPIC]
        require(len(logs) == 1, 'exactly one custody deposit required')
        log = logs[0]
        require(log.get('removed') is False and len(log['topics']) == 4, 'custody deposit discovery log')
        deposit_id, asset, payer = [unhex(value, 32) for value in log['topics'][1:]]
        data = unhex(log['data'], 96)
        beneficiary, amount_word, nonce_word = data[:32], data[32:64], data[64:]
        amount, nonce = int.from_bytes(amount_word, 'big'), int.from_bytes(nonce_word, 'big')
        require(asset == profile[97:129] and payer[:12] == bytes(12) and
                0 < amount < 2**128 and 0 < nonce < 2**64 and amount == args.expected_amount
                and beneficiary == unhex(args.beneficiary, 32), 'custody deposit expected fields')
        domain = b'LXP/Paxeer/custody-deposit/v1'
        preimage = (big(256, 32)+big(125, 32)+bytes(12)+profile[13:33]+payer+asset+beneficiary+
                    amount_word+nonce_word+big(len(domain), 32)+domain.ljust(32, b'\0'))
        require(sha(preimage) == deposit_id, 'deposit ID preimage')
        deposits.append((deposit_id, asset, beneficiary, payer[12:], amount, nonce))
    require(deposits[0] == deposits[1], 'deposit discovery quorum')
    deposit_id, asset, beneficiary, payer, amount, nonce = deposits[0]
    owner = unhex(args.beneficiary_key, 32)
    scratch = args.output+'.produced'
    producer(rpcs[0], 'light-credit', '--rpc', comet_rpc(args), '--profile', args.profile,
             '--deposit-id', '0x'+deposit_id.hex(), '--owner-key', '0x'+owner.hex(), '--output', scratch)
    credit = Path(scratch).read_bytes()
    require(len(credit) > 363 and credit[:43] == b'LXDC3'+sha(profile)+big(args.network_id, 4)+big(3, 2)
            and credit[327:363] == sha(credit[363:])+big(PROOF_KIND, 4), 'Paxeer custody credit layout')
    require(credit[43:75] == deposit_id and credit[75:107] == asset and credit[107:139] == beneficiary
            and credit[139:171] == owner and credit[171:191] == payer and credit[191:207] == big(amount, 16)
            and credit[207:215] == big(nonce, 8), 'custody record and deposit log binding')
    state_height = int.from_bytes(credit[215:223], 'big')
    require(int.from_bytes(credit[287:295], 'big') == state_height+1
            and state_height >= max(quantity(receipt['blockNumber']) for receipt in receipts),
            'custody credit state height')
    nullifier = sha(b'LX:DEPOSIT:NULLIFIER:v1'+deposit_id).hex().encode()+b'\n'
    write_new(args.output, credit)
    write_new(args.output+'.nullifier', nullifier)
    os.unlink(scratch)
    os.unlink(scratch+'.nullifier')
