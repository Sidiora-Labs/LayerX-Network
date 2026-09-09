#!/usr/bin/env bash

human_owner_provision() (
    set -euo pipefail
    umask 077
    local input="$WORK_DIR/human-evidence-input" output="$WORK_DIR/human-owner-result.json"
    local manifest="$WORK_DIR/human-provision-owner-job.json" state="$WORK_DIR/human-provision-node.json"
    [ ! -e "$output" ] || fail "$output: existing owner result requires explicit reconciliation with retained state"
    python3 "$REPO_ROOT/platform/hosted/human/provision.py" --validate-job-input --work-dir "$WORK_DIR"
    python3 "$REPO_ROOT/platform/hosted/human/provision.py" --account-requests --work-dir "$WORK_DIR"
    kube -n "$TESTNET_NAMESPACE" get statefulset layerx-node -o json > "$state"
    python3 - "$state" <<'PY'
import json
import sys
value = json.load(open(sys.argv[1]))
containers = value['spec']['template']['spec']['containers']
if any(c['name'].startswith('human') for c in containers):
    raise SystemExit('layerx-node: Human runtime must not be enabled during owner provisioning')
PY
    kube -n "$TESTNET_NAMESPACE" get pods -l app=layerx-node -o json > "$state"
    python3 - "$state" <<'PY'
import json
import sys
value = json.load(open(sys.argv[1]))
if len(value['items']) != 1:
    raise SystemExit('layerx-node: exactly one bootstrap pod is required')
pod = value['items'][0]
if any(c['name'].startswith('human') for c in pod['spec']['containers']):
    raise SystemExit('layerx-node: Human runtime pod must be stopped before owner provisioning')
if not pod['spec'].get('nodeName'):
    raise SystemExit('layerx-node: bootstrap pod is not scheduled')
PY
    python3 - "$REPO_ROOT/platform/hosted/human/provision-owner-job.yaml" "$manifest" \
        "$TESTNET_NAMESPACE" "$(image_ref layerx-human)" "$state" <<'PY'
import json
import sys
import yaml
value = yaml.safe_load(open(sys.argv[1]))
value['metadata']['namespace'] = sys.argv[3]
pod = value['spec']['template']['spec']
for container in pod['containers'] + pod['initContainers']:
    container['image'] = sys.argv[4]
pod['nodeName'] = json.load(open(sys.argv[5]))['items'][0]['spec']['nodeName']
with open(sys.argv[2], 'x') as output:
    json.dump(value, output)
PY
    apply_secret "$TESTNET_NAMESPACE" layerx-human-provision-owner-input \
        --from-file=owner-request.json="$input/owner-request.json" \
        --from-file=recovery-policy.json="$input/recovery-policy.json" \
        --from-file=treasury-request.json="$input/treasury-request.json" \
        --from-file=sequencer-request.json="$input/sequencer-request.json" \
        --from-file=account-head-request.json="$input/account-head-request.json"
    kube create -f "$manifest" > /dev/null
    kube -n "$TESTNET_NAMESPACE" wait --for=condition=complete --timeout=150s job/layerx-human-provision-owner > /dev/null \
        || fail 'layerx-human-provision-owner: Job did not complete; owner result not published'
    kube -n "$TESTNET_NAMESPACE" logs job/layerx-human-provision-owner -c provision-owner > "$output.pending"
    python3 "$REPO_ROOT/platform/hosted/human/provision.py" --validate-owner-result \
        --work-dir "$WORK_DIR" --request "$output.pending"
    mv "$output.pending" "$output"
    kube -n "$TESTNET_NAMESPACE" logs job/layerx-human-provision-owner -c validate-account-head > "$input/account-head-result.json"
    local account
    for account in treasury sequencer; do
        kube -n "$TESTNET_NAMESPACE" logs job/layerx-human-provision-owner -c "provision-$account" > "$input/$account.json"
    done
)

human_journal_materialize() (
    set -euo pipefail
    umask 077
    local stage pod manifest
    stage=$(mktemp -d "$WORK_DIR/.journal-export-XXXXXXXX")
    pod="human-journal-${stage##*-}"
    pod=${pod,,}
    manifest="$stage/pod.json"
    trap 'rm -rf "$stage"' EXIT
    kube -n "$TESTNET_NAMESPACE" get pod layerx-program-registry-0 -o json > "$stage/registry.json"
    python3 - "$stage/registry.json" "$manifest" "$pod" <<'PYJOURNAL'
import json
import sys
source = json.load(open(sys.argv[1]))
spec = source['spec']
registry = next(c for c in spec['containers'] if c['name'] == 'registry')
path = next(e['value'] for e in registry['env'] if e['name'] == 'LAYERX_REGISTRY_JOURNAL')
mount = next(m for m in registry['volumeMounts'] if m['mountPath'] == path)
volume = next(v for v in spec['volumes'] if v['name'] == mount['name'])
if (path != '/var/lib/layerx-registry-journal' or mount.get('subPath') != 'journal'
        or volume['persistentVolumeClaim']['claimName'] != 'layerx-registry-journal'
        or not spec.get('nodeName')):
    raise SystemExit('registry journal PVC binding refused')
value = {'apiVersion': 'v1', 'kind': 'Pod',
    'metadata': {'name': sys.argv[3], 'namespace': source['metadata']['namespace']},
    'spec': {'nodeName': spec['nodeName'], 'restartPolicy': 'Never',
        'automountServiceAccountToken': False, 'activeDeadlineSeconds': 300,
        'securityContext': {'runAsNonRoot': True, 'runAsUser': 4030, 'runAsGroup': 4030},
        'containers': [{'name': 'journal', 'image': registry['image'],
            'imagePullPolicy': registry['imagePullPolicy'], 'command': ['sleep', '300'],
            'securityContext': {'allowPrivilegeEscalation': False, 'readOnlyRootFilesystem': True,
                'capabilities': {'drop': ['ALL']}, 'seccompProfile': {'type': 'RuntimeDefault'}},
            'resources': {'requests': {'cpu': '10m', 'memory': '16Mi'},
                'limits': {'cpu': '100m', 'memory': '64Mi'}},
            'volumeMounts': [dict(mount, readOnly=True)]}],
        'volumes': [{'name': volume['name'], 'persistentVolumeClaim':
            dict(volume['persistentVolumeClaim'], readOnly=True)}]}}
with open(sys.argv[2], 'x') as output:
    json.dump(value, output)
PYJOURNAL
    kube create -f "$manifest" > /dev/null
    trap 'kube -n "$TESTNET_NAMESPACE" delete pod "$pod" --wait=false > /dev/null; rm -rf "$stage"' EXIT
    kube -n "$TESTNET_NAMESPACE" wait --for=condition=Ready "pod/$pod" --timeout=120s > /dev/null
    mkdir -m 0700 "$stage/journal"
    kube -n "$TESTNET_NAMESPACE" cp -c journal "$pod:/var/lib/layerx-registry-journal/pairs/." "$stage/journal"
    python3 "$REPO_ROOT/platform/hosted/human/provision.py" --materialize-journal \
        --work-dir "$WORK_DIR" --journal "$stage/journal"
)

human_journal_deploy() (
    set -euo pipefail
    umask 077
    local request="$WORK_DIR/human-evidence-input/program-deployment.lxa"
    local response="$WORK_DIR/registry-deployment-result.json" status
    local provision="$REPO_ROOT/platform/hosted/human/provision.py"
    python3 - "$provision" "$request" "$response" <<'PYDEPLOY'
import importlib.util
from pathlib import Path
import sys
spec = importlib.util.spec_from_file_location('provision', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.protected_bytes(Path(sys.argv[2]))
module.require(not Path(sys.argv[3]).exists() and not Path(sys.argv[3]).is_symlink(),
               sys.argv[3], 'existing deployment result requires reconciliation')
PYDEPLOY
    port_forward registry "$TESTNET_NAMESPACE" layerx-program-registry 19455 9420
    status=$(curl --silent --show-error --max-time 120 --max-filesize 1048576 --noproxy '*' \
        --cacert "$CA_DIR/ca.crt" --cert "$CA_DIR/gateway-client/cert.pem" --key "$CA_DIR/gateway-client/key.pem" \
        --connect-to 'layerx-program-registry:9420:127.0.0.1:19455' \
        --header "Authorization: Bearer $(cat "$SECRETS_DIR/registry-request.token")" \
        --header 'Content-Type: application/octet-stream' --data-binary "@$request" \
        --output "$response" --write-out '%{http_code}' \
        'https://layerx-program-registry:9420/__registry/deployments')
    [ "$status" = 200 ] || fail "registry ingress refused deployment with status $status; evidence not published"
    human_journal_materialize
    python3 - "$provision" "$response" "$WORK_DIR/registry-journal" <<'PYPAIR'
import importlib.util
from pathlib import Path
import sys
spec = importlib.util.spec_from_file_location('provision', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
response = module.protected_json(Path(sys.argv[2]))
module.fields(response, 'activity_id receipt_digest state', sys.argv[2], 'deployment response')
module.require(response['state'] == 'deployed', sys.argv[2], 'deployed state')
for key in ('activity_id', 'receipt_digest'):
    module.h32(response[key], sys.argv[2], key)
records = module.journal_records(Path(sys.argv[3]))
for suffix in ('.admission', '.deployment'):
    module.require(response['receipt_digest'] + suffix in records, sys.argv[3], 'ingress journal pair')
PYPAIR
)

human_evidence_provision() (
    set -euo pipefail
    umask 077
    local input="$WORK_DIR/human-evidence-input" status
    local provision="$REPO_ROOT/platform/hosted/human/provision.py"
    python3 "$provision" --validate-owner-registration --work-dir "$WORK_DIR"
    python3 "$provision" --validate-job-input --work-dir "$WORK_DIR"
    python3 "$provision" --validate-evidence-inputs --work-dir "$WORK_DIR" \
        --registry "$SECRETS_DIR/module-registry.json" --journal "$WORK_DIR/registry-journal"
    kube -n "$TESTNET_NAMESPACE" get secret layerx-guarantor-checkpoint-authority \
        -o 'jsonpath={.data.public\.hex}' > "$input/checkpoint-public.base64" \
        || fail 'Secret layerx-guarantor-checkpoint-authority/public.hex: checkpoint producer output required'
    python3 "$provision" --movement-source --work-dir "$WORK_DIR" --secrets-dir "$SECRETS_DIR"
    port_forward human-account-head "$TESTNET_NAMESPACE" layerx-agent-boundary 19454 9443
    status=$(curl --silent --show-error --max-time 30 --max-filesize 1048576 \
        --cacert "$CA_DIR/ca.crt" --header "Authorization: Bearer $(cat "$SECRETS_DIR/registry-node.token")" \
        --output "$input/account-head.json" --write-out '%{http_code}' \
        'https://localhost:19454/v1/protocol/account-state/head')
    [ "$status" = 200 ] || fail "$input/account-head.json: agent boundary refused account-state head with status $status"
    python3 - "$provision" "$input" "$NODE_NETWORK_ID" "$NODE_SEQUENCER_ID" "$NODE_SEQUENCER_PUBLIC_KEY" <<'PYHEAD'
import importlib.util
from pathlib import Path
import sys
spec = importlib.util.spec_from_file_location('provision', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
root = Path(sys.argv[2])
module.write_json(root / 'account-head-request.json', {
    'head': module.protected_json(root / 'account-head.json'), 'network_id': int(sys.argv[3]),
    'sequencer_id': sys.argv[4], 'public_key': sys.argv[5]})
PYHEAD
    human_owner_provision
    python3 "$provision" --assemble --work-dir "$WORK_DIR" \
        --registry "$SECRETS_DIR/module-registry.json" --asset "$NODE_ASSET_ID" \
        --journal "$WORK_DIR/registry-journal"
)
