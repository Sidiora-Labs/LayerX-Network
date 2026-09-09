import json
from pathlib import Path
import sys

from provision import write_json


def configure(work_dir, network, sequencer, image, stateful):
    root = Path(work_dir) / 'human-evidence-input'
    work = '/run/owner/work'
    private = work + '/human-evidence-input'
    write_json(root / 'owner-native.json', dict(node_socket='/run/layerx/node/layerxd.lni.sock',
        network_id=network, owner_seed_file=private + '/owner.seed', pending_seed_file=private + '/pending.seed',
        sequencer_public_key=sequencer, layerxctl='/usr/local/bin/layerxctl', fee_limit=0,
        authority_url='https://localhost:9445', authority_token_file=private + '/authority.token',
        authority_ca_file=private + '/ca.crt', authority_state_root='/run/owner/authority'))
    pod = stateful['spec']['template']['spec']
    if any(c['name'] == 'owner-producer' for c in pod['containers']):
        raise ValueError('owner producer already present; reconcile retained execution')
    commands = '''umask 077
mkdir -m 0700 /run/owner/work
mkdir -m 0700 /run/owner/work/human-evidence-input
for name in owner-native.json recovery-policy.json recovery-guardians.json owner-admission.json custody-credit.bin owner.seed pending.seed authority.token ca.crt; do
    install -m 0600 "/run/owner/input/$name" "/run/owner/work/human-evidence-input/$name"
done
install -m 0600 /run/owner/input/human-owner-result.json /run/owner/work/human-owner-result.json
exec sleep infinity'''
    pod['containers'].append(dict(name='owner-producer', image=image, command=['/bin/sh', '-ec'], args=[commands],
        securityContext=dict(runAsUser=4021, runAsGroup=4020, runAsNonRoot=True, allowPrivilegeEscalation=False,
                             readOnlyRootFilesystem=True, capabilities=dict(drop=['ALL'])),
        resources=dict(requests=dict(cpu='100m', memory='64Mi'), limits=dict(cpu='1', memory='256Mi')),
        volumeMounts=[dict(name='owner-input', mountPath='/run/owner/input', readOnly=True),
                      dict(name='owner-work', mountPath='/run/owner'),
                      dict(name='run', mountPath='/run/layerx'),
                      dict(name='human-state', mountPath='/run/owner/authority', subPath='authority')]))
    pod['volumes'].extend([dict(name='owner-input', secret=dict(secretName='layerx-human-native-input', defaultMode=0o440)),
                          dict(name='owner-work', emptyDir=dict(medium='Memory', sizeLimit='32Mi'))])
    for field in ('status',):
        stateful.pop(field, None)
    for field in ('managedFields', 'resourceVersion', 'uid', 'creationTimestamp', 'generation'):
        stateful['metadata'].pop(field, None)
    return stateful


if __name__ == '__main__':
    source = Path(sys.argv[5])
    value = configure(sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4], json.loads(source.read_text()))
    write_json(Path(sys.argv[6]), value)
