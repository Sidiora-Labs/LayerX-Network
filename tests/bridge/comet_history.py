import argparse
import json
import os
from pathlib import Path
import time

from comet_credit import canonical, comet, positive_decimal, verifier
from custody_credit import attest, identity_rpcs, require, write_new


def qualify(args):
    rpcs, genesis = identity_rpcs(args)
    original = json.loads(Path(args.evidence).read_bytes())['requests'][0]
    state = Path(args.history_state)
    os.environ['LAYERX_CUSTODY_HISTORY_STATE'] = str(state)
    os.environ['LAYERX_CUSTODY_HISTORY_KEY'] = args.attestor_key
    previous = verifier(original | {'operation': 'status'})
    target = max(8192+257, previous['height']+257)
    deadline = time.monotonic()+1800
    while True:
        budget = {'calls': 0, 'bytes': 0}
        for rpc in rpcs:
            rpc.budget = budget
        tips = [positive_decimal(comet(rpc, 'commit', {})['signed_header']['header']['height']) for rpc in rpcs]
        if min(tips) >= target+3:
            break
        require(time.monotonic() < deadline, 'real Comet history window deadline')
        time.sleep(1)
    evidence_dir = Path(args.output).parent/'history-chunks'
    evidence_dir.mkdir(mode=0o700, exist_ok=False)
    args.history_evidence = str(evidence_dir)
    attest(args)
    completed = json.loads(Path(args.output+'.proof.json').read_bytes())
    request = completed['requests'][0]
    require(request['bundle']['finalized_height'] >= target, 'real Comet lifetime window not crossed')
    reopened = verifier(request | {'operation': 'status'})
    require(reopened['height'] > previous['height'] and reopened['height'] == request['bundle']['finalized_height']+1,
            'authenticated history restart or progress')
    require(reopened['prefix'] != previous['prefix'], 'history prefix did not advance')
    require(verifier(request) == completed['results'][0], 'post-pruning real state proof')
    try:
        verifier(request | {'operation': 'export'})
    except ValueError as error:
        require('individual history export bound' in str(error), 'unexpected retained-history refusal')
    else:
        raise ValueError('unbounded full-history export accepted')
    try:
        verifier(original)
    except ValueError:
        pass
    else:
        raise ValueError('old endpoint history accepted after authenticated progress')
    write_new(args.output+'.history.json', canonical(dict(previous=previous, reopened=reopened,
              genesis_sha256='0x'+genesis.hex(), credit=request['expected']['deposit_id']))+b'\n')
    print('genuine Comet history crossed 8192 records, reopened monotonically and verified custody after pruning')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--rpc', action='append', required=True)
    parser.add_argument('--ca-bundle', required=True)
    parser.add_argument('--disposable-identity', required=True)
    parser.add_argument('--vault-artifact', required=True)
    parser.add_argument('--history-state', required=True)
    parser.add_argument('--evidence', required=True)
    parser.add_argument('--profile', required=True)
    parser.add_argument('--network-id', type=int, required=True)
    parser.add_argument('--transaction', required=True)
    parser.add_argument('--beneficiary', required=True)
    parser.add_argument('--beneficiary-key', required=True)
    parser.add_argument('--expected-amount', type=int, required=True)
    parser.add_argument('--attestor-key', required=True)
    parser.add_argument('--output', required=True)
    qualify(parser.parse_args())
