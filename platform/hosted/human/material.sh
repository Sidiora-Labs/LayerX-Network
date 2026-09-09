retained_material_inventory() {
    python3 - "$CA_DIR" "$SECRETS_DIR" "$1" <<'PY'
import json
import os
from pathlib import Path
import stat
import sys
ca, secrets = map(Path, sys.argv[1:3])
manifest = secrets / 'retained-material.json'

def refuse(detail):
    raise SystemExit('beta-cluster: error: retained material refused: ' + detail)

def entries():
    result = {}
    for root in (ca, secrets):
        if root.is_symlink() or not root.is_dir():
            refuse('missing directory ' + str(root))
        for path in [root, *sorted(root.rglob('*'))]:
            if path == manifest:
                continue
            info = path.lstat()
            if not (stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode)):
                refuse('non-regular entry ' + str(path))
            if info.st_uid != os.geteuid() or (stat.S_ISREG(info.st_mode) and info.st_nlink != 1):
                refuse('ownership or link count ' + str(path))
            expected = 0o700 if path.is_dir() else 0o600
            if stat.S_IMODE(info.st_mode) != expected:
                refuse('expected mode ' + oct(expected) + ' for ' + str(path))
            result[root.name + '/' + str(path.relative_to(root))] = expected
    return result

if sys.argv[3] == 'save':
    for root in (ca, secrets):
        for path in [root, *root.rglob('*')]:
            if path.is_symlink() or not (path.is_dir() or path.is_file()):
                refuse('non-regular entry ' + str(path))
            path.chmod(0o700 if path.is_dir() else 0o600)
    value = entries()
    with manifest.open('w') as output:
        os.chmod(manifest, 0o600)
        json.dump(value, output, sort_keys=True)
else:
    if manifest.is_symlink() or not manifest.is_file():
        refuse('missing inventory ' + str(manifest))
    info = manifest.stat()
    if stat.S_IMODE(info.st_mode) != 0o600 or info.st_uid != os.geteuid() or info.st_nlink != 1:
        refuse('inventory ownership or mode')
    if info.st_size > 1048576:
        refuse('inventory exceeds bound')
    try:
        expected = json.loads(manifest.read_text())
    except (ValueError, OSError):
        refuse('invalid inventory')
    actual = entries()
    if actual != expected:
        missing = sorted(set(expected) - set(actual))
        refuse('incomplete or changed file set' + (': ' + missing[0] if missing else ''))
    for name in ('ca/ca.key', 'ca/ca.crt', 'ca/ca.der',
                 'secrets/module-registry.json', 'secrets/human/kms/kms-seal',
                 'secrets/human/kms/registry.json', 'secrets/retained-context.json'):
        if name not in actual:
            refuse('missing required file ' + name)
    if (secrets / 'human/kms/kms-seal').stat().st_size != 32:
        refuse('kms-seal must contain exactly 32 bytes')
    for name in ('config', 'agent-config', 'movement-config', 'authority-config', 'journal'):
        directory = secrets / 'human' / name
        if not directory.is_dir() or not any(directory.iterdir()):
            refuse('empty Human directory ' + name)
PY
}

retained_material_live_check() {
    kube -n "$TESTNET_NAMESPACE" get secret layerx-human-kms-material --ignore-not-found -o json \
        | python3 -c '
import base64, hashlib, json, pathlib, sys
raw = sys.stdin.buffer.read()
if not raw.strip():
    sys.exit(0)
try:
    seal = base64.b64decode(json.loads(raw)["data"]["kms-seal"], validate=True)
    local = pathlib.Path(sys.argv[1]).read_bytes()
    if len(seal) != 32 or len(local) != 32 or hashlib.sha256(seal).digest() != hashlib.sha256(local).digest():
        raise ValueError()
except (ValueError, KeyError, OSError):
    sys.exit("beta-cluster: error: retained material refused: live layerx-human-kms-material kms-seal digest differs from disk")
' "$SECRETS_DIR/human/kms/kms-seal" \
        || fail "retained material refused: live KMS seal verification failed"
}
