from pathlib import Path
import sys

from onboarding_material import write
from owner_native import Reader
from provision import protected_bytes, protected_json, require


def produce(genesis, deployment, evidence, material, ca, sequencer):
    genesis, material = Path(genesis), Path(material)
    artifact = genesis / 'genesis-handover-trust.lxt'
    if not artifact.exists():
        require(not artifact.is_symlink(), artifact, 'regular optional genesis trust')
        return
    trust = protected_bytes(artifact)
    descriptor = protected_bytes(genesis / 'paxeer-deployment-descriptor.lxgd', 105)
    require(len(descriptor) == 105 and descriptor[:5] == b'LXGD\1', genesis, 'actual builder descriptor')
    network = int.from_bytes(descriptor[5:9], 'big')
    canonical_root = descriptor[41:73]
    reader = Reader(trust, artifact)
    domain = b'LXP/public-handover-genesis/v1\0'
    require(reader.take(len(domain)) == domain and reader.number(4) == network
            and reader.span(32) == canonical_root and reader.span(32) == bytes.fromhex(sequencer),
            artifact, 'actual initial network, state root and signer pins')
    deployment = protected_json(deployment)
    require(deployment['network_id'] == network, genesis, 'deployed network')
    movement = protected_json(Path(evidence) / 'movement-policy.json')
    confirmations = movement['PAXEER_CONFIRMATIONS']
    require(type(confirmations) is int and confirmations > 0, evidence, 'finality confirmation policy')
    addresses = deployment['addresses']
    policy = dict(version=1, url='https://paxeer-boundary.layerx-testnet.svc.cluster.local:9443',
        transport='pinned-tls', trust_anchor_der=protected_bytes(ca).hex(), chain_id=deployment['chain_id'],
        request_timeout_ms=10000, registry=addresses['checkpoint_registry'].removeprefix('0x').lower(),
        guarantor_bond=addresses['guarantor_bond'].removeprefix('0x').lower(), protocol_version=3,
        network_id=network, canonical_genesis_root=canonical_root.hex(), confirmations=confirmations)
    encoded = ''.join(key + '=' + str(value) + '\n' for key, value in policy.items()).encode()
    for role in ('agent', 'authority'):
        write(material / role / 'genesis-handover-trust.lxt', trust)
        write(material / role / 'handover-finality.conf', encoded)
    write(material / 'authority-config' / 'genesis-trust-file', b'/run/authority-private/material/genesis-handover-trust.lxt')
    write(material / 'authority-config' / 'handover-finality-file', b'/run/authority-private/material/handover-finality.conf')


if __name__ == '__main__':
    produce(*sys.argv[1:])
