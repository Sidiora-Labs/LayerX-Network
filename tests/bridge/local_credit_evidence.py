import argparse
import json
import os
from pathlib import Path
from types import SimpleNamespace

from comet_credit import CUSTODY_ADDRESS, module_identity
from custody_credit import attest, create_profile, require, sha, unhex, write_new
from deploy_local_custody import disposable_rpc


def existing_evidence(args, directory):
    require(len(args.rpc) == 2 and args.ca_bundle and args.disposable_identity,
            'two trusted RPC origins, CA bundle and disposable identity required')
    require(args.custody and args.asset and args.comet_rpc
            and args.beneficiary_key and args.network_id == 402 and args.trusted_height
            and args.trusted_height > 0, 'explicit cluster custody configuration required')
    rpcs = [disposable_rpc(url, args.ca_bundle, args.disposable_identity) for url in args.rpc]
    require(rpcs[0].identity != rpcs[1].identity, 'distinct trusted RPC origins required')
    custody = json.loads(Path(args.custody).read_text())
    require(unhex(custody['asset'], 32) == unhex(args.asset, 32), 'configured asset binding')
    # Paxeer custody is the native layerxcustody module behind the precompile at 0x…1013: the
    # custody record must name it and the module identity, never a deployed vault runtime.
    require(unhex(custody['vault'], 20) == unhex(CUSTODY_ADDRESS, 20)
            and unhex(custody['runtime_sha256'], 32) == module_identity(),
            'cluster custody must name the native custody precompile')
    public = unhex(args.beneficiary_key, 32)
    did = 'did:layerx:' + public.hex()
    name = ('agent:' + did + ':main').encode()
    beneficiary = '0x' + sha(b'LX:ACCOUNT:v1' + len(name).to_bytes(4, 'big') + name).hex()
    require(unhex(custody['beneficiary'], 32) == unhex(beneficiary, 32), 'configured beneficiary binding')
    identity = json.loads(Path(args.disposable_identity).read_text())
    require(custody['chain_id'] == identity['chain_id']
            and custody['comet_chain_id'] == identity['comet_chain_id']
            and unhex(custody['genesis_sha256'], 32) == unhex(identity['genesis_sha256'], 32),
            'deployment chain binding')
    profile = str(directory / 'custody.profile')
    previous_ca = os.environ.get('SSL_CERT_FILE')
    os.environ['SSL_CERT_FILE'] = str(Path(args.ca_bundle).resolve())
    try:
        create_profile(SimpleNamespace(rpc=args.rpc, ca_bundle=args.ca_bundle,
                       disposable_identity=args.disposable_identity,
                       chain_id=identity['chain_id'], network_id=args.network_id,
                       vault=custody['vault'], runtime_sha256=custody['runtime_sha256'], asset=args.asset,
                       trusted_height=args.trusted_height, comet_rpc=args.comet_rpc, output=profile))
        encoded = Path(profile).read_bytes()
        require(encoded[169:201] == identity['comet_chain_id'].encode().ljust(32, b'\0'),
                'profile disposable Comet chain binding')
        attest(SimpleNamespace(rpc=args.rpc, ca_bundle=args.ca_bundle,
               disposable_identity=args.disposable_identity, profile=profile, network_id=args.network_id,
               transaction=custody['transaction'], beneficiary=beneficiary, beneficiary_key=args.beneficiary_key,
               expected_amount=int(custody['amount']), comet_rpc=args.comet_rpc,
               output=str(directory / 'custody.credit')))
    finally:
        if previous_ca is None:
            del os.environ['SSL_CERT_FILE']
        else:
            os.environ['SSL_CERT_FILE'] = previous_ca
    write_new(directory / 'identity.json', json.dumps({
        'did': did, 'public_key': public.hex(), 'beneficiary': beneficiary,
        'asset': args.asset, 'amount': custody['amount'], 'network_id': args.network_id,
        'protocol_version': 3, 'genesis_sha256': identity['genesis_sha256'],
        'comet_chain_id': identity['comet_chain_id'], 'genesis_source': identity['genesis_source'],
        'rpc_origins': args.rpc,
    }).encode())
    print('Existing TLS-verified custody observations, profile and credit verified')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', required=True)
    parser.add_argument('--rpc', action='append', required=True)
    parser.add_argument('--comet-rpc', required=True)
    parser.add_argument('--ca-bundle')
    parser.add_argument('--disposable-identity')
    parser.add_argument('--custody')
    parser.add_argument('--asset')
    parser.add_argument('--beneficiary-key')
    parser.add_argument('--network-id', type=int)
    parser.add_argument('--trusted-height', type=int)
    args = parser.parse_args()
    directory = Path(args.output).resolve()
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    existing_evidence(args, directory)


if __name__ == '__main__':
    main()
