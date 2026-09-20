#!/usr/bin/env python3
"""Writes the layerxcustody genesis section init-chain.sh merges into the Paxeer genesis.

Custody is the native layerxcustody module behind the precompile at
0x0000000000000000000000000000000000001013. Nothing is deployed for it: the network id, the
sequencer authorization, the payout delays and the asset map are chain genesis state.
"""
import argparse
import json
import os
import re

CUSTODY_ADDRESS = '0x0000000000000000000000000000000000001013'
HASH = re.compile(r'^[0-9a-f]{64}$')
DENOM = re.compile(r'^[a-zA-Z][a-zA-Z0-9/:._-]{2,127}$')
ADDRESS = re.compile(r'^0x[0-9a-fA-F]{40}$')


def require(condition, message):
    if not condition:
        raise SystemExit('custody-genesis: ' + message)


def hash32(value, name):
    value = value.removeprefix('0x').lower()
    require(HASH.match(value) and value != '0' * 64, name + ' must be a nonzero 32-byte hex value')
    return value


def asset(value):
    parts = value.split(':')
    require(2 <= len(parts) <= 3, 'asset is ASSET_ID:DENOM[:POINTER]')
    require(DENOM.match(parts[1]), 'asset denom')
    pointer = parts[2] if len(parts) == 3 else ''
    require(not pointer or ADDRESS.match(pointer), 'asset pointer must be an EVM address')
    return dict(asset_id=hash32(parts[0], 'asset id'), denom=parts[1], pointer=pointer, enabled=True,
                paused=False, minimum_deposit='', custody_cap='')


def checkpoint(value):
    parts = value.split(':')
    require(len(parts) == 3 and parts[0].isdecimal(), 'checkpoint is BATCH:STATE_ROOT:RECEIPT_ROOT')
    return dict(batch_number=str(int(parts[0])), state_root=hash32(parts[1], 'state root'),
                receipt_root=hash32(parts[2], 'receipt root'), finalized_at='0')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--network-id', type=int, required=True)
    parser.add_argument('--sequencer-id', required=True)
    parser.add_argument('--sequencer-public-key', required=True)
    parser.add_argument('--first-batch', type=int, default=1)
    parser.add_argument('--last-batch', type=int, default=1 << 40)
    parser.add_argument('--withdrawal-delay-seconds', type=int, default=3600)
    parser.add_argument('--forced-exit-delay-seconds', type=int, default=0)
    parser.add_argument('--liveness-bound-seconds', type=int, default=86400)
    parser.add_argument('--authority', default='')
    parser.add_argument('--deposit-root-authority', default='')
    parser.add_argument('--asset', action='append', required=True, type=asset)
    parser.add_argument('--checkpoint', action='append', default=[], type=checkpoint)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    require(0 < args.network_id < 2 ** 32, 'network id')
    require(0 < args.first_batch <= args.last_batch < 2 ** 64, 'sequencer batch range')
    require(0 <= args.withdrawal_delay_seconds <= 90 * 86400
            and 0 <= args.forced_exit_delay_seconds <= 90 * 86400, 'payout delay exceeds the 90 day bound')
    require(args.liveness_bound_seconds >= 3600, 'liveness bound is below one hour')
    require(len({entry['asset_id'] for entry in args.asset}) == len(args.asset), 'duplicate asset')
    genesis = dict(
        params=dict(authority=args.authority, network_id=args.network_id,
                    withdrawal_delay_seconds=str(args.withdrawal_delay_seconds),
                    forced_exit_delay_seconds=str(args.forced_exit_delay_seconds),
                    liveness_bound_seconds=str(args.liveness_bound_seconds),
                    deposit_root_authority=hash32(args.deposit_root_authority, 'deposit root authority')
                    if args.deposit_root_authority else '',
                    sequencer_authorizations=[dict(
                        sequencer_id=hash32(args.sequencer_id, 'sequencer id'),
                        public_key=hash32(args.sequencer_public_key, 'sequencer public key'),
                        first_batch_number=str(args.first_batch), last_batch_number=str(args.last_batch))]),
        assets=args.asset, deposit_count='0', deposits=[], deposit_nonces=[], claims=[], nullifiers=[],
        checkpoints=args.checkpoint, consumed_balances=[], totals=[], emergency=False,
        deposit_roots=[])
    descriptor = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o644)
    with os.fdopen(descriptor, 'w') as output:
        json.dump(genesis, output, sort_keys=True, separators=(',', ':'))
        output.write('\n')


if __name__ == '__main__':
    main()
