#!/usr/bin/env bash
set -euo pipefail
umask 077

PROGRAM=${0##*/}
DEFAULT_PREFIX=/opt/layerx/relay_archive
DEFAULT_CONFIG=/etc/layerx/relay-archive.json
REQUIRED_PAYLOAD=(
    layerxd
    layerx-archive-codec
    __init__.py
    runtime.py
    store.py
    protocol.py
    forward.py
    peers.py
    layerx-relay-archive.service
)

fail() {
    printf '%s: %s\n' "$PROGRAM" "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
usage: install.sh (--bundle DIR | --release HTTPS_URL) --manifest-sha256 HEX [options]

Release verification:
  --bundle DIR                 install an already unpacked release bundle
  --release HTTPS_URL          download a flat tar release over HTTPS
  --expected-sha256 HEX        required SHA-256 of a downloaded release tar
  --manifest-sha256 HEX        required independent SHA-256 of manifest.sha256

Installation:
  --prefix DIR                 software prefix (default /opt/layerx/relay_archive)
  --config FILE                existing config, or destination for generated config
  --no-service                 rootless install; do not create a user or systemd unit

Required when --config does not exist:
  --network-id N
  --genesis-sha256 HEX
  --sequencer-id HEX
  --sequencer-public-key HEX
  --data-dir DIR
  --listen HOST:PORT
  --public-url ORIGIN

Optional generated-config fields:
  --codec FILE                 default PREFIX/layerx-archive-codec
  --upstream ORIGIN            repeatable read synchronization origin
  --submission-upstream ORIGIN repeatable activity-forwarding origin
  --genesis-manifest FILE      paired with --genesis-snapshot
  --genesis-snapshot FILE      paired with --genesis-manifest
  --source-log FILE            canonical source availability log
  --ca-file FILE
  --tls-cert FILE              paired with --tls-key
  --tls-key FILE               paired with --tls-cert
  --peer-seed ORIGIN           repeatable public discovery seed
  --allow-loopback-dev         explicitly allow loopback HTTP for local development
EOF
}

require_value() {
    [ "$#" -ge 2 ] || fail "$1 requires a value"
    [ -n "$2" ] || fail "$1 requires a non-empty value"
}

is_sha256() {
    [[ $1 =~ ^[0-9a-f]{64}$ ]] && [ "$1" != "$(printf '0%.0s' {1..64})" ]
}

BUNDLE=
RELEASE=
EXPECTED_SHA256=
MANIFEST_SHA256=
PREFIX=$DEFAULT_PREFIX
CONFIG=$DEFAULT_CONFIG
NO_SERVICE=0
CONFIG_CREATED=0
NETWORK_ID=
GENESIS_SHA256=
SEQUENCER_ID=
SEQUENCER_PUBLIC_KEY=
DATA_DIR=
LISTEN=
PUBLIC_URL=
CODEC=
GENESIS_MANIFEST=
GENESIS_SNAPSHOT=
SOURCE_LOG=
CA_FILE=
TLS_CERT=
TLS_KEY=
ALLOW_LOOPBACK_DEV=0
UPSTREAMS=()
SUBMISSION_UPSTREAMS=()
PEER_SEEDS=()

while [ "$#" -gt 0 ]; do
    case "$1" in
        --bundle) require_value "$@"; BUNDLE=$2; shift 2 ;;
        --release) require_value "$@"; RELEASE=$2; shift 2 ;;
        --expected-sha256) require_value "$@"; EXPECTED_SHA256=$2; shift 2 ;;
        --manifest-sha256) require_value "$@"; MANIFEST_SHA256=$2; shift 2 ;;
        --prefix) require_value "$@"; PREFIX=$2; shift 2 ;;
        --config) require_value "$@"; CONFIG=$2; shift 2 ;;
        --network-id) require_value "$@"; NETWORK_ID=$2; shift 2 ;;
        --genesis-sha256) require_value "$@"; GENESIS_SHA256=$2; shift 2 ;;
        --sequencer-id) require_value "$@"; SEQUENCER_ID=$2; shift 2 ;;
        --sequencer-public-key) require_value "$@"; SEQUENCER_PUBLIC_KEY=$2; shift 2 ;;
        --data-dir) require_value "$@"; DATA_DIR=$2; shift 2 ;;
        --listen) require_value "$@"; LISTEN=$2; shift 2 ;;
        --public-url) require_value "$@"; PUBLIC_URL=$2; shift 2 ;;
        --codec) require_value "$@"; CODEC=$2; shift 2 ;;
        --upstream) require_value "$@"; UPSTREAMS+=("$2"); shift 2 ;;
        --submission-upstream) require_value "$@"; SUBMISSION_UPSTREAMS+=("$2"); shift 2 ;;
        --genesis-manifest) require_value "$@"; GENESIS_MANIFEST=$2; shift 2 ;;
        --genesis-snapshot) require_value "$@"; GENESIS_SNAPSHOT=$2; shift 2 ;;
        --source-log) require_value "$@"; SOURCE_LOG=$2; shift 2 ;;
        --ca-file) require_value "$@"; CA_FILE=$2; shift 2 ;;
        --tls-cert) require_value "$@"; TLS_CERT=$2; shift 2 ;;
        --tls-key) require_value "$@"; TLS_KEY=$2; shift 2 ;;
        --peer-seed) require_value "$@"; PEER_SEEDS+=("$2"); shift 2 ;;
        --allow-loopback-dev) ALLOW_LOOPBACK_DEV=1; shift ;;
        --no-service) NO_SERVICE=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) fail "unknown argument: $1" ;;
    esac
done

[ -n "$BUNDLE" ] || [ -n "$RELEASE" ] || fail "one of --bundle or --release is required"
[ -z "$BUNDLE" ] || [ -z "$RELEASE" ] || fail "--bundle and --release are mutually exclusive"
is_sha256 "$MANIFEST_SHA256" || fail "--manifest-sha256 must be a non-zero lowercase SHA-256"
if [ -n "$RELEASE" ]; then
    is_sha256 "$EXPECTED_SHA256" || fail "--expected-sha256 is required for --release"
fi

python3 - "$PREFIX" "$CONFIG" <<'PY'
import os
import pathlib
import sys

for name, value in (("prefix", sys.argv[1]), ("config", sys.argv[2])):
    path = pathlib.PurePosixPath(value)
    if not path.is_absolute() or value == "/" or ".." in path.parts or "\n" in value:
        raise SystemExit(f"{name} must be a safe absolute path")
PY

WORK_DIR=$(mktemp -d)
STAGING=
cleanup() {
    if [ -n "$STAGING" ] && [ -d "$STAGING" ]; then
        rm -rf -- "$STAGING"
    fi
    rm -rf -- "$WORK_DIR"
}
trap cleanup EXIT HUP INT TERM

if [ -n "$RELEASE" ]; then
    python3 - "$RELEASE" <<'PY'
import sys
import urllib.parse

value = urllib.parse.urlsplit(sys.argv[1])
if (value.scheme != "https" or value.hostname is None or value.username is not None
        or value.password is not None or value.query or value.fragment):
    raise SystemExit("release URL must be credential-free HTTPS")
PY
    command -v curl >/dev/null 2>&1 || fail "curl is required for --release"
    ARCHIVE=$WORK_DIR/release.tar
    curl --fail --silent --show-error --location --max-redirs 3 \
        --proto '=https' --proto-redir '=https' --tlsv1.2 \
        --output "$ARCHIVE" "$RELEASE"
    [ "$(wc -c < "$ARCHIVE")" -le 536870912 ] || fail "release archive exceeds 512 MiB"
    ACTUAL=$(sha256sum "$ARCHIVE" | awk '{print $1}')
    [ "$ACTUAL" = "$EXPECTED_SHA256" ] || fail "release archive SHA-256 mismatch"
    BUNDLE=$WORK_DIR/bundle
    mkdir -m 0700 "$BUNDLE"
    python3 - "$ARCHIVE" "$BUNDLE" "${REQUIRED_PAYLOAD[@]}" <<'PY'
import os
import pathlib
import shutil
import sys
import tarfile

archive = pathlib.Path(sys.argv[1])
destination = pathlib.Path(sys.argv[2])
allowed = set(sys.argv[3:]) | {"manifest.sha256"}
with tarfile.open(archive, mode="r:*") as source:
    members = source.getmembers()
    names = [member.name for member in members]
    if len(names) != len(set(names)) or set(names) != allowed:
        raise SystemExit("release archive does not contain the exact flat payload")
    if sum(member.size for member in members) > 536870912:
        raise SystemExit("release archive expands beyond 512 MiB")
    for member in members:
        if not member.isfile() or member.size < 0 or member.size > 268435456:
            raise SystemExit("release archive contains a non-regular or oversized entry")
        name = pathlib.PurePosixPath(member.name)
        if name.name != member.name:
            raise SystemExit("release archive entry is not flat")
        stream = source.extractfile(member)
        if stream is None:
            raise SystemExit("release archive entry could not be read")
        target = destination / member.name
        descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as output:
            shutil.copyfileobj(stream, output, length=1024 * 1024)
PY
else
    [ -d "$BUNDLE" ] || fail "bundle directory does not exist"
fi

MANIFEST=$BUNDLE/manifest.sha256
[ -f "$MANIFEST" ] && [ ! -L "$MANIFEST" ] || fail "bundle manifest must be a regular non-symlink file"
[ "$(wc -c < "$MANIFEST")" -le 65536 ] || fail "bundle manifest exceeds 64 KiB"
ACTUAL_MANIFEST=$(sha256sum "$MANIFEST" | awk '{print $1}')
[ "$ACTUAL_MANIFEST" = "$MANIFEST_SHA256" ] || fail "bundle manifest SHA-256 mismatch"

python3 - "$BUNDLE" "${REQUIRED_PAYLOAD[@]}" <<'PY'
import hashlib
import os
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
required = set(sys.argv[2:])
if set(os.listdir(root)) != required | {"manifest.sha256"}:
    raise SystemExit("bundle must contain only the declared payload and manifest.sha256")
manifest = root / "manifest.sha256"
lines = manifest.read_text(encoding="ascii").splitlines()
entries = {}
for line in lines:
    match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_][A-Za-z0-9._-]*)", line)
    if match is None or match.group(2) in entries:
        raise SystemExit("bundle manifest is non-canonical")
    entries[match.group(2)] = match.group(1)
if set(entries) != required or len(lines) != len(required):
    raise SystemExit("bundle manifest does not name the exact required payload")
for name, expected in entries.items():
    path = root / name
    if not path.is_file() or path.is_symlink() or path.stat().st_size > 268435456:
        raise SystemExit(f"bundle payload {name} is not a bounded regular file")
    digest = hashlib.file_digest(path.open("rb"), "sha256").hexdigest()
    if digest != expected:
        raise SystemExit(f"bundle payload {name} failed SHA-256 verification")
if sum((root / name).stat().st_size for name in entries) > 536870912:
    raise SystemExit("bundle payload exceeds 512 MiB")
PY

if [ -e "$CONFIG" ]; then
    [ -f "$CONFIG" ] && [ ! -L "$CONFIG" ] || fail "existing config must be a regular non-symlink file"
    if [ -n "$NETWORK_ID$GENESIS_SHA256$SEQUENCER_ID$SEQUENCER_PUBLIC_KEY$DATA_DIR$LISTEN$PUBLIC_URL$CODEC$GENESIS_MANIFEST$GENESIS_SNAPSHOT$SOURCE_LOG$CA_FILE$TLS_CERT$TLS_KEY" ] \
        || [ "${#UPSTREAMS[@]}" -ne 0 ] || [ "${#SUBMISSION_UPSTREAMS[@]}" -ne 0 ] \
        || [ "${#PEER_SEEDS[@]}" -ne 0 ] || [ "$ALLOW_LOOPBACK_DEV" -ne 0 ]; then
        fail "config generation options cannot be combined with an existing --config file"
    fi
else
    [ -n "$NETWORK_ID" ] || fail "--network-id is required to generate config"
    is_sha256 "$GENESIS_SHA256" || fail "--genesis-sha256 must be a non-zero lowercase SHA-256"
    is_sha256 "$SEQUENCER_ID" || fail "--sequencer-id must be a non-zero lowercase SHA-256"
    is_sha256 "$SEQUENCER_PUBLIC_KEY" || fail "--sequencer-public-key must be a non-zero lowercase hexadecimal key"
    [ -n "$DATA_DIR" ] || fail "--data-dir is required to generate config"
    [ -n "$LISTEN" ] || fail "--listen is required to generate config"
    [ -n "$PUBLIC_URL" ] || fail "--public-url is required to generate config"
    if [ -n "$GENESIS_MANIFEST" ] || [ -n "$GENESIS_SNAPSHOT" ]; then
        [ -n "$GENESIS_MANIFEST" ] && [ -n "$GENESIS_SNAPSHOT" ] \
            || fail "--genesis-manifest and --genesis-snapshot must be supplied together"
    fi
    if [ -n "$TLS_CERT" ] || [ -n "$TLS_KEY" ]; then
        [ -n "$TLS_CERT" ] && [ -n "$TLS_KEY" ] || fail "--tls-cert and --tls-key must be supplied together"
    fi
fi

mkdir -p "$PREFIX/releases"
chmod 0755 "$PREFIX" "$PREFIX/releases"
RELEASE_DIR=$PREFIX/releases/$ACTUAL_MANIFEST
if [ ! -e "$RELEASE_DIR" ]; then
    STAGING=$(mktemp -d "$PREFIX/releases/.install.XXXXXX")
    install -m 0755 "$BUNDLE/layerxd" "$STAGING/layerxd"
    install -m 0755 "$BUNDLE/layerx-archive-codec" "$STAGING/layerx-archive-codec"
    install -m 0755 "$BUNDLE/runtime.py" "$STAGING/runtime.py"
    install -m 0644 "$BUNDLE/__init__.py" "$STAGING/__init__.py"
    install -m 0644 "$BUNDLE/store.py" "$STAGING/store.py"
    install -m 0644 "$BUNDLE/protocol.py" "$STAGING/protocol.py"
    install -m 0644 "$BUNDLE/forward.py" "$STAGING/forward.py"
    install -m 0644 "$BUNDLE/peers.py" "$STAGING/peers.py"
    install -m 0644 "$BUNDLE/layerx-relay-archive.service" "$STAGING/layerx-relay-archive.service"
    install -m 0644 "$MANIFEST" "$STAGING/manifest.sha256"
    python3 - "$STAGING" <<'PY'
import hashlib
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
for line in (root / "manifest.sha256").read_text(encoding="ascii").splitlines():
    expected, name = re.fullmatch(r"([0-9a-f]{64})  (.+)", line).groups()
    if hashlib.file_digest((root / name).open("rb"), "sha256").hexdigest() != expected:
        raise SystemExit("installed payload changed after verification")
PY
    chmod 0755 "$STAGING"
    mv "$STAGING" "$RELEASE_DIR"
    STAGING=
else
    [ -d "$RELEASE_DIR" ] && [ ! -L "$RELEASE_DIR" ] || fail "existing release path is unsafe"
fi

[ -f "$RELEASE_DIR/manifest.sha256" ] && [ ! -L "$RELEASE_DIR/manifest.sha256" ] \
    || fail "installed release manifest is unsafe"
cmp -s "$MANIFEST" "$RELEASE_DIR/manifest.sha256" \
    || fail "installed release manifest differs from the pinned bundle"
python3 - "$RELEASE_DIR" "${REQUIRED_PAYLOAD[@]}" <<'PY'
import hashlib
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
required = set(sys.argv[2:])
if {path.name for path in root.iterdir()} != required | {"manifest.sha256"}:
    raise SystemExit("installed release contains an undeclared payload")
entries = {}
for line in (root / "manifest.sha256").read_text(encoding="ascii").splitlines():
    match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_][A-Za-z0-9._-]*)", line)
    if match is None or match.group(2) in entries:
        raise SystemExit("installed release manifest is non-canonical")
    entries[match.group(2)] = match.group(1)
if set(entries) != required:
    raise SystemExit("installed release manifest has the wrong payload")
for name, expected in entries.items():
    path = root / name
    if not path.is_file() or path.is_symlink():
        raise SystemExit(f"installed payload {name} is unsafe")
    if hashlib.file_digest(path.open("rb"), "sha256").hexdigest() != expected:
        raise SystemExit(f"installed payload {name} failed re-verification")
PY

for name in layerxd layerx-archive-codec __init__.py runtime.py store.py protocol.py forward.py peers.py; do
    target=$PREFIX/$name
    if { [ -e "$target" ] || [ -L "$target" ]; } && [ ! -L "$target" ]; then
        fail "refusing to replace non-symlink software path $target"
    fi
    link=$PREFIX/."$name".new.$$
    ln -s "releases/$ACTUAL_MANIFEST/$name" "$link"
    mv -Tf "$link" "$target"
done

if [ -z "$CODEC" ]; then
    CODEC=$PREFIX/layerx-archive-codec
fi

if [ ! -e "$CONFIG" ]; then
    CONFIG_PARENT=$(dirname -- "$CONFIG")
    mkdir -p "$CONFIG_PARENT"
    if [ "$NO_SERVICE" -eq 1 ]; then
        if [ ! -e "$DATA_DIR" ]; then
            mkdir -m 0700 -p "$DATA_DIR"
        else
            [ -d "$DATA_DIR" ] && [ ! -L "$DATA_DIR" ] \
                || fail "existing data path must be a non-symlink directory"
        fi
    fi
    python3 - "$CONFIG" "$NETWORK_ID" "$GENESIS_SHA256" "$SEQUENCER_ID" \
        "$SEQUENCER_PUBLIC_KEY" "$DATA_DIR" "$LISTEN" "$PUBLIC_URL" "$CODEC" \
        "$GENESIS_MANIFEST" "$GENESIS_SNAPSHOT" "$SOURCE_LOG" "$CA_FILE" \
        "$TLS_CERT" "$TLS_KEY" "$ALLOW_LOOPBACK_DEV" \
        "${#UPSTREAMS[@]}" "${#SUBMISSION_UPSTREAMS[@]}" "${#PEER_SEEDS[@]}" \
        "${UPSTREAMS[@]}" "${SUBMISSION_UPSTREAMS[@]}" "${PEER_SEEDS[@]}" <<'PY'
import ipaddress
import json
import os
import pathlib
import re
import sys
import urllib.parse

(
    destination, network_id, genesis_sha256, sequencer_id, sequencer_public_key,
    data_dir, listen, public_url, codec, genesis_manifest, genesis_snapshot,
    source_log, ca_file, tls_cert, tls_key, allow_loopback, upstream_count,
    submission_count, peer_count, *values
) = sys.argv[1:]
allow_loopback = allow_loopback == "1"
upstream_count = int(upstream_count)
submission_count = int(submission_count)
peer_count = int(peer_count)
if len(values) != upstream_count + submission_count + peer_count:
    raise SystemExit("internal config argument count differs")
upstreams = values[:upstream_count]
submission_upstreams = values[upstream_count:upstream_count + submission_count]
peer_seeds = values[upstream_count + submission_count:]

def absolute_path(value, name, required=True):
    if not value and not required:
        return
    path = pathlib.PurePosixPath(value)
    if not path.is_absolute() or ".." in path.parts or "\n" in value:
        raise SystemExit(f"{name} must be a safe absolute path")

for value, name, required in (
    (data_dir, "data_dir", True), (codec, "codec", True),
    (genesis_manifest, "genesis_manifest", False),
    (genesis_snapshot, "genesis_snapshot", False), (source_log, "source_log", False),
    (ca_file, "ca_file", False), (tls_cert, "tls_cert", False), (tls_key, "tls_key", False),
):
    absolute_path(value, name, required)

def origin(value, name, allowed_paths=("", "/")):
    try:
        parsed = urllib.parse.urlsplit(value)
        host = parsed.hostname
        port = parsed.port
    except ValueError as error:
        raise SystemExit(f"{name} is malformed") from error
    if (parsed.scheme not in {"https", "http"} or host is None
            or parsed.username is not None or parsed.password is not None
            or parsed.query or parsed.fragment or parsed.path not in allowed_paths):
        raise SystemExit(f"{name} must be a credential-free endpoint with a supported path")
    if port is not None and not 1 <= port <= 65535:
        raise SystemExit(f"{name} port is out of range")
    if parsed.scheme == "http":
        try:
            address = ipaddress.ip_address(host)
        except ValueError as error:
            raise SystemExit(f"{name} HTTP host must be a loopback literal") from error
        if not allow_loopback or not address.is_loopback:
            raise SystemExit(f"{name} HTTP is allowed only for explicit loopback development")

for index, value in enumerate([public_url, *upstreams, *peer_seeds]):
    origin(value, f"origin[{index}]")
for index, value in enumerate(submission_upstreams):
    origin(value, f"submission_upstream[{index}]", ("", "/", "/rpc", "/v1/activities"))

try:
    parsed_network_id = int(network_id)
except ValueError as error:
    raise SystemExit("network_id must be an integer") from error
if not 1 <= parsed_network_id <= 0xFFFFFFFF:
    raise SystemExit("network_id is out of range")
for value, name in (
    (genesis_sha256, "genesis_sha256"), (sequencer_id, "sequencer_id"),
    (sequencer_public_key, "sequencer_public_key"),
):
    if re.fullmatch(r"[0-9a-f]{64}", value) is None or value == "0" * 64:
        raise SystemExit(f"{name} is not a non-zero lowercase hexadecimal value")
if re.fullmatch(r"(?:127\.0\.0\.1|\[::1\]):[1-9][0-9]{0,4}", listen) is None:
    if not tls_cert or not tls_key:
        raise SystemExit("non-loopback listen requires --tls-cert and --tls-key")

document = {
    "network_id": parsed_network_id,
    "genesis_sha256": genesis_sha256,
    "sequencer_id": sequencer_id,
    "sequencer_public_key": sequencer_public_key,
    "data_dir": data_dir,
    "listen": listen,
    "public_url": public_url.rstrip("/"),
    "codec": codec,
    "upstreams": [value.rstrip("/") for value in upstreams],
    "submission_upstreams": [value.rstrip("/") for value in submission_upstreams],
    "allow_loopback_dev": allow_loopback,
    "peer_discovery": {
        "enabled": bool(peer_seeds),
        "seeds": [value.rstrip("/") for value in peer_seeds],
        "advertise_ttl_seconds": 300,
        "refresh_interval_seconds": 60,
        "max_peers": 64,
        "max_advertised_peers": 32,
        "allow_loopback_dev": allow_loopback,
    },
}
for name, value in (
    ("genesis_manifest", genesis_manifest), ("genesis_snapshot", genesis_snapshot),
    ("source_log", source_log), ("ca_file", ca_file),
    ("tls_cert", tls_cert), ("tls_key", tls_key),
):
    if value:
        document[name] = value

parent = pathlib.Path(destination).parent
descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(descriptor, "w", encoding="utf-8") as output:
    json.dump(document, output, sort_keys=True, separators=(",", ":"))
    output.write("\n")
PY
    CONFIG_CREATED=1
fi

if [ "$NO_SERVICE" -eq 0 ]; then
    [ "$(id -u)" -eq 0 ] || fail "service installation requires root; use --no-service"
    [ "$PREFIX" = "$DEFAULT_PREFIX" ] || fail "systemd installation requires prefix $DEFAULT_PREFIX"
    [ "$CONFIG" = "$DEFAULT_CONFIG" ] || fail "systemd installation requires config $DEFAULT_CONFIG"
    if ! getent group layerx-relay-archive >/dev/null; then
        groupadd --system layerx-relay-archive
    fi
    if ! getent passwd layerx-relay-archive >/dev/null; then
        useradd --system --gid layerx-relay-archive --home-dir /nonexistent \
            --shell /usr/sbin/nologin layerx-relay-archive
    fi
    DATA_DIR=$(python3 - "$CONFIG" <<'PY'
import json
import pathlib
import sys

with pathlib.Path(sys.argv[1]).open("r", encoding="utf-8") as source:
    value = json.load(source).get("data_dir")
if not isinstance(value, str) or value != "/var/lib/layerx/relay-archive":
    raise SystemExit("systemd config data_dir must be /var/lib/layerx/relay-archive")
print(value)
PY
    )
    if [ ! -d "$DATA_DIR" ]; then
        install -d -m 0750 -o layerx-relay-archive -g layerx-relay-archive "$DATA_DIR"
    else
        [ ! -L "$DATA_DIR" ] || fail "data directory must not be a symlink"
        DATA_OWNER=$(stat -c '%U:%G' "$DATA_DIR")
        [ "$DATA_OWNER" = "layerx-relay-archive:layerx-relay-archive" ] \
            || fail "existing data directory must be owned by layerx-relay-archive"
    fi
    if [ "$CONFIG_CREATED" -eq 1 ]; then
        chown root:layerx-relay-archive "$CONFIG_PARENT"
        chmod 0750 "$CONFIG_PARENT"
        chown root:layerx-relay-archive "$CONFIG"
        chmod 0640 "$CONFIG"
    fi
    install -m 0644 "$RELEASE_DIR/layerx-relay-archive.service" \
        /etc/systemd/system/layerx-relay-archive.service
    systemctl daemon-reload
    systemctl enable --now layerx-relay-archive.service
fi

printf 'installed LayerX relay/archive release %s under %s\n' "$ACTUAL_MANIFEST" "$PREFIX"
if [ "$NO_SERVICE" -eq 1 ]; then
    printf 'run with: LAYERX_RELAY_ARCHIVE_RUNTIME=%s/runtime.py %s/layerxd --relay-archive %s\n' \
        "$PREFIX" "$PREFIX" "$CONFIG"
fi
