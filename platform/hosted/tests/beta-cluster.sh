#!/usr/bin/env bash
# LayerX beta cluster bring-up and teardown.
#
# Usage:
#   beta-cluster.sh up [--boundary-checks]
#   beta-cluster.sh down
#   beta-cluster.sh render
#
# Inputs (environment variables, all optional unless stated):
#   LAYERX_BETA_KUBECONFIG              owner cluster kubeconfig; unset selects a disposable local kind cluster
#   LAYERX_BETA_CLUSTER_NAME            kind cluster name (default layerx-beta)
#   LAYERX_BETA_IMAGE_REGISTRY          registry prefix images are pushed to for an owner cluster; unset loads
#                                       the locally built images into the kind nodes
#   LAYERX_BETA_BUILDER_ENVIRONMENT_DIR owner hermetic builder root filesystem (regular files and directories only,
#                                       entrypoint bin/layerx-build) published into the registry builder release PVC
#   LAYERX_BETA_SEQUENCER_KEY_FILE      PEM ed25519 private key of the beta sequencer; generated when unset. Its
#                                       32-byte seed is the node's sequencer key, so the node, the receipt
#                                       authority, the gateway and the registry share one sequencer identity
#   LAYERX_BETA_FOUNDRY_BIN             directory holding the pinned forge and cast that
#                                       platform/hosted/paxeer/deploy-contracts.sh requires (default /root/.foundry/bin)
#   LAYERX_BETA_FAUCET_HOST             public faucet hostname (default faucet.testnet.layerx.network)
#   LAYERX_BETA_DEVELOPER_HOST          public developer hostname (default developers.testnet.layerx.network)
#   LAYERX_BETA_KIND_CNI                calico (default, enforces NetworkPolicy) or kindnet
#   LAYERX_BETA_READY_TIMEOUT           seconds to wait for every journey to report ready (default 900)
#   LAYERX_BETA_MIN_FREE_GIB            free disk required before building images (default 24)
#   LAYERX_BETA_TESTNET_PORT            host ports of the testnet, gateway and faucet port-forwards
#   LAYERX_BETA_GATEWAY_PORT            (defaults 19443, 19444, 19445)
#   LAYERX_BETA_FAUCET_PORT
#   LAYERX_BETA_TEST_AUTH_TOKEN_FILE    identity session token for the smoke source; when unset the bring-up
#                                       provisions a principal for the source DID in the identity service with
#                                       a generated ed25519 signer key and mints its session
#   LAYERX_BETA_TEST_SOURCE_DID         smoke source DID (default did:layerx:beta:<random>)
#   LAYERX_BETA_TEST_DESTINATION_DID    smoke destination DID (default did:layerx:beta:<random>, provisioned too)
#   LAYERX_BETA_TEST_AMOUNT             smoke move amount in the node asset (default 1)
#   LAYERX_BETA_QUALIFICATION_NODE_URL  overrides for the qualification runner component URLs
#   LAYERX_BETA_QUALIFICATION_AGENT_URL
#   LAYERX_BETA_QUALIFICATION_HUMAN_URL
#   LAYERX_BETA_QUALIFICATION_PAXEER_URL
#   LAYERX_BETA_FORBIDDEN_CHAIN_ID      refuse contract transactions on this chain id, including disposable clusters
#   LAYERX_BETA_KEEP_TOOLS              set to 1 to keep the pinned kind/kubectl downloads on teardown
#
# The trusted-boundary services (node with core boundary, receipt authority and agent boundary; identity;
# Paxeer chain with its boundary) are built from the repository, applied before the testnet, gateway,
# registry and developer manifests, and bound together in this order: the Paxeer chain starts with the
# generated deployer address, the node bootstraps its genesis, deploy-contracts.sh deploys the settlement
# contracts from the node's genesis artifacts through the Paxeer boundary, and the resulting GuarantorBond
# and CheckpointRegistry addresses are published to the node as the layerx-node-settlement ConfigMap the
# sequencer supervisor waits for before starting layerxd --serve.
#
# Boundary checks (--boundary-checks) additionally read the inputs of
# platform/hosted/gateway/tests/hosted-boundary.sh and platform/hosted/webhooks/tests/fault-injection.sh.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../../.." && pwd)
WORK_DIR="$REPO_ROOT/build/beta-cluster"
TOOLS_DIR="$REPO_ROOT/build/bin"
CA_DIR="$WORK_DIR/ca"
SECRETS_DIR="$WORK_DIR/secrets"
MANIFESTS_DIR="$WORK_DIR/manifests"
LOG_DIR="$WORK_DIR/logs"
ENV_FILE="$WORK_DIR/env"
IDENTITY_FILE="$WORK_DIR/identity"
STATE_FILE="$WORK_DIR/state"

KIND_VERSION=v0.30.0
KIND_SHA256=517ab7fc89ddeed5fa65abf71530d90648d9638ef0c4cde22c2c11f8097b8889
KUBECTL_VERSION=v1.34.1
KUBECTL_SHA256=7721f265e18709862655affba5343e85e1980639395d5754473dafaadcaa69e3
KIND_NODE_IMAGE=kindest/node:v1.34.0@sha256:7416a61b42b1662ca6ca89f02028ac133a309a2a30ba309614e8ec94d976dc5a
CALICO_VERSION=v3.30.3
CALICO_SHA256=9382d2b27a76f40c170454b408653e6d71e2205ef0aef069e942bb690e7381d0

CLUSTER_NAME=${LAYERX_BETA_CLUSTER_NAME:-layerx-beta}
FAUCET_HOST=${LAYERX_BETA_FAUCET_HOST:-faucet.testnet.layerx.network}
DEVELOPER_HOST=${LAYERX_BETA_DEVELOPER_HOST:-developers.testnet.layerx.network}
TESTNET_HOST=testnet.layerx.network
GATEWAY_HOST=api.testnet.layerx.network
KIND_CNI=${LAYERX_BETA_KIND_CNI:-calico}
READY_TIMEOUT=${LAYERX_BETA_READY_TIMEOUT:-900}
MIN_FREE_GIB=${LAYERX_BETA_MIN_FREE_GIB:-24}
TESTNET_PORT=${LAYERX_BETA_TESTNET_PORT:-19443}
GATEWAY_PORT=${LAYERX_BETA_GATEWAY_PORT:-19444}
FAUCET_PORT=${LAYERX_BETA_FAUCET_PORT:-19445}
TESTNET_NAMESPACE=layerx-testnet
DEVELOPER_NAMESPACE=layerx-developer
IMAGE_LABEL=io.layerx.beta-cluster
BOUNDARY_LABEL=layerx.io/program-registry-boundary

IMAGE_NAMES=(layerx-testnet-control layerx-gateway layerx-faucet layerx-program-registry layerx-webhooks layerx-dashboard layerx-dashboard-web
    layerx-internal layerx-human layerx-node layerx-core-boundary layerx-receipt-authority layerx-agent-boundary layerx-identity layerx-paxeer-boundary paxd-node paxd)
    layerx-internal layerx-node layerx-core-boundary layerx-receipt-authority layerx-agent-boundary layerx-identity layerx-paxeer-boundary paxd-node paxd)
TRUSTED_BOUNDARY_SERVICES=(layerx-pending-core layerx-pending-core-admin paxeer-boundary layerx-identity layerx-receipt-authority layerx-agent-boundary)
INTERNAL_NAMESPACE=layerx-internal
FOUNDRY_BIN=${LAYERX_BETA_FOUNDRY_BIN:-/root/.foundry/bin}
CUSTODY_PROFILE=${LAYERX_BETA_CUSTODY_PROFILE:-}
IDENTITY_PORT=19451
PAXEER_CHAIN_ID=125
NODE_MANIFEST="$REPO_ROOT/platform/hosted/node/deployment.yaml"
NODE_NETWORK_ID=$(sed -n 's/^  network-id: "\([0-9]*\)"$/\1/p' "$NODE_MANIFEST")
NODE_ASSET_ID=$(sed -n 's/^  asset-id: "\([0-9a-f]*\)"$/\1/p' "$NODE_MANIFEST")
PAXEER_RELAY_PORT=$(sed -n 's/^  paxeer-relay-port: "\([0-9]*\)"$/\1/p' "$NODE_MANIFEST")

log() { printf 'beta-cluster: %s\n' "$*" >&2; }
fail() { printf 'beta-cluster: error: %s\n' "$*" >&2; exit 1; }

require_tool() {
    local tool
    for tool in "$@"; do
        command -v "$tool" >/dev/null 2>&1 || fail "required host tool '$tool' is not installed"
    done
}

image_source() {
    case "$1" in
        layerx-testnet-control) printf 'ghcr.io/sidiora-labs/layerx-testnet-control:0.1.0 platform/hosted/testnet/Dockerfile' ;;
        layerx-gateway) printf 'ghcr.io/sidiora-labs/layerx-gateway:0.1.0 platform/hosted/gateway/Dockerfile' ;;
        layerx-faucet) printf 'ghcr.io/sidiora-labs/layerx-faucet:0.1.0 platform/hosted/faucet/Dockerfile' ;;
        layerx-program-registry) printf 'ghcr.io/sidiora-labs/layerx-program-registry:0.1.0 platform/hosted/registry/Dockerfile' ;;
        layerx-webhooks) printf 'ghcr.io/sidiora-labs/layerx-webhooks:0.1.0 platform/hosted/webhooks/Dockerfile' ;;
        layerx-dashboard) printf 'ghcr.io/sidiora-labs/layerx-dashboard:0.1.0 platform/hosted/dashboard/Dockerfile' ;;
        layerx-dashboard-web) printf 'ghcr.io/sidiora-labs/layerx-dashboard-web:0.1.0 platform/hosted/dashboard/web/Dockerfile' ;;
        layerx-internal) printf 'ghcr.io/sidiora-labs/layerx-internal:0.1.0 platform/hosted/internal/Dockerfile' ;;
        layerx-human) printf 'ghcr.io/sidiora-labs/layerx-human:0.1.0 platform/hosted/human/Dockerfile' ;;
        layerx-node) printf 'ghcr.io/sidiora-labs/layerx-node:0.1.0 platform/hosted/node/Dockerfile' ;;
        layerx-core-boundary) printf 'ghcr.io/sidiora-labs/layerx-core-boundary:0.1.0 platform/hosted/core/Dockerfile' ;;
        layerx-receipt-authority) printf 'ghcr.io/sidiora-labs/layerx-receipt-authority:0.1.0 platform/hosted/authority/Dockerfile' ;;
        layerx-agent-boundary) printf 'ghcr.io/sidiora-labs/layerx-agent-boundary:0.1.0 platform/hosted/agent-boundary/Dockerfile' ;;
        layerx-identity) printf 'ghcr.io/sidiora-labs/layerx-identity:0.1.0 platform/hosted/identity/Dockerfile' ;;
        layerx-paxeer-boundary) printf 'ghcr.io/sidiora-labs/layerx-paxeer-boundary:0.1.0 platform/hosted/paxeer/Dockerfile' ;;
        paxd-node) printf 'ghcr.io/sidiora-labs/paxd-node:0.1.0 platform/hosted/paxeer/Dockerfile.paxd-node' ;;
        paxd) printf 'ghcr.io/sidiora-labs/paxd:0.1.0 platform/hosted/paxeer/Dockerfile.paxd' ;;
        *) fail "unknown image $1" ;;
    esac
}

revision() {
    local rev
    rev=$(git -C "$REPO_ROOT" rev-parse --short=12 HEAD)
    if [ -n "$(git -C "$REPO_ROOT" status --porcelain --untracked-files=no -- platform)" ]; then
        rev="$rev-dirty"
    fi
    printf '%s' "$rev"
}

image_ref() {
    printf '%s/%s:%s' "${LAYERX_BETA_IMAGE_REGISTRY:-layerx-beta}" "$1" "$REVISION"
}

cluster_mode() {
    if [ -n "${LAYERX_BETA_KUBECONFIG:-}" ]; then printf 'owner'; else printf 'kind'; fi
}

kube() {
    "$TOOLS_DIR/kubectl" --kubeconfig "$KUBECONFIG_FILE" "$@"
}

state_get() {
    [ -f "$STATE_FILE" ] || return 1
    sed -n "s/^$1=//p" "$STATE_FILE" | tail -n 1
}

state_set() {
    mkdir -p "$WORK_DIR"
    touch "$STATE_FILE"
    grep -v "^$1=" "$STATE_FILE" > "$STATE_FILE.next" || true
    printf '%s=%s\n' "$1" "$2" >> "$STATE_FILE.next"
    mv "$STATE_FILE.next" "$STATE_FILE"
}

free_gib() {
    local dir=$1
    while [ ! -d "$dir" ]; do dir=$(dirname "$dir"); done
    df -Pk "$dir" | awk 'NR == 2 { printf "%d", $4 / 1048576 }'
}

preflight_disk() {
    local docker_root free
    docker_root=$(docker info --format '{{.DockerRootDir}}')
    for dir in "$REPO_ROOT/build" "$docker_root"; do
        free=$(free_gib "$dir")
        if [ "$free" -lt "$MIN_FREE_GIB" ]; then
            fail "insufficient free disk under $dir: ${free} GiB free, ${MIN_FREE_GIB} GiB required (LAYERX_BETA_MIN_FREE_GIB) to build the beta images and run a local cluster"
        fi
    done
}

fetch_pinned() {
    local name=$1 url=$2 sha=$3 dest=$4 actual
    if [ -x "$dest" ] && printf '%s  %s\n' "$sha" "$dest" | sha256sum --check --status; then
        return 0
    fi
    log "downloading pinned $name from $url"
    curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --output "$dest.part" "$url" \
        || fail "download of pinned $name failed: $url"
    actual=$(sha256sum "$dest.part" | cut -d ' ' -f 1)
    if [ "$actual" != "$sha" ]; then
        rm -f "$dest.part"
        fail "pinned $name sha256 mismatch: expected $sha, downloaded $actual"
    fi
    chmod 0755 "$dest.part"
    mv "$dest.part" "$dest"
}

tools_install() {
    mkdir -p "$TOOLS_DIR"
    fetch_pinned kubectl "https://dl.k8s.io/release/$KUBECTL_VERSION/bin/linux/amd64/kubectl" "$KUBECTL_SHA256" "$TOOLS_DIR/kubectl"
    if [ "$(cluster_mode)" = kind ]; then
        fetch_pinned kind "https://github.com/kubernetes-sigs/kind/releases/download/$KIND_VERSION/kind-linux-amd64" "$KIND_SHA256" "$TOOLS_DIR/kind"
        if [ "$KIND_CNI" = calico ]; then
            fetch_pinned calico-manifest "https://raw.githubusercontent.com/projectcalico/calico/$CALICO_VERSION/manifests/calico.yaml" "$CALICO_SHA256" "$TOOLS_DIR/calico-$CALICO_VERSION.yaml"
        fi
    fi
}

build_context() {
    log "packing the build context from the tracked source files of $REPO_ROOT"
    (cd "$REPO_ROOT" && git ls-files -z --cached \
        | while IFS= read -r -d '' path; do
            case "/$path" in */.env|*/.env.*|/qual-logs/*|/NEEDS.md|/STATUS.md) continue ;; esac
            [ -e "$path" ] && printf '%s\0' "$path"
        done \
        | tar --null --files-from - -cf "$WORK_DIR/context.tar")
}

image_build_args() {
    case "$1" in
        layerx-node) printf -- '--build-arg LXP_REVISION=%s' "$REVISION" ;;
        paxd-node) printf -- '--build-arg PAX_CHAIN_REF=%s' "$REVISION" ;;
        paxd) printf -- '--build-arg PAXD_IMAGE=%s' "$(image_ref paxd-node)" ;;
        *) ;;
    esac
}

build_images() {
    local name canonical dockerfile ref id
    local -a build_args
    mkdir -p "$LOG_DIR"
    : > "$WORK_DIR/images"
    build_context
    for name in "${IMAGE_NAMES[@]}"; do
        read -r canonical dockerfile <<<"$(image_source "$name")"
        ref=$(image_ref "$name")
        read -r -a build_args <<<"$(image_build_args "$name")"
        log "building $ref from $dockerfile"
        docker build --file "$dockerfile" --tag "$ref" --label "$IMAGE_LABEL=$CLUSTER_NAME" "${build_args[@]}" - < "$WORK_DIR/context.tar" \
            > "$LOG_DIR/build-$name.log" 2>&1 || { tail -n 40 "$LOG_DIR/build-$name.log" >&2; fail "image build failed for $name (log $LOG_DIR/build-$name.log)"; }
        id=$(docker image inspect --format '{{.Id}}' "$ref")
        printf '%s %s %s %s\n' "$name" "$canonical" "$ref" "$id" >> "$WORK_DIR/images"
    done
}

kind_nodes() {
    docker ps --filter "label=io.x-k8s.kind.cluster=$CLUSTER_NAME" --format '{{.Names}}'
}

cluster_create() {
    mkdir -p "$WORK_DIR"
    if [ "$(cluster_mode)" = owner ]; then
        KUBECONFIG_FILE=$LAYERX_BETA_KUBECONFIG
        [ -r "$KUBECONFIG_FILE" ] || fail "LAYERX_BETA_KUBECONFIG=$KUBECONFIG_FILE is not readable"
        state_set mode owner
        return 0
    fi
    KUBECONFIG_FILE="$WORK_DIR/kubeconfig"
    state_set mode kind
    if "$TOOLS_DIR/kind" get clusters 2>/dev/null | grep -qx "$CLUSTER_NAME"; then
        log "kind cluster $CLUSTER_NAME already exists; reusing it"
        "$TOOLS_DIR/kind" export kubeconfig --name "$CLUSTER_NAME" --kubeconfig "$KUBECONFIG_FILE"
        return 0
    fi
    {
        printf 'kind: Cluster\napiVersion: kind.x-k8s.io/v1alpha4\n'
        printf 'name: %s\n' "$CLUSTER_NAME"
        if [ "$KIND_CNI" = calico ]; then printf 'networking:\n  disableDefaultCNI: true\n  podSubnet: 192.168.0.0/16\n'; fi
        printf 'nodes:\n  - role: control-plane\n    image: %s\n  - role: worker\n    image: %s\n' "$KIND_NODE_IMAGE" "$KIND_NODE_IMAGE"
    } > "$WORK_DIR/kind-config.yaml"
    log "creating kind cluster $CLUSTER_NAME"
    "$TOOLS_DIR/kind" create cluster --config "$WORK_DIR/kind-config.yaml" --kubeconfig "$KUBECONFIG_FILE" --wait 120s
    if [ "$KIND_CNI" = calico ]; then
        kube apply -f "$TOOLS_DIR/calico-$CALICO_VERSION.yaml" > /dev/null
        kube -n kube-system rollout status daemonset/calico-node --timeout=300s
    fi
    kube wait --for=condition=Ready nodes --all --timeout=300s
}

load_images() {
    local name canonical ref id
    while read -r name canonical ref id; do
        if [ "$(cluster_mode)" = owner ]; then
            [ -n "${LAYERX_BETA_IMAGE_REGISTRY:-}" ] || fail "LAYERX_BETA_IMAGE_REGISTRY is required to push images for an owner cluster"
            log "pushing $ref"
            docker push "$ref" > "$LOG_DIR/push-$name.log" 2>&1 || fail "docker push failed for $ref"
        else
            log "loading $ref into kind nodes"
            "$TOOLS_DIR/kind" load docker-image --name "$CLUSTER_NAME" "$ref" > "$LOG_DIR/load-$name.log" 2>&1 || fail "kind load failed for $ref"
        fi
    done < "$WORK_DIR/images"
}

node_boundary_install() {
    local node script unit
    script="$REPO_ROOT/platform/hosted/registry/node-provision-build-boundary.sh"
    unit="$REPO_ROOT/platform/hosted/registry/layerx-program-registry-boundary.service"
    if [ "$(cluster_mode)" = owner ]; then
        if [ -z "$(kube get nodes -l "$BOUNDARY_LABEL=v1" -o name)" ]; then
            fail "no node of the owner cluster carries $BOUNDARY_LABEL=v1; the owner installs $unit and $script on the registry nodes before labelling them"
        fi
        return 0
    fi
    for node in $(kind_nodes); do
        case "$node" in *control-plane*) continue ;; esac
        log "installing registry node boundary on $node"
        docker exec "$node" sh -c 'for tool in losetup mkfs.ext4 e2fsck mountpoint findmnt flock; do command -v "$tool" >/dev/null 2>&1 || exit 1; done' || {
            docker exec "$node" sh -c 'apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq e2fsprogs util-linux >/dev/null' \
                || fail "kind node $node lacks losetup/mkfs.ext4/e2fsck/mountpoint/findmnt/flock and they could not be installed"
        }
        docker exec "$node" mkdir -p /usr/libexec/layerx /var/lib/layerx-program-registry-builds
        docker cp "$script" "$node:/usr/libexec/layerx/node-provision-build-boundary.sh"
        docker cp "$unit" "$node:/etc/systemd/system/layerx-program-registry-boundary.service"
        docker exec "$node" chmod 0755 /usr/libexec/layerx/node-provision-build-boundary.sh
        docker exec "$node" systemctl daemon-reload
        docker exec "$node" systemctl enable --now layerx-program-registry-boundary.service > /dev/null 2>&1 \
            || { docker exec "$node" systemctl status --no-pager layerx-program-registry-boundary.service >&2 || true; fail "registry node boundary provisioning failed on $node"; }
        docker exec "$node" systemctl is-active --quiet layerx-program-registry-boundary.service || fail "registry node boundary unit is not active on $node"
        docker exec "$node" test -d /sys/fs/cgroup/layerx-program-registry
        kube label node "$node" "$BOUNDARY_LABEL=v1" --overwrite > /dev/null
    done
}

random_hex() { openssl rand -hex "$1"; }

write_token() {
    local path=$1
    (umask 077; printf '%s' "$(random_hex 32)" > "$path")
}

component_secrets_generate() {
    local directory=$1
    mkdir -p "$directory"
    write_token "$directory/gateway-component.token"
    write_token "$directory/registry-node.token"
    write_token "$directory/webhook-component.token"
}

issue_cert() {
    local name=$1 cn=$2 usage=$3 subject_alt=$4 dir
    dir="$CA_DIR/$name"
    mkdir -p "$dir"
    (umask 077; openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$dir/key.pem" 2>/dev/null)
    openssl req -new -key "$dir/key.pem" -subj "/O=LayerX beta/CN=$cn" -out "$dir/csr.pem" 2>/dev/null
    {
        printf 'basicConstraints=CA:FALSE\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=%s\n' "$usage"
        [ -n "$subject_alt" ] && printf 'subjectAltName=%s\n' "$subject_alt"
    } > "$dir/ext.cnf"
    openssl x509 -req -in "$dir/csr.pem" -CA "$CA_DIR/ca.crt" -CAkey "$CA_DIR/ca.key" -CAcreateserial \
        -days 30 -sha256 -extfile "$dir/ext.cnf" -out "$dir/cert.pem" 2>/dev/null
    openssl x509 -in "$dir/cert.pem" -outform DER -out "$dir/cert.der"
    (umask 077; openssl pkcs8 -topk8 -nocrypt -in "$dir/key.pem" -outform DER -out "$dir/key.der")
}

issue_client_identity() {
    local name=$1 cn=$2 dir
    dir="$CA_DIR/$name"
    issue_cert "$name" "$cn" clientAuth ""
    write_token "$dir/password"
    (umask 077; openssl pkcs12 -export -inkey "$dir/key.pem" -in "$dir/cert.pem" -certfile "$CA_DIR/ca.crt" \
        -name "$cn" -passout "file:$dir/password" -out "$dir/client.p12")
}

ca_generate() {
    rm -rf "$CA_DIR" "$SECRETS_DIR"
    mkdir -p "$CA_DIR" "$SECRETS_DIR"
    chmod 0700 "$CA_DIR" "$SECRETS_DIR"
    (umask 077; openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$CA_DIR/ca.key" 2>/dev/null)
    openssl req -x509 -new -key "$CA_DIR/ca.key" -days 30 -sha256 -subj "/O=LayerX beta/CN=LayerX beta internal CA" \
        -addext 'basicConstraints=critical,CA:TRUE,pathlen:0' -addext 'keyUsage=critical,keyCertSign,cRLSign' -out "$CA_DIR/ca.crt" 2>/dev/null
    openssl x509 -in "$CA_DIR/ca.crt" -outform DER -out "$CA_DIR/ca.der"
    local svc="$TESTNET_NAMESPACE.svc.cluster.local" dev="$DEVELOPER_NAMESPACE.svc.cluster.local"
    issue_cert testnet-control layerx-testnet-control serverAuth \
        "DNS:layerx-testnet-public.$svc,DNS:layerx-testnet-admin.$svc,DNS:layerx-testnet-public,DNS:layerx-testnet-admin,DNS:$TESTNET_HOST,DNS:localhost,IP:127.0.0.1"
    issue_cert gateway layerx-gateway serverAuth \
        "DNS:layerx-gateway.$svc,DNS:layerx-gateway.$TESTNET_NAMESPACE.svc,DNS:layerx-gateway,DNS:$GATEWAY_HOST,DNS:localhost,IP:127.0.0.1"
    issue_cert human layerx-human serverAuth \
        "DNS:layerx-human.$svc,DNS:layerx-human.$TESTNET_NAMESPACE.svc,DNS:layerx-human,DNS:human.testnet.layerx.network,DNS:localhost,IP:127.0.0.1"
    issue_cert faucet layerx-faucet serverAuth \
        "DNS:layerx-faucet-public.$svc,DNS:layerx-faucet-public,DNS:$FAUCET_HOST,DNS:localhost,IP:127.0.0.1"
    issue_cert registry layerx-program-registry serverAuth \
        "DNS:layerx-program-registry.$svc,DNS:layerx-program-registry"
    issue_cert faucet-redis layerx-faucet-redis serverAuth "DNS:layerx-faucet-redis.$svc,DNS:layerx-faucet-redis"
    issue_cert gateway-redis layerx-gateway-redis serverAuth "DNS:layerx-gateway-redis.$svc,DNS:layerx-gateway-redis"
    issue_cert developer layerx-developer serverAuth \
        "DNS:layerx-webhooks.$dev,DNS:layerx-dashboard-api.$dev,DNS:layerx-webhooks,DNS:layerx-dashboard-api,DNS:$DEVELOPER_HOST,DNS:localhost,IP:127.0.0.1"
    local internal="$INTERNAL_NAMESPACE.svc.cluster.local"
    local service
    issue_cert internal-redis redis serverAuth "DNS:redis.$internal,DNS:redis.$INTERNAL_NAMESPACE.svc,DNS:redis"
    for service in kms journeys payments approvals programs; do
        issue_cert "internal-$service" "$service" serverAuth "DNS:$service.$internal,DNS:$service.$INTERNAL_NAMESPACE.svc,DNS:$service"
    done
    issue_cert pending-core layerx-pending-core serverAuth \
        "DNS:layerx-pending-core.$svc,DNS:layerx-pending-core.$TESTNET_NAMESPACE.svc,DNS:layerx-pending-core,DNS:localhost,IP:127.0.0.1"
    issue_cert pending-core-admin layerx-pending-core-admin serverAuth \
        "DNS:layerx-pending-core-admin.$svc,DNS:layerx-pending-core-admin.$TESTNET_NAMESPACE.svc,DNS:layerx-pending-core-admin"
    issue_cert receipt-authority layerx-receipt-authority serverAuth \
        "DNS:layerx-receipt-authority.$svc,DNS:layerx-receipt-authority.$TESTNET_NAMESPACE.svc,DNS:layerx-receipt-authority,DNS:authority.$internal,DNS:authority.$INTERNAL_NAMESPACE.svc"
    issue_cert agent-boundary layerx-agent-boundary serverAuth \
        "DNS:layerx-agent-boundary.$svc,DNS:layerx-agent-boundary.$TESTNET_NAMESPACE.svc,DNS:layerx-agent-boundary,DNS:component.$internal,DNS:component.$INTERNAL_NAMESPACE.svc,DNS:localhost,IP:127.0.0.1"
    issue_cert identity layerx-identity serverAuth \
        "DNS:layerx-identity.$svc,DNS:layerx-identity.$TESTNET_NAMESPACE.svc,DNS:layerx-identity,DNS:identity.$internal,DNS:identity.$INTERNAL_NAMESPACE.svc,DNS:localhost,IP:127.0.0.1"
    issue_cert paxeer-boundary paxeer-boundary serverAuth \
        "DNS:paxeer-boundary.$svc,DNS:paxeer-boundary.$TESTNET_NAMESPACE.svc,DNS:paxeer-boundary,DNS:paxeer.$svc,DNS:localhost,IP:127.0.0.1"
    issue_client_identity gateway-client layerx-gateway
    issue_client_identity developer-client layerx-developer
    if [ -n "${LAYERX_BETA_SEQUENCER_KEY_FILE:-}" ]; then
        [ -r "$LAYERX_BETA_SEQUENCER_KEY_FILE" ] || fail "LAYERX_BETA_SEQUENCER_KEY_FILE=$LAYERX_BETA_SEQUENCER_KEY_FILE is not readable"
        (umask 077; cp "$LAYERX_BETA_SEQUENCER_KEY_FILE" "$CA_DIR/sequencer.key")
        SEQUENCER_KEY_SOURCE=LAYERX_BETA_SEQUENCER_KEY_FILE
    else
        (umask 077; openssl genpkey -algorithm ed25519 -out "$CA_DIR/sequencer.key" 2>/dev/null)
        SEQUENCER_KEY_SOURCE=generated
    fi
    openssl pkey -in "$CA_DIR/sequencer.key" -pubout -outform DER 2>/dev/null | tail -c 32 | od -An -v -tx1 | tr -d ' \n' > "$CA_DIR/sequencer.pub.hex"
    [ "$(wc -c < "$CA_DIR/sequencer.pub.hex")" -eq 64 ] || fail "sequencer key is not an ed25519 key"
    (umask 077; openssl pkey -in "$CA_DIR/sequencer.key" -outform DER 2>/dev/null | tail -c 32 | od -An -v -tx1 | tr -d ' \n' > "$CA_DIR/sequencer.seed.hex")
    [ "$(wc -c < "$CA_DIR/sequencer.seed.hex")" -eq 64 ] || fail "sequencer key seed is not 32 bytes"
    SEQUENCER_ID=$(printf 'layerx-sequencer:%s' "$(cat "$CA_DIR/sequencer.pub.hex")" | sha256sum | cut -d ' ' -f 1)
}

ed25519_public_hex() {
    openssl pkey -in "$1" -pubout -outform DER 2>/dev/null | tail -c 32 | od -An -v -tx1 | tr -d ' \n'
}

evm_key_generate() {
    # evm_key_generate NAME -> SECRETS_DIR/NAME.key (0x-prefixed secp256k1 secret) and SECRETS_DIR/NAME.address
    local name=$1 key address
    while :; do
        key=0x$(random_hex 32)
        address=$("$FOUNDRY_BIN/cast" wallet address --private-key "$key" 2>/dev/null) || continue
        [[ $address =~ ^0x[0-9a-fA-F]{40}$ ]] && break
    done
    (umask 077; printf '%s' "$key" > "$SECRETS_DIR/$name.key")
    printf '%s' "$address" > "$SECRETS_DIR/$name.address"
}

encode_trust_history() {
    python3 - "$1" "$2" "$3" "$NODE_NETWORK_ID" <<'PY'
import struct, sys
out, sequencer_id, public_key = sys.argv[1], bytes.fromhex(sys.argv[2]), bytes.fromhex(sys.argv[3])
entry = struct.pack(">HIQ", 2, int(sys.argv[4]), 1) + sequencer_id + public_key + struct.pack(">QQBQ", 1, 1 << 40, 0, 0)
assert len(entry) == 103
payload = b"LayerX/sequencer-trust-history/v1\0" + struct.pack(">HH", 1, 0) + entry
with open(out, "wb") as handle:
    handle.write(payload)
PY
}

environment_digest() {
    python3 - "$1" <<'PY'
import hashlib, os, struct, sys
root = sys.argv[1]
entries = []
for current, dirs, files in os.walk(root):
    dirs.sort()
    for name in dirs + files:
        full = os.path.join(current, name)
        rel = os.path.relpath(full, root)
        st = os.lstat(full)
        if os.path.islink(full) or not (os.path.isdir(full) or os.path.isfile(full)):
            sys.exit("builder environment contains a non-regular entry: %s" % rel)
        entries.append((tuple(rel.split(os.sep)), rel, os.path.isdir(full), 0 if os.path.isdir(full) else st.st_mode))
if len(entries) > 100_000:
    sys.exit("builder environment exceeds 100000 entries")
entries.sort()
digest = hashlib.sha256(b"LayerX/hosted-builder/environment/v1\0")
total = 0
for _, rel, is_dir, mode in entries:
    name = rel.encode()
    digest.update(struct.pack(">Q", len(name)) + name + bytes([1 if is_dir else 0]) + struct.pack(">I", mode & 0xFFFFFFFF))
    if is_dir:
        digest.update(struct.pack(">Q", 0))
        continue
    with open(os.path.join(root, rel), "rb") as handle:
        data = handle.read()
    total += len(data)
    if total > 4 << 30:
        sys.exit("builder environment exceeds 4 GiB")
    digest.update(struct.pack(">Q", len(data)) + data)
print(digest.hexdigest())
PY
}

write_redis_acl() {
    local acl=$1 user=$2 password=$3
    (umask 077; printf 'user default off\nuser %s on >%s ~* &* +@all\n' "$user" "$password" > "$acl")
}

secrets_generate() {
    local d="$SECRETS_DIR"
    mkdir -p "$d"
    write_token "$d/backend-admin.token"
    write_token "$d/control-admin.token"
    write_token "$d/identity-client.token"
    write_token "$d/status-publisher.token"
    component_secrets_generate "$d"
    write_token "$d/gateway-authority.token"
    write_token "$d/gateway-identity.token"
    write_token "$d/registry-request.token"
    write_token "$d/registry-publication.token"
    write_token "$d/registry-authority.token"
    write_token "$d/provisioning.key"
    write_token "$d/cursor.key"
    printf 'layerx-faucet' > "$d/faucet-redis.username"
    write_token "$d/faucet-redis.password"
    write_redis_acl "$d/faucet-redis.acl" layerx-faucet "$(cat "$d/faucet-redis.password")"
    printf 'layerx-gateway' > "$d/gateway-redis.username"
    write_token "$d/gateway-redis.password"
    write_redis_acl "$d/gateway-redis.acl" layerx-gateway "$(cat "$d/gateway-redis.password")"
    printf 'layerx-webhooks' > "$d/webhook-redis.username"
    write_token "$d/webhook-redis.password"
    printf 'layerx-dashboard' > "$d/dashboard-redis.username"
    write_token "$d/dashboard-redis.password"
    (umask 077; printf 'user layerx-dashboard on >%s ~gateway:* +ping +get +hmget +smembers +xrevrange\n' \
        "$(cat "$d/dashboard-redis.password")" >> "$d/gateway-redis.acl")
    (umask 077; printf 'user default off\nuser layerx-webhooks on >%s ~webhooks:* +ping +eval +hget +hmget +hset +hincrby +smembers +sadd\n' \
        "$(cat "$d/webhook-redis.password")" > "$d/internal-redis.acl")
    write_token "$d/internal-kms-seal.key"
    local token
    for token in kms identity authority journey payment approval program source-trigger operator; do
        write_token "$d/developer-$token.token"
    done
    cp "$CA_DIR/sequencer.pub.hex" "$d/sequencer-public-key"
    (umask 077; encode_trust_history "$d/trust-history" "$SEQUENCER_ID" "$(cat "$CA_DIR/sequencer.pub.hex")")
    python3 "$SCRIPT_DIR/sequencer-pins.py" "$d" "$WORK_DIR/sequencer-authorization.json"
    printf '%s' "$(random_hex 32)" > "$d/receipt-authority-replica-id"
    cp "$REPO_ROOT/interop/deploy/gateway/module-registry.example.json" "$d/module-registry.json"
    (umask 077; cp "$CA_DIR/sequencer.seed.hex" "$d/node-sequencer.key")
    (umask 077; random_hex 32 > "$d/node-treasury.key")
    write_token "$d/node-program.token"
    write_token "$d/node-replica.token"
    mkdir -p "$d/identity-tokens"
    chmod 0700 "$d/identity-tokens"
    cp "$d/gateway-identity.token" "$d/identity-tokens/gateway"
    cp "$d/developer-identity.token" "$d/identity-tokens/webhooks"
    cp "$d/identity-client.token" "$d/identity-tokens/faucet"
    local service
    for service in dashboard testnet ramp provisioning; do
        write_token "$d/identity-tokens/$service"
    done
    write_token "$d/identity-store.key"
    evm_key_generate paxeer-deployer
    evm_key_generate paxeer-final-proposer
    evm_key_generate paxeer-final-executor
    evm_key_generate paxeer-emergency-council
    evm_key_generate paxeer-guarantor-controller
    if [ -n "${LAYERX_BETA_TEST_AUTH_TOKEN_FILE:-}" ]; then
        [ -r "$LAYERX_BETA_TEST_AUTH_TOKEN_FILE" ] || fail "LAYERX_BETA_TEST_AUTH_TOKEN_FILE=$LAYERX_BETA_TEST_AUTH_TOKEN_FILE is not readable"
        (umask 077; cp "$LAYERX_BETA_TEST_AUTH_TOKEN_FILE" "$d/test-auth.token")
        TEST_AUTH_SOURCE=LAYERX_BETA_TEST_AUTH_TOKEN_FILE
    else
        TEST_AUTH_SOURCE=identity-provisioning
    fi
    (umask 077; openssl genpkey -algorithm ed25519 -out "$d/test-source-signer.key" 2>/dev/null)
    ed25519_public_hex "$d/test-source-signer.key" > "$d/test-source-signer.pub.hex"
    [ "$(wc -c < "$d/test-source-signer.pub.hex")" -eq 64 ] || fail "test source signer key is not an ed25519 key"
    (umask 077; openssl genpkey -algorithm ed25519 -out "$d/test-destination-signer.key" 2>/dev/null)
    ed25519_public_hex "$d/test-destination-signer.key" > "$d/test-destination-signer.pub.hex"
    TEST_SOURCE_DID=${LAYERX_BETA_TEST_SOURCE_DID:-did:layerx:beta:$(random_hex 16)}
    TEST_DESTINATION_DID=${LAYERX_BETA_TEST_DESTINATION_DID:-did:layerx:beta:$(random_hex 16)}
    source "$REPO_ROOT/platform/hosted/human/material.sh"
    human_secrets_generate
    [ "$TEST_SOURCE_DID" != "$TEST_DESTINATION_DID" ] || fail "the smoke source and destination DIDs must differ"
    TEST_AMOUNT=${LAYERX_BETA_TEST_AMOUNT:-1}
    [[ $TEST_AMOUNT =~ ^[1-9][0-9]*$ ]] || fail "LAYERX_BETA_TEST_AMOUNT must be a positive decimal"
}

apply_secret() {
    local namespace=$1 name=$2
    shift 2
    kube -n "$namespace" create secret generic "$name" "$@" --dry-run=client -o yaml | kube apply -f - > /dev/null
}

apply_configmap() {
    local namespace=$1 name=$2
    shift 2
    kube -n "$namespace" create configmap "$name" "$@" --dry-run=client -o yaml | kube apply -f - > /dev/null
}

apply_tls_secret() {
    local namespace=$1 name=$2 cert=$3
    kube -n "$namespace" create secret tls "$name" --cert="$CA_DIR/$cert/cert.pem" --key="$CA_DIR/$cert/key.pem" --dry-run=client -o yaml | kube apply -f - > /dev/null
}

secrets_apply() {
    local c="$CA_DIR" s="$SECRETS_DIR" ns="$TESTNET_NAMESPACE" dev="$DEVELOPER_NAMESPACE"
    kube create namespace "$ns" --dry-run=client -o yaml | kube apply -f - > /dev/null
    kube create namespace "$dev" --dry-run=client -o yaml | kube apply -f - > /dev/null
    kube create namespace "$INTERNAL_NAMESPACE" --dry-run=client -o yaml | kube apply -f - > /dev/null
    apply_secret "$INTERNAL_NAMESPACE" layerx-internal-redis-runtime \
        --from-file=server.pem="$c/internal-redis/cert.pem" --from-file=server.key="$c/internal-redis/key.pem" \
        --from-file=ca.pem="$c/ca.crt" --from-file=users.acl="$s/internal-redis.acl"
    apply_secret "$INTERNAL_NAMESPACE" layerx-internal-kms-runtime \
        --from-file=server.der="$c/internal-kms/cert.der" --from-file=server-key.der="$c/internal-kms/key.der" \
        --from-file=ca.der="$c/ca.der" --from-file=token="$s/developer-kms.token" --from-file=seal-secret="$s/internal-kms-seal.key"
    local service token
    printf '{}\n' > "$s/human-credentials.json"
    for service in journeys payments approvals programs; do
        token=${service%s}
        apply_secret "$INTERNAL_NAMESPACE" "layerx-internal-$service-runtime" \
            --from-file=server.der="$c/internal-$service/cert.der" --from-file=server-key.der="$c/internal-$service/key.der" \
            --from-file=ca.der="$c/ca.der" --from-file=upstream-ca.der="$c/ca.der" --from-file=token="$s/developer-$token.token" \
            --from-file=credentials.json="$s/human-credentials.json"
    done
    human_secrets_apply
    MISSING_INPUTS+=("Human production component configuration/material and local agentd, identity, security, movement and mTLS KMS providers; see platform/hosted/human/README.md")
    MISSING_INPUTS+=("Authenticated Human principal cookies for journeys/approvals require the passkey assertion and session.open ceremony; credential maps remain empty")
    apply_secret "$ns" layerx-human-tls --from-file=server.crt.der="$c/human/cert.der" \
        --from-file=server.key.der="$c/human/key.der" --from-file=ca.crt="$c/ca.crt"
    apply_secret "$ns" layerx-internal-ca --from-file=ca.crt.der="$c/ca.der" --from-file=ca.crt="$c/ca.crt"
    apply_secret "$ns" layerx-testnet-control-tls --from-file=server.crt.der="$c/testnet-control/cert.der" \
        --from-file=server.key.der="$c/testnet-control/key.der" --from-file=ca.crt.der="$c/ca.der" --from-file=ca.crt="$c/ca.crt"
    apply_secret "$ns" layerx-testnet-backend-admin --from-file=token="$s/backend-admin.token"
    apply_secret "$ns" layerx-testnet-control-admin --from-file=token="$s/control-admin.token"
    apply_secret "$ns" layerx-testnet-identity-client --from-file=token="$s/identity-client.token"
    apply_secret "$ns" layerx-testnet-status-publisher --from-file=token="$s/status-publisher.token"
    apply_secret "$ns" layerx-faucet-tls --from-file=server.crt.der="$c/faucet/cert.der" \
        --from-file=server.key.der="$c/faucet/key.der" --from-file=ca.crt.der="$c/ca.der"
    apply_secret "$ns" layerx-faucet-redis-tls --from-file=tls.crt="$c/faucet-redis/cert.pem" \
        --from-file=tls.key="$c/faucet-redis/key.pem" --from-file=ca.crt="$c/ca.crt"
    apply_secret "$ns" layerx-faucet-redis-auth --from-file=users.acl="$s/faucet-redis.acl"
    apply_secret "$ns" layerx-faucet-redis-client --from-file=username="$s/faucet-redis.username" --from-file=password="$s/faucet-redis.password"
    apply_secret "$ns" layerx-gateway-server-tls --from-file=server.crt.der="$c/gateway/cert.der" --from-file=server.key.der="$c/gateway/key.der"
    apply_secret "$ns" layerx-gateway-client-identity --from-file=client.p12="$c/gateway-client/client.p12" --from-file=password="$c/gateway-client/password"
    apply_secret "$ns" layerx-webhooks-component-client --from-file=token="$s/webhook-component.token"
    apply_secret "$dev" layerx-webhooks-component-client --from-file=token="$s/webhook-component.token"
    apply_secret "$ns" layerx-gateway-component-client --from-file=token="$s/gateway-component.token"
    apply_secret "$ns" layerx-gateway-authority-client --from-file=token="$s/gateway-authority.token" --from-file=sequencer-public-key="$s/sequencer-public-key" \
        --from-file=sequencer-id="$s/sequencer-id" --from-file=sequencer-first-batch="$s/sequencer-first-batch" \
        --from-file=sequencer-last-batch="$s/sequencer-last-batch"
    apply_secret "$ns" layerx-gateway-identity-client --from-file=token="$s/gateway-identity.token"
    apply_secret "$ns" layerx-gateway-redis-tls --from-file=tls.crt="$c/gateway-redis/cert.pem" \
        --from-file=tls.key="$c/gateway-redis/key.pem" --from-file=ca.crt="$c/ca.crt"
    apply_secret "$ns" layerx-gateway-redis-auth --from-file=users.acl="$s/gateway-redis.acl"
    apply_secret "$ns" layerx-gateway-redis-client --from-file=username="$s/gateway-redis.username" --from-file=password="$s/gateway-redis.password"
    apply_secret "$ns" layerx-gateway-key-provisioning --from-file=key="$s/provisioning.key"
    apply_configmap "$ns" layerx-core-module-registry --from-file=registry.json="$s/module-registry.json"
    apply_secret "$ns" layerx-program-registry-request-client --from-file=token="$s/registry-request.token"
    apply_secret "$ns" layerx-program-registry-publication-operator --from-file=token="$s/registry-publication.token"
    apply_secret "$ns" layerx-program-registry-server-tls --from-file=tls.crt.der="$c/registry/cert.der" --from-file=tls.key.der="$c/registry/key.der"
    apply_secret "$ns" layerx-program-registry-node-client --from-file=token="$s/registry-node.token"
    apply_secret "$ns" layerx-program-registry-authority-client --from-file=token="$s/registry-authority.token"
    apply_secret "$ns" layerx-sequencer-trust-history --from-file=history="$s/trust-history"
    apply_configmap "$ns" layerx-receipt-authority --from-file=replica-id="$s/receipt-authority-replica-id"
    apply_secret "$ns" layerx-node-keys --from-file=sequencer.key="$s/node-sequencer.key" --from-file=treasury.key="$s/node-treasury.key"
    if [ -n "$CUSTODY_PROFILE" ]; then
        apply_configmap "$ns" layerx-node-custody-profile --from-file=profile="$CUSTODY_PROFILE"
    fi
    apply_secret "$ns" layerx-node-tokens --from-file=program-token="$s/node-program.token" --from-file=replica-token="$s/node-replica.token"
    apply_secret "$ns" layerx-pending-core-tls --from-file=server.crt.der="$c/pending-core/cert.der" --from-file=server.key.der="$c/pending-core/key.der"
    apply_secret "$ns" layerx-pending-core-admin-tls --from-file=server.crt.der="$c/pending-core-admin/cert.der" --from-file=server.key.der="$c/pending-core-admin/key.der"
    apply_secret "$ns" layerx-receipt-authority-tls --from-file=server.crt.der="$c/receipt-authority/cert.der" --from-file=server.key.der="$c/receipt-authority/key.der"
    apply_secret "$ns" layerx-agent-boundary-tls --from-file=server.crt.der="$c/agent-boundary/cert.der" --from-file=server.key.der="$c/agent-boundary/key.der"
    apply_secret "$ns" layerx-webhooks-authority-client --from-file=token="$s/developer-authority.token"
    apply_secret "$ns" layerx-identity-server-tls --from-file=server.crt.der="$c/identity/cert.der" --from-file=server.key.der="$c/identity/key.der"
    apply_secret "$ns" layerx-identity-service-tokens --from-file="$s/identity-tokens"
    apply_secret "$ns" layerx-identity-store-key --from-file=key="$s/identity-store.key"
    apply_secret "$ns" paxeer-boundary-tls --from-file=server.crt.der="$c/paxeer-boundary/cert.der" --from-file=server.key.der="$c/paxeer-boundary/key.der"
    apply_secret "$ns" paxeer-deployer-address --from-file=address="$s/paxeer-deployer.address"
    apply_tls_secret "$ns" layerx-testnet-ingress-tls testnet-control
    apply_tls_secret "$ns" layerx-gateway-ingress-tls gateway
    apply_tls_secret "$ns" layerx-faucet-ingress-tls faucet
    apply_secret "$dev" layerx-internal-ca --from-file=ca.crt.der="$c/ca.der" --from-file=ca.crt="$c/ca.crt"
    apply_secret "$dev" layerx-developer-hosted-runtime \
        --from-file=tls-cert.der="$c/developer/cert.der" --from-file=tls-key.der="$c/developer/key.der" \
        --from-file=internal-ca.der="$c/ca.der" --from-file=public-ca.der="$c/ca.der" \
        --from-file=client-identity.p12="$c/developer-client/client.p12" --from-file=client-password="$c/developer-client/password" \
        --from-file=webhook-redis-username="$s/webhook-redis.username" --from-file=webhook-redis-password="$s/webhook-redis.password" \
        --from-file=dashboard-redis-username="$s/dashboard-redis.username" --from-file=dashboard-redis-password="$s/dashboard-redis.password" \
        --from-file=kms-token="$s/developer-kms.token" --from-file=identity-token="$s/developer-identity.token" \
        --from-file=authority-token="$s/developer-authority.token" \
        --from-file=journey-source-token="$s/developer-journey.token" --from-file=payment-source-token="$s/developer-payment.token" \
        --from-file=approval-source-token="$s/developer-approval.token" --from-file=program-source-token="$s/developer-program.token" \
        --from-file=source-trigger-token="$s/developer-source-trigger.token" --from-file=webhook-operator-token="$s/developer-operator.token" \
        --from-file=cursor-key="$s/cursor.key" --from-file=sequencer-public-key="$s/sequencer-public-key" \
        --from-file=sequencer-id="$s/sequencer-id" --from-file=sequencer-first-batch="$s/sequencer-first-batch" \
        --from-file=sequencer-last-batch="$s/sequencer-last-batch"
    apply_tls_secret "$dev" layerx-developer-ingress-tls developer
}

builder_release_publish() {
    local ns="$TESTNET_NAMESPACE" ref digest bwrap_digest cgroup_digest sums
    ref=$(image_ref layerx-program-registry)
    sums=$(docker run --rm --entrypoint /bin/sh "$ref" -c 'sha256sum /usr/bin/bwrap /usr/bin/layerx-cgroup-exec')
    bwrap_digest=$(printf '%s\n' "$sums" | awk '$2 == "/usr/bin/bwrap" { print $1 }')
    cgroup_digest=$(printf '%s\n' "$sums" | awk '$2 == "/usr/bin/layerx-cgroup-exec" { print $1 }')
    [ "${#bwrap_digest}" -eq 64 ] && [ "${#cgroup_digest}" -eq 64 ] || fail "could not read bwrap and layerx-cgroup-exec digests from $ref"
    printf '%s' "$bwrap_digest" > "$SECRETS_DIR/bwrap-digest"
    printf '%s' "$cgroup_digest" > "$SECRETS_DIR/cgroup-exec-digest"
    if [ -z "${LAYERX_BETA_BUILDER_ENVIRONMENT_DIR:-}" ]; then
        MISSING_INPUTS+=("LAYERX_BETA_BUILDER_ENVIRONMENT_DIR (hermetic builder root filesystem for the layerx-program-builder-release volume; the registry cannot start without it)")
        return 0
    fi
    [ -d "$LAYERX_BETA_BUILDER_ENVIRONMENT_DIR" ] || fail "LAYERX_BETA_BUILDER_ENVIRONMENT_DIR=$LAYERX_BETA_BUILDER_ENVIRONMENT_DIR is not a directory"
    [ -f "$LAYERX_BETA_BUILDER_ENVIRONMENT_DIR/bin/layerx-build" ] || fail "LAYERX_BETA_BUILDER_ENVIRONMENT_DIR lacks the bin/layerx-build entrypoint"
    digest=$(environment_digest "$LAYERX_BETA_BUILDER_ENVIRONMENT_DIR") || fail "builder environment digest failed"
    printf '%s' "$digest" > "$SECRETS_DIR/environment-tree-digest"
    apply_configmap "$ns" layerx-program-builder-release --from-file=environment-tree-digest="$SECRETS_DIR/environment-tree-digest" \
        --from-file=bwrap-digest="$SECRETS_DIR/bwrap-digest" --from-file=cgroup-exec-digest="$SECRETS_DIR/cgroup-exec-digest"
    cat > "$MANIFESTS_DIR/builder-release.yaml" <<EOF
apiVersion: v1
kind: PersistentVolumeClaim
metadata: {name: layerx-program-builder-release, namespace: $ns}
spec: {accessModes: [ReadWriteOnce], resources: {requests: {storage: 8Gi}}}
---
apiVersion: v1
kind: Pod
metadata: {name: layerx-program-builder-loader, namespace: $ns, labels: {app: layerx-program-builder-loader}}
spec:
  restartPolicy: Never
  nodeSelector: {$BOUNDARY_LABEL: "v1"}
  securityContext: {runAsNonRoot: true, runAsUser: 4030, runAsGroup: 4030, fsGroup: 4030}
  containers:
    - name: loader
      image: $ref
      imagePullPolicy: $PULL_POLICY
      command: [sh, -c, "while [ ! -f /opt/layerx-builder/.sealed ]; do sleep 1; done"]
      securityContext: {allowPrivilegeEscalation: false, capabilities: {drop: [ALL]}}
      volumeMounts: [{name: builder, mountPath: /opt/layerx-builder}]
  volumes: [{name: builder, persistentVolumeClaim: {claimName: layerx-program-builder-release}}]
EOF
    kube apply -f "$MANIFESTS_DIR/builder-release.yaml" > /dev/null
    kube -n "$ns" wait --for=condition=Ready pod/layerx-program-builder-loader --timeout=300s > /dev/null
    kube -n "$ns" exec layerx-program-builder-loader -- sh -c 'rm -rf /opt/layerx-builder/rootfs && mkdir -p /opt/layerx-builder/rootfs'
    tar --mode=u+w -C "$LAYERX_BETA_BUILDER_ENVIRONMENT_DIR" -cf - . | kube -n "$ns" exec -i layerx-program-builder-loader -- tar -C /opt/layerx-builder/rootfs -xf -
    kube -n "$ns" exec layerx-program-builder-loader -- sh -c 'chmod -R a-w /opt/layerx-builder/rootfs && touch /opt/layerx-builder/.sealed'
    kube -n "$ns" wait --for=jsonpath='{.status.phase}'=Succeeded pod/layerx-program-builder-loader --timeout=120s > /dev/null
    kube -n "$ns" delete pod layerx-program-builder-loader --wait=true > /dev/null
}

render_manifest() {
    local src=$1 dst=$2 name canonical ref id
    cp "$src" "$dst"
    while read -r name canonical ref id; do
        sed -i "s|image: $canonical\$|image: $ref|" "$dst"
    done < "$WORK_DIR/images"
    sed -i "s|imagePullPolicy: Always|imagePullPolicy: $PULL_POLICY|" "$dst"
    sed -i "s|developers\.layerx\.example|$DEVELOPER_HOST|g" "$dst"
    if grep -q 'ghcr.io/' "$dst"; then
        fail "rendered manifest $dst still references an unbuilt image: $(grep -o 'ghcr.io/[^ ]*' "$dst" | sort -u | tr '\n' ' ')"
    fi
}

paxeer_observer_render() {
    python3 - "$MANIFESTS_DIR/paxeer.yaml" <<'PYOBSERVER'
import copy
import sys
import yaml
path = sys.argv[1]
with open(path) as source:
    documents = list(yaml.safe_load_all(source))
stateful = next(doc for doc in documents if doc['kind'] == 'StatefulSet')
pod = stateful['spec']['template']['spec']
initializer = copy.deepcopy(pod['initContainers'][0])
initializer['name'] = 'observer-genesis'
initializer['env'] = []
initializer['command'] = ['bash', '-ec', r'''
primary=/var/lib/paxeer
observer=/var/lib/paxeer-observer
if [ ! -e "$observer/config/.observer-initialised" ]; then
    test ! -e "$observer/config/genesis.json"
    paxd init paxeer-observer --chain-id "$(jq -r .chain_id "$primary/config/genesis.json")" --home "$observer" >/dev/null 2>&1
    cp "$primary/config/genesis.json" "$observer/config/genesis.json"
    cp "$primary/config/config.toml" "$observer/config/config.toml"
    cp "$primary/config/app.toml" "$observer/config/app.toml"
    peer=$(paxd tendermint show-node-id --home "$primary")
    sed -i 's/^mode = .*/mode = "full"/; s/127.0.0.1:26657/127.0.0.1:26667/g; s/127.0.0.1:26656/127.0.0.1:26666/g' "$observer/config/config.toml"
    sed -i "s/^persistent-peers = .*/persistent-peers = \"$peer@127.0.0.1:26656\"/" "$observer/config/config.toml"
    sed -i 's/^http_port = 8545$/http_port = 8555/; s/^ws_port = 8546$/ws_port = 8556/; s/127.0.0.1:9090/127.0.0.1:9190/g; s/127.0.0.1:9091/127.0.0.1:9191/g' "$observer/config/app.toml"
    paxd validate-genesis --home "$observer" >/dev/null
    touch "$observer/config/.observer-initialised"
fi
cmp "$primary/config/genesis.json" "$observer/config/genesis.json"
''']
initializer['volumeMounts'] = [
    {'name': 'data', 'mountPath': '/var/lib/paxeer', 'readOnly': True},
    {'name': 'observer-data', 'mountPath': '/var/lib/paxeer-observer'},
    {'name': 'tmp', 'mountPath': '/tmp'},
]
pod['initContainers'].append(initializer)
observer = copy.deepcopy(next(c for c in pod['containers'] if c['name'] == 'paxd'))
observer['name'] = 'paxd-observer'
observer['args'] = ['start', '--home', '/var/lib/paxeer-observer']
observer['volumeMounts'] = [
    {'name': 'observer-data', 'mountPath': '/var/lib/paxeer-observer'},
    {'name': 'tmp', 'mountPath': '/tmp'},
]
pod['containers'].append(observer)
boundary = copy.deepcopy(next(c for c in pod['containers'] if c['name'] == 'boundary'))
boundary['name'] = 'observer-boundary'
for env in boundary['env']:
    if env['name'] == 'LAYERX_PAXEER_BOUNDARY_LISTEN':
        env['value'] = '0.0.0.0:9444'
    elif env['name'] == 'LAYERX_PAXEER_NODE_URL':
        env['value'] = 'http://127.0.0.1:8555'
    elif env['name'] == 'LAYERX_PAXEER_COMET_URL':
        env['value'] = 'http://127.0.0.1:26667'
boundary['ports'] = [{'name': 'observer-https', 'containerPort': 9444}]
for probe in ['readinessProbe', 'livenessProbe']:
    boundary[probe]['httpGet']['port'] = 'observer-https'
pod['containers'].append(boundary)
claim = copy.deepcopy(stateful['spec']['volumeClaimTemplates'][0])
claim['metadata']['name'] = 'observer-data'
stateful['spec']['volumeClaimTemplates'].append(claim)
documents.append({
    'apiVersion': 'v1', 'kind': 'Service',
    'metadata': {'name': 'paxeer-observer-boundary', 'namespace': 'layerx-testnet'},
    'spec': {'selector': {'app': 'paxeer'}, 'ports': [
        {'name': 'https', 'port': 9443, 'targetPort': 'observer-https'}]},
})
for doc in documents:
    if doc['kind'] == 'NetworkPolicy' and doc['metadata']['name'] == 'paxeer-boundary':
        for rule in doc['spec']['ingress']:
            rule['ports'].append({'protocol': 'TCP', 'port': 9444})
with open(path, 'w') as output:
    yaml.safe_dump_all(documents, output, sort_keys=False)
PYOBSERVER
}

paxeer_origins_write() {
    mkdir -p "$WORK_DIR/paxeer"
    openssl x509 -inform DER -in "$CA_DIR/ca.der" -out "$CA_DIR/ca.pem"
    jq -n --arg primary "$PAXEER_URL" --arg observer "$PAXEER_OBSERVER_URL" \
        --arg ca "$CA_DIR/ca.pem" --arg key "$SECRETS_DIR/paxeer-deployer.key" \
        '{rpc_origins: [$primary, $observer], ca_bundle: $ca, key_file: $key,
          backends: [{container: "paxd", home: "/var/lib/paxeer"},
                     {container: "paxd-observer", home: "/var/lib/paxeer-observer"}]}' \
        > "$WORK_DIR/paxeer/rpc-origins.json"
    local comet_chain_id
    comet_chain_id=$(kube -n "$TESTNET_NAMESPACE" exec paxeer-0 -c paxd -- jq -er .chain_id /var/lib/paxeer/config/genesis.json)
    python3 "$SCRIPT_DIR/paxeer-identity.py" "$WORK_DIR/paxeer/rpc-origins.json" "$comet_chain_id"
    log "Paxeer origins: $PAXEER_URL $PAXEER_OBSERVER_URL; CA bundle: $CA_DIR/ca.pem; inputs: $WORK_DIR/paxeer/rpc-origins.json"
}

manifests_render() {
    mkdir -p "$MANIFESTS_DIR"
    render_manifest "$NODE_MANIFEST" "$MANIFESTS_DIR/node.yaml"
    sed -i "s|^  replica-id: \"[0-9a-f]*\"$|  replica-id: \"$(cat "$SECRETS_DIR/receipt-authority-replica-id")\"|" "$MANIFESTS_DIR/node.yaml"
    grep -q "^  replica-id: \"$(cat "$SECRETS_DIR/receipt-authority-replica-id")\"$" "$MANIFESTS_DIR/node.yaml" \
        || fail "the node manifest replica-id could not be bound to the generated receipt authority replica id"
    if [ -n "$CUSTODY_PROFILE" ]; then
        python3 - "$MANIFESTS_DIR/node.yaml" <<'PY'
import sys
import yaml
path = sys.argv[1]
with open(path) as source:
    documents = list(yaml.safe_load_all(source))
for document in documents:
    if document.get("kind") != "StatefulSet":
        continue
    pod = document["spec"]["template"]["spec"]
    daemon = next(container for container in pod["containers"] if container["name"] == "layerxd")
    daemon["args"] += ["--custody-profile", "/run/layerx/custody.profile"]
    daemon["volumeMounts"].append({"name": "custody-profile", "mountPath": "/run/layerx/custody.profile", "subPath": "profile", "readOnly": True})
    pod["volumes"].append({"name": "custody-profile", "configMap": {"name": "layerx-node-custody-profile"}})
with open(path, "w") as output:
    yaml.safe_dump_all(documents, output, sort_keys=False)
PY
    fi
    render_manifest "$REPO_ROOT/platform/hosted/identity/deployment.yaml" "$MANIFESTS_DIR/identity.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/paxeer/deployment.yaml" "$MANIFESTS_DIR/paxeer.yaml"
    paxeer_observer_render
    render_manifest "$REPO_ROOT/platform/hosted/testnet/deployment.yaml" "$MANIFESTS_DIR/testnet.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/gateway/deployment.yaml" "$MANIFESTS_DIR/gateway.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/registry/deployment.yaml" "$MANIFESTS_DIR/registry.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/human/deployment.yaml" "$MANIFESTS_DIR/human.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/internal/deployment.yaml" "$MANIFESTS_DIR/internal.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/webhooks/deployment.yaml" "$MANIFESTS_DIR/developer.yaml"
    python3 "$SCRIPT_DIR/sequencer-pins.py" --manifests "$MANIFESTS_DIR"
    cat >> "$MANIFESTS_DIR/testnet.yaml" <<EOF
---
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: layerx-faucet-public
  namespace: $TESTNET_NAMESPACE
  annotations: {nginx.ingress.kubernetes.io/backend-protocol: HTTPS}
spec:
  ingressClassName: nginx
  tls: [{hosts: [$FAUCET_HOST], secretName: layerx-faucet-ingress-tls}]
  rules:
    - host: $FAUCET_HOST
      http:
        paths:
          - path: /
            pathType: Prefix
            backend: {service: {name: layerx-faucet-public, port: {name: https}}}
EOF
}

trusted_boundary_apply() {
    local ns="$TESTNET_NAMESPACE" service
    kube apply -f "$MANIFESTS_DIR/paxeer.yaml" > /dev/null
    kube apply -f "$MANIFESTS_DIR/identity.yaml" > /dev/null
    kube apply -f "$MANIFESTS_DIR/node.yaml" > /dev/null
    for service in "${TRUSTED_BOUNDARY_SERVICES[@]}"; do
        kube -n "$ns" get service "$service" > /dev/null 2>&1 || fail "trusted-boundary Service $ns/$service was not created by the repository manifests"
    done
}

manifests_apply() {
    kube apply -f "$MANIFESTS_DIR/human.yaml" > /dev/null
    kube apply -f "$MANIFESTS_DIR/testnet.yaml" > /dev/null
    kube apply -f "$MANIFESTS_DIR/gateway.yaml" > /dev/null
    kube apply -f "$MANIFESTS_DIR/registry.yaml" > /dev/null
}

internal_apply() {
    kube apply -f "$MANIFESTS_DIR/internal.yaml" > /dev/null
    kube -n "$DEVELOPER_NAMESPACE" apply -f "$MANIFESTS_DIR/developer.yaml" > /dev/null
}

node_exec() {
    kube -n "$TESTNET_NAMESPACE" exec layerx-node-0 -c layerxd -- "$@"
}

node_file_fetch() {
    # node_file_fetch REMOTE LOCAL
    node_exec base64 -w0 "$1" | base64 -d > "$2"
    [ -s "$2" ] || fail "node file $1 is empty"
}

wait_for_node_genesis() {
    local deadline=$((SECONDS + 600)) data=/var/lib/layerx/node
    log "waiting for the node bootstrap to produce its genesis artifacts"
    while :; do
        if node_exec sh -c "test -r $data/node.env && test -s $data/genesis/paxeer-deployment-descriptor.lxgd && test -s $data/genesis/paxeer-registration-request.lxrr" > /dev/null 2>&1; then
            break
        fi
        if [ "$SECONDS" -ge "$deadline" ]; then
            kube -n "$TESTNET_NAMESPACE" get pod layerx-node-0 -o wide >&2 || true
            kube -n "$TESTNET_NAMESPACE" logs layerx-node-0 -c layerxd --tail=40 >&2 || true
            fail "the node did not bootstrap its genesis within 600s"
        fi
        sleep 5
    done
    mkdir -p "$WORK_DIR/genesis"
    node_file_fetch "$data/genesis/paxeer-deployment-descriptor.lxgd" "$WORK_DIR/genesis/paxeer-deployment-descriptor.lxgd"
    node_file_fetch "$data/genesis/paxeer-registration-request.lxrr" "$WORK_DIR/genesis/paxeer-registration-request.lxrr"
    node_file_fetch "$data/node.env" "$WORK_DIR/genesis/node.env"
    NODE_GUARANTOR_ID=$(sed -n 's/^LAYERX_NODE_GENESIS_GUARANTOR_ID=//p' "$WORK_DIR/genesis/node.env")
    NODE_GUARANTOR_PUBLIC_KEY=$(sed -n 's/^LAYERX_NODE_GENESIS_GUARANTOR_PUBLIC_KEY=//p' "$WORK_DIR/genesis/node.env")
    NODE_SEQUENCER_ID=$(sed -n 's/^LAYERX_NODE_SEQUENCER_ID=//p' "$WORK_DIR/genesis/node.env")
    NODE_SEQUENCER_PUBLIC_KEY=$(sed -n 's/^LAYERX_NODE_SEQUENCER_PUBLIC_KEY=//p' "$WORK_DIR/genesis/node.env")
    [[ $NODE_GUARANTOR_ID =~ ^[0-9a-f]{64}$ ]] || fail "node.env carries no genesis guarantor id"
    [[ $NODE_GUARANTOR_PUBLIC_KEY =~ ^0[23][0-9a-f]{64}$ ]] || fail "node.env carries no compressed genesis guarantor public key"
    [ "$NODE_SEQUENCER_ID" = "$SEQUENCER_ID" ] || fail "the node derived sequencer id $NODE_SEQUENCER_ID but the registry trust history carries $SEQUENCER_ID"
    [ "$NODE_SEQUENCER_PUBLIC_KEY" = "$(cat "$CA_DIR/sequencer.pub.hex")" ] || fail "the node sequencer public key differs from the generated sequencer key"
}

guarantor_set_render() {
    jq -s 'sort_by(.guarantor_id) | to_entries | map(.value + {governance_sequence: (.key + 1)})' "$1"
}

guarantor_sequence_test() (
    set -euo pipefail
    local dir threshold index public signer id mutation
    dir=$(mktemp -d)
    trap 'rm -rf "$dir"' EXIT
    umask 077
    for threshold in 2 3; do
        : > "$dir/members.jsonl"
        for ((index = threshold; index > 0; index--)); do
            openssl ecparam -name secp256k1 -genkey -noout -out "$dir/member.key"
            public=0x$(openssl ec -in "$dir/member.key" -pubout -conv_form compressed -outform DER 2>/dev/null | tail -c 33 | od -An -v -tx1 | tr -d ' \n')
            signer=$(python3 "$REPO_ROOT/platform/hosted/paxeer/settlement-domain.py" signer "$public")
            printf -v id '0x%064x' "$index"
            jq -n --arg id "$id" --arg public "$public" --arg signer "$signer" \
                '{guarantor_id: $id, public_key: $public, signer: $signer, bond_controller: $signer, joined_epoch: 1, bond_amount: "1"}' >> "$dir/members.jsonl"
        done
        guarantor_set_render "$dir/members.jsonl" > "$dir/members.json"
        jq -e --argjson threshold "$threshold" '
            length == $threshold and . == sort_by(.guarantor_id) and
            [.[].governance_sequence] == [range(1; $threshold + 1)]
        ' "$dir/members.json" > /dev/null
        jq -s 'sort_by(.guarantor_id)' "$dir/members.jsonl" > "$dir/expected.json"
        jq -e --slurpfile expected "$dir/expected.json" 'map(del(.governance_sequence)) == $expected[0]' "$dir/members.json" > /dev/null
        LAYERX_PAXEER_GUARANTORS="$dir/members.json" bash "$REPO_ROOT/platform/hosted/paxeer/deploy-contracts.sh" check-guarantors
        for mutation in 'map(.governance_sequence = 1)' '.[0].governance_sequence = 0' '.[1].governance_sequence = 3' '.[1].governance_sequence = "2"' '.[1].governance_sequence = 2.5' 'reverse'; do
            jq "$mutation" "$dir/members.json" > "$dir/invalid.json"
            if LAYERX_PAXEER_GUARANTORS="$dir/invalid.json" bash "$REPO_ROOT/platform/hosted/paxeer/deploy-contracts.sh" check-guarantors > "$dir/refusal.log" 2>&1; then
                fail "non-contiguous governance sequences were accepted: $mutation"
            fi
            grep -q 'governance sequences must be contiguous from 1 in member order' "$dir/refusal.log"
        done
        log "guarantor sequence threshold $threshold: sorted 1..$threshold and six explicit refusals passed"
    done
)

paxeer_contracts_deploy() {
    [ "${LAYERX_BETA_FORBIDDEN_CHAIN_ID:-}" != "$PAXEER_CHAIN_ID" ] \
        || fail "contract transactions on chain $PAXEER_CHAIN_ID are forbidden by LAYERX_BETA_FORBIDDEN_CHAIN_ID"
    local dir="$WORK_DIR/paxeer" signer bond_amount count index id public controller previous_id=""
    mkdir -p "$dir/guarantor-keys"
    chmod 0700 "$dir/guarantor-keys"
    count=$(sed -n 's/^LAYERX_NODE_GENESIS_GUARANTOR_COUNT=//p' "$WORK_DIR/genesis/node.env")
    [[ $count =~ ^[1-9][0-9]*$ ]] && [ "$count" -le 32 ] || fail "node.env carries no bounded genesis guarantor count"
    [ "$count" -ge "$(jq -er '.finality_policy.certificate_threshold' "$REPO_ROOT/contracts/config/checkpoint-settlement.json")" ] \
        || fail "genesis guarantors cannot meet the certificate threshold"
    bond_amount=$(jq -r '((.usdl_custody_cap | tonumber) * .minimum_bond_bps / 10000 | floor) | tostring' "$REPO_ROOT/platform/hosted/paxeer/deployment-input.beta.json")
    [[ $bond_amount =~ ^[1-9][0-9]*$ ]] || fail "the beta deployment input yields no positive minimum guarantor bond"
    : > "$dir/guarantors.jsonl"
    for ((index = 0; index < count; index++)); do
        id=$(sed -n "s/^LAYERX_NODE_GENESIS_GUARANTOR_ID_$index=//p" "$WORK_DIR/genesis/node.env")
        public=$(sed -n "s/^LAYERX_NODE_GENESIS_GUARANTOR_PUBLIC_KEY_$index=//p" "$WORK_DIR/genesis/node.env")
        [[ $id =~ ^[0-9a-f]{64}$ && $id > $previous_id ]] || fail "genesis guarantor ids must be strictly ascending"
        [[ $public =~ ^0[23][0-9a-f]{64}$ ]] || fail "node.env carries no compressed genesis guarantor public key"
        previous_id=$id
        controller=paxeer-guarantor-controller
        if [ "$index" -gt 0 ]; then
            controller="paxeer-guarantor-controller-$index"
            evm_key_generate "$controller"
        fi
        signer=$(python3 "$REPO_ROOT/platform/hosted/paxeer/settlement-domain.py" signer "0x$public") || fail "guarantor signer derivation failed"
        jq -n --arg id "0x$id" --arg signer "$signer" --arg public_key "0x$public" \
            --arg controller "$(cat "$SECRETS_DIR/$controller.address")" --arg bond "$bond_amount" \
            '{guarantor_id: $id, signer: $signer, public_key: $public_key, bond_controller: $controller, joined_epoch: 1, bond_amount: $bond}' \
            >> "$dir/guarantors.jsonl"
        (umask 077; cp "$SECRETS_DIR/$controller.key" "$dir/guarantor-keys/0x$id.controller.key")
    done
    guarantor_set_render "$dir/guarantors.jsonl" > "$dir/guarantors.json"
    jq --arg proposer "$(cat "$SECRETS_DIR/paxeer-final-proposer.address")" --arg executor "$(cat "$SECRETS_DIR/paxeer-final-executor.address")" \
        --arg council "$(cat "$SECRETS_DIR/paxeer-emergency-council.address")" \
        '. + {protocol_version: 3, final_proposer: $proposer, final_executor: $executor, emergency_council: $council}' \
        "$REPO_ROOT/platform/hosted/paxeer/deployment-input.beta.json" > "$dir/deployment-input.json"
    cp "$REPO_ROOT/contracts/config/checkpoint-settlement.json" "$dir/checkpoint-settlement.json"
    log "deploying the settlement contracts from the node genesis through the Paxeer boundary"
    if ! LAYERX_PAXEER_BOUNDARY_URL="$PAXEER_URL" LAYERX_PAXEER_BOUNDARY_CA_DER="$CA_DIR/ca.der" LAYERX_PAXEER_CHAIN_ID="$PAXEER_CHAIN_ID" \
        LAYERX_PAXEER_DEPLOYER_KEY_FILE="$SECRETS_DIR/paxeer-deployer.key" LAYERX_PAXEER_GENESIS_DIR="$WORK_DIR/genesis" \
        LAYERX_PAXEER_DEPLOYMENT_INPUT="$dir/deployment-input.json" LAYERX_PAXEER_GUARANTORS="$dir/guarantors.json" \
        LAYERX_PAXEER_GUARANTOR_KEYS_DIR="$dir/guarantor-keys" LAYERX_PAXEER_DEPLOYMENT_RECORD="$dir/deployment.json" \
        LAYERX_PAXEER_SETTLEMENT_JSON="$dir/checkpoint-settlement.json" LAYERX_PAXEER_SETTLEMENT_DOMAIN=beta \
        LAYERX_PAXEER_FOUNDRY_BIN="$FOUNDRY_BIN" \
        bash "$REPO_ROOT/platform/hosted/paxeer/deploy-contracts.sh" bootstrap > "$LOG_DIR/deploy-contracts.log" 2>&1; then
        tail -n 40 "$LOG_DIR/deploy-contracts.log" >&2
        fail "deploy-contracts.sh bootstrap failed (log $LOG_DIR/deploy-contracts.log)"
    fi
    GUARANTOR_BOND=$(jq -r '.addresses.guarantor_bond' "$dir/deployment.json")
    CHECKPOINT_REGISTRY=$(jq -r '.addresses.checkpoint_registry' "$dir/deployment.json")
    [[ $GUARANTOR_BOND =~ ^0x[0-9a-fA-F]{40}$ ]] && [[ $CHECKPOINT_REGISTRY =~ ^0x[0-9a-fA-F]{40}$ ]] || fail "the deployment record lacks the GuarantorBond and CheckpointRegistry addresses"
    [ "$(jq -r '.network_id' "$dir/deployment.json")" = "$NODE_NETWORK_ID" ] || fail "the deployed CheckpointRegistry network id differs from the node network id $NODE_NETWORK_ID"
    log "settlement contracts deployed: GuarantorBond $GUARANTOR_BOND, CheckpointRegistry $CHECKPOINT_REGISTRY"
}

settlement_publish() {
    local ns="$TESTNET_NAMESPACE"
    if [ -n "$CUSTODY_PROFILE" ]; then
        custody_registration_publish
    fi
    printf 'LAYERX_NODE_PAXEER_CHAIN_ID=%s\nLAYERX_NODE_SETTLEMENT_CONTRACT=%s\nLAYERX_NODE_CHECKPOINT_REGISTRY=%s\nLAYERX_NODE_PAXEER_RPC_ADDRESS=127.0.0.1\nLAYERX_NODE_PAXEER_RPC_PORT=%s\n' \
        "$PAXEER_CHAIN_ID" "$GUARANTOR_BOND" "$CHECKPOINT_REGISTRY" "$PAXEER_RELAY_PORT" > "$WORK_DIR/paxeer/settlement.env"
    bash "$REPO_ROOT/platform/hosted/node/bootstrap.sh" --check-settlement "$WORK_DIR/paxeer/settlement.env" > /dev/null \
        || fail "the settlement environment was refused by bootstrap.sh --check-settlement"
    apply_configmap "$ns" layerx-node-settlement --from-file=settlement.env="$WORK_DIR/paxeer/settlement.env"
    log "settlement environment published as ConfigMap $ns/layerx-node-settlement"
}

custody_registration_publish() {
    local field
    local -a roots=()
    for field in genesisManifestDigest genesisCanonicalStateRoot genesisReceiptRoot; do
        roots+=("$(SSL_CERT_FILE="$CA_DIR/ca.crt" "$FOUNDRY_BIN/cast" call --rpc-url "$PAXEER_URL" \
            "$CHECKPOINT_REGISTRY" "$field()(bytes32)")")
    done
    python3 - "$WORK_DIR/genesis/paxeer-deployment-descriptor.lxgd" \
        "$WORK_DIR/genesis/paxeer-registration-request.lxrr" \
        "$WORK_DIR/genesis/genesis.registration" "${roots[@]}" <<'PY'
import pathlib
import sys
descriptor = pathlib.Path(sys.argv[1]).read_bytes()
request = pathlib.Path(sys.argv[2]).read_bytes()
roots = [bytes.fromhex(value.removeprefix("0x")) for value in sys.argv[4:]]
if (len(descriptor) != 105 or descriptor[:5] != b"LXGD\x01" or
        len(request) != 73 or request[:5] != b"LXRR\x01" or
        request[5:9] != descriptor[5:9] or
        any(len(root) != 32 for root in roots) or
        descriptor[9:] != b"".join(roots) or request[9:] != b"".join(roots[1:])):
    raise SystemExit("custody genesis differs from the deployed CheckpointRegistry")
registration = b"LXGR\x01" + request[5:9] + bytes(8) + roots[2] + roots[2] + b"\x01"
with open(sys.argv[3], "xb") as output:
    output.write(registration)
PY
    kube -n "$TESTNET_NAMESPACE" exec -i layerx-node-0 -c layerxd -- sh -ec \
        'umask 077; cat > /var/lib/layerx/node/genesis/genesis.registration.tmp; mv /var/lib/layerx/node/genesis/genesis.registration.tmp /var/lib/layerx/node/genesis/genesis.registration' \
        < "$WORK_DIR/genesis/genesis.registration"
}

wait_for_pod_ready() {
    # wait_for_pod_ready NAMESPACE SELECTOR SECONDS
    local namespace=$1 selector=$2 seconds=$3
    if ! kube -n "$namespace" wait --for=condition=Ready pod -l "$selector" --timeout="${seconds}s" > /dev/null 2>&1; then
        kube -n "$namespace" get pods -l "$selector" -o wide >&2 || true
        fail "pods $namespace/$selector did not become ready within ${seconds}s"
    fi
}

identity_request() {
    # identity_request METHOD PATH BODY_FILE OUT_FILE -> status code
    curl --silent --show-error --max-time 30 --cacert "$CA_DIR/ca.crt" \
        --header "Authorization: Bearer $(cat "$SECRETS_DIR/identity-tokens/provisioning")" \
        --header 'Content-Type: application/json' --request "$1" --data-binary "@$3" \
        --output "$4" --write-out '%{http_code}' "$IDENTITY_URL$2"
}

identity_provision() {
    local dir="$WORK_DIR/identity" status
    mkdir -p "$dir"
    chmod 0700 "$dir"
    jq -n --arg sub "$TEST_SOURCE_DID" --arg key "$(cat "$SECRETS_DIR/test-source-signer.pub.hex")" \
        '{sub: $sub, allowed_signer_public_keys: [$key]}' > "$dir/source-principal.json"
    status=$(identity_request POST /v1/principals "$dir/source-principal.json" "$dir/source-principal.response.json")
    [ "$status" = 201 ] || [ "$status" = 200 ] || fail "identity refused the smoke source principal with status $status: $(cat "$dir/source-principal.response.json")"
    jq -n --arg sub "$TEST_DESTINATION_DID" --arg key "$(cat "$SECRETS_DIR/test-destination-signer.pub.hex")" \
        '{sub: $sub, allowed_signer_public_keys: [$key]}' > "$dir/destination-principal.json"
    status=$(identity_request POST /v1/principals "$dir/destination-principal.json" "$dir/destination-principal.response.json")
    [ "$status" = 201 ] || [ "$status" = 200 ] || fail "identity refused the smoke destination principal with status $status: $(cat "$dir/destination-principal.response.json")"
    if [ "$TEST_AUTH_SOURCE" = identity-provisioning ]; then
        jq -n --arg sub "$TEST_SOURCE_DID" '{sub: $sub}' > "$dir/source-session.json"
        (umask 077; : > "$dir/source-session.response.json")
        status=$(identity_request POST /v1/sessions "$dir/source-session.json" "$dir/source-session.response.json")
        [ "$status" = 201 ] || [ "$status" = 200 ] || fail "identity refused the smoke source session with status $status"
        (umask 077; jq -r '.token' "$dir/source-session.response.json" > "$SECRETS_DIR/test-auth.token")
        grep -Eq '^ses_[0-9a-f]{32}\.[0-9a-f]{64}$' "$SECRETS_DIR/test-auth.token" || fail "identity returned no session token for the smoke source"
        rm -f "$dir/source-session.response.json"
    fi
    log "identity provisioned $TEST_SOURCE_DID and $TEST_DESTINATION_DID (session token source: $TEST_AUTH_SOURCE)"
}

internal_principals_provision() {
    local service scope status dir="$WORK_DIR/internal-principals"
    mkdir -p "$dir"
    chmod 0700 "$dir"
    for service in payments programs; do
        if [ "$service" = payments ]; then scope=receipt:read; else scope=program:read; fi
        jq -n --arg key "$(cat "$SECRETS_DIR/test-source-signer.pub.hex")" --arg scope "$scope" \
            '{signer_public_key: $key, scopes: [$scope], quota_requests: 1000, quota_window_seconds: 60}' > "$dir/$service-request.json"
        (umask 077; : > "$dir/$service-response.json")
        status=$(curl --silent --show-error --max-time 30 --cacert "$CA_DIR/ca.crt" \
            --header "Authorization: Bearer $(cat "$SECRETS_DIR/test-auth.token")" \
            --header 'Content-Type: application/json' --header "Idempotency-Key: internal-$service" \
            --data-binary "@$dir/$service-request.json" --output "$dir/$service-response.json" \
            --write-out '%{http_code}' "$GATEWAY_URL/v1/keys")
        [ "$status" = 201 ] || [ "$status" = 200 ] || fail "gateway refused $service principal key with status $status"
        (umask 077; jq -er '.key | select(.authorization_scheme == "LayerX-Key") | .id + ":" + .secret' \
            "$dir/$service-response.json" > "$dir/$service.credential")
        (umask 077; jq -n --arg sub "$TEST_SOURCE_DID" '{($sub): "/run/layerx/principal.credential"}' > "$dir/credentials.json")
        apply_secret "$INTERNAL_NAMESPACE" "layerx-internal-$service-runtime" \
            --from-file=server.der="$CA_DIR/internal-$service/cert.der" --from-file=server-key.der="$CA_DIR/internal-$service/key.der" \
            --from-file=ca.der="$CA_DIR/ca.der" --from-file=upstream-ca.der="$CA_DIR/ca.der" \
            --from-file=token="$SECRETS_DIR/developer-${service%s}.token" \
            --from-file=credentials.json="$dir/credentials.json" --from-file=principal.credential="$dir/$service.credential"
        rm -f "$dir/$service-response.json"
    done
}

port_forward_tcp_ready() {
    python3 - "$1" <<'PYTCP'
import socket
import sys
try:
    with socket.create_connection(("127.0.0.1", int(sys.argv[1])), timeout=0.2):
        pass
except OSError:
    sys.exit(1)
PYTCP
}

port_forward_supervise() {
    local name=$1 namespace=$2 service=$3 port=$4 target=$5
    local child="" failures=0 started deadline status ready
    trap 'if [ -n "$child" ]; then kill "$child" 2>/dev/null || true; wait "$child" 2>/dev/null || true; fi' EXIT
    trap 'exit 0' TERM INT HUP
    while :; do
        started=$SECONDS
        ready=0
        "$TOOLS_DIR/kubectl" --kubeconfig "$KUBECONFIG_FILE" -n "$namespace" \
            port-forward --address 127.0.0.1 "service/$service" "$port:$target" &
        child=$!
        deadline=$((SECONDS + 60))
        while kill -0 "$child" 2>/dev/null; do
            if port_forward_tcp_ready "$port"; then
                ready=1
                started=$SECONDS
                printf 'supervisor: ready name=%s child=%s port=%s\n' "$name" "$child" "$port"
                break
            fi
            if [ "$SECONDS" -ge "$deadline" ]; then
                printf 'supervisor: readiness timeout name=%s child=%s after=60s\n' "$name" "$child"
                kill "$child" 2>/dev/null || true
                break
            fi
            sleep 0.2
        done
        status=0
        wait "$child" || status=$?
        child=""
        if [ "$ready" = 1 ] && [ "$((SECONDS - started))" -ge 120 ]; then failures=0; fi
        failures=$((failures + 1))
        if [ "$failures" -ge 8 ]; then
            printf 'supervisor: exhausted name=%s failures=%s status=%s stable_window=120s\n' "$name" "$failures" "$status"
            return 1
        fi
        printf 'supervisor: restart name=%s failure=%s limit=8 stable_window=120s status=%s\n' "$name" "$failures" "$status"
        sleep 1
    done
}

port_forward_stop() {
    local pidfile=$1 pid
    [ -f "$pidfile" ] || return 0
    pid=$(cat "$pidfile")
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    local attempt
    for attempt in $(seq 1 50); do
        if ! kill -0 "$pid" 2>/dev/null; then break; fi
        sleep 0.1
    done
    rm -f "$pidfile"
}

port_forward() {
    local name=$1 namespace=$2 service=$3 port=$4 target=$5 pidfile supervisor deadline
    pidfile="$WORK_DIR/port-forward-$name.pid"
    port_forward_stop "$pidfile"
    port_forward_supervise "$name" "$namespace" "$service" "$port" "$target" \
        >> "$LOG_DIR/port-forward-$name.log" 2>&1 < /dev/null &
    supervisor=$!
    printf '%s' "$supervisor" > "$pidfile"
    deadline=$((SECONDS + 60))
    while kill -0 "$supervisor" 2>/dev/null; do
        if port_forward_tcp_ready "$port"; then return 0; fi
        [ "$SECONDS" -lt "$deadline" ] || break
        sleep 0.2
    done
    port_forward_stop "$pidfile"
    fail "port-forward to $namespace/$service did not accept TCP within 60s (log $LOG_DIR/port-forward-$name.log)"
}

port_forwards_stop() {
    local pidfile
    for pidfile in "$WORK_DIR"/port-forward-*.pid; do
        port_forward_stop "$pidfile"
    done
}

readyz() {
    curl --silent --show-error --max-time 10 --cacert "$CA_DIR/ca.crt" "$1/readyz" 2>/dev/null
}

wait_ready() {
    local deadline=$((SECONDS + READY_TIMEOUT)) body developer_ready internal_ready human_ready human_status
    while :; do
        body=$(readyz "$TESTNET_URL") || body=""
        human_status=$(curl --silent --show-error --max-time 10 --cacert "$CA_DIR/ca.crt" \
            --output "$WORK_DIR/human-readyz.json" --write-out '%{http_code}' "$HUMAN_URL/readyz" 2>/dev/null) || human_status=unreachable
        human_ready=false
        if [ "$human_status" = 200 ] && jq -e '.ready == true and all(.components[]; . == "ready")' "$WORK_DIR/human-readyz.json" >/dev/null 2>&1; then
            human_ready=true
        fi
        developer_ready=$(kube -n "$DEVELOPER_NAMESPACE" get deployments -o json 2>/dev/null \
            | jq -r '[.items[] | select((.status.readyReplicas // 0) < .spec.replicas) | .metadata.name] | join(",")') \
            || developer_ready="namespace $DEVELOPER_NAMESPACE unreadable"
        internal_ready=$(kube -n "$INTERNAL_NAMESPACE" get deployments,statefulsets -o json 2>/dev/null \
            | jq -r '[.items[] | select((.status.readyReplicas // 0) < .spec.replicas) | .metadata.name] | join(",")') \
            || internal_ready="namespace $INTERNAL_NAMESPACE unreadable"
        if [ -n "$body" ] && jq -e '.state == "ready" and all(.journeys[]; .ready == true)' <<<"$body" > /dev/null 2>&1 \
            && jq -e 'all(.dependencies[]; .ready == true) and (.journeys | length) == 4' <<<"$body" > /dev/null 2>&1 && [ -z "$developer_ready" ] && [ -z "$internal_ready" ] && [ "$human_ready" = true ]; then
            printf '%s' "$body" > "$WORK_DIR/readyz.json"
            return 0
        fi
        if [ "$SECONDS" -ge "$deadline" ]; then
            {
                printf 'beta-cluster: readiness not reached within %ss\n' "$READY_TIMEOUT"
                if [ -n "$body" ]; then
                    printf 'beta-cluster: testnet /readyz: %s\n' "$body"
                    jq -r '(.dependencies // [])[] | select(.ready != true) | "beta-cluster: dependency not ready: \(.name): \(.detail)"' <<<"$body" 2>/dev/null || true
                    jq -r '(.journeys // [])[] | select(.ready != true) | "beta-cluster: journey not ready: \(.journey) (failing: \((.failing // []) | join(",")))"' <<<"$body" 2>/dev/null || true
                else
                    printf 'beta-cluster: testnet /readyz unreachable at %s\n' "$TESTNET_URL"
                fi
                printf 'beta-cluster: Human /readyz HTTP status: %s\n' "$human_status"
                [ -z "$developer_ready" ] || printf 'beta-cluster: developer plane deployments not ready: %s\n' "$developer_ready"
                [ -z "$internal_ready" ] || printf 'beta-cluster: internal workloads not ready: %s\n' "$internal_ready"
                kube -n "$INTERNAL_NAMESPACE" get pods -o wide 2>/dev/null || true
                kube -n "$TESTNET_NAMESPACE" get pods -o wide 2>/dev/null || true
                kube -n "$DEVELOPER_NAMESPACE" get pods -o wide 2>/dev/null || true
                local input
                for input in "${MISSING_INPUTS[@]}"; do printf 'beta-cluster: missing owner input: %s\n' "$input"; done
            } >&2
            return 1
        fi
        sleep 5
    done
}

qualification_url() {
    # qualification_url VARIABLE OVERRIDE URL SURFACE
    local variable=$1 override=$2 url=$3 surface=$4 value
    value=${!override:-}
    if [ -n "$value" ]; then
        printf 'export %s=%s\n' "$variable" "$value" >> "$ENV_FILE"
        return 0
    fi
    if [ -z "$url" ]; then
        printf '# %s: %s\nunset %s\n' "$variable" "$surface" "$variable" >> "$ENV_FILE"
        MISSING_INPUTS+=("$variable: $surface; supply $override only for a real deployed surface")
        return 0
    fi
    printf '# %s: %s\nexport %s=%s\n' "$variable" "$surface" "$variable" "$url" >> "$ENV_FILE"
}

env_write() {
    mkdir -p "$WORK_DIR"
    (umask 077; : > "$ENV_FILE")
    {
        printf 'export LAYERX_TESTNET_URL=%s\n' "$TESTNET_URL"
        printf 'export LAYERX_GATEWAY_URL=%s\n' "$GATEWAY_URL"
        printf 'export LAYERX_FAUCET_URL=%s\n' "$FAUCET_URL"
        printf 'export LAYERX_TEST_AUTH_TOKEN_FILE=%s\n' "$SECRETS_DIR/test-auth.token"
        printf 'export LAYERX_TEST_CA_FILE=%s\n' "$CA_DIR/ca.crt"
        printf 'export LAYERX_TEST_SOURCE_DID=%s\n' "$TEST_SOURCE_DID"
        printf 'export LAYERX_TEST_SOURCE_PUBLIC_KEY=%s\n' "$(cat "$SECRETS_DIR/test-source-signer.pub.hex")"
        printf 'export LAYERX_TEST_SOURCE_KEY_FILE=%s\n' "$SECRETS_DIR/test-source-signer.key"
        printf 'export LAYERX_TEST_DESTINATION_DID=%s\n' "$TEST_DESTINATION_DID"
        printf 'export LAYERX_TEST_ASSET=%s\n' "$NODE_ASSET_ID"
        printf 'export LAYERX_TEST_AMOUNT=%s\n' "$TEST_AMOUNT"
        printf 'export LAYERX_GATEWAY_CA_FILE=%s\n' "$CA_DIR/ca.crt"
        printf 'export WEBHOOKS_URL=%s\n' "$DEVELOPER_URL"
        printf 'export LAYERX_AGENT_BOUNDARY_URL=%s\n' "$AGENT_URL"
        printf 'export LAYERX_IDENTITY_URL=%s\n' "$IDENTITY_URL"
        printf 'export LAYERX_PAXEER_BOUNDARY_URL=%s\n' "$PAXEER_URL"
        printf 'export LAYERX_PAXEER_SETTLEMENT_CONTRACT=%s\n' "$GUARANTOR_BOND"
        printf 'export LAYERX_PAXEER_CHECKPOINT_REGISTRY=%s\n' "$CHECKPOINT_REGISTRY"
        printf 'export LAYERX_PAXEER_DEPLOYMENT_RECORD=%s\n' "$WORK_DIR/paxeer/deployment.json"
        printf 'export KUBECONFIG=%s\n' "$KUBECONFIG_FILE"
    } >> "$ENV_FILE"
    qualification_url LAYERX_QUALIFICATION_NODE_URL LAYERX_BETA_QUALIFICATION_NODE_URL "$NODE_URL" \
        "beta_driver.py --node-url: the core boundary Service layerx-pending-core (node readiness, state and receipts)"
    qualification_url LAYERX_QUALIFICATION_AGENT_URL LAYERX_BETA_QUALIFICATION_AGENT_URL "" \
        "beta_driver.py --agentd-url requires agentd; no hosted agentd Service is declared"
    qualification_url LAYERX_QUALIFICATION_HUMAN_URL LAYERX_BETA_QUALIFICATION_HUMAN_URL "$HUMAN_URL" \
        "beta_driver.py --human-service-url: layerx-human HTTPS API; /readyz verifies all production components"
    qualification_url LAYERX_QUALIFICATION_HUMAN_URL LAYERX_BETA_QUALIFICATION_HUMAN_URL "" \
        "beta_driver.py --human-service-url requires Human; no hosted Human Service is declared"
    qualification_url LAYERX_QUALIFICATION_PAXEER_URL LAYERX_BETA_QUALIFICATION_PAXEER_URL "$PAXEER_URL" \
        "beta_driver.py --paxeer-testnet-url: the Paxeer boundary Service paxeer-boundary (JSON-RPC relay to the chain $PAXEER_CHAIN_ID node)"
}

identity_write() {
    local name canonical ref id fingerprint server
    fingerprint=$(openssl x509 -in "$CA_DIR/ca.crt" -noout -fingerprint -sha256 | sed 's/^.*=//')
    server=$(kube version -o json 2>/dev/null | jq -r '.serverVersion.gitVersion // "unknown"')
    {
        printf 'cluster_mode=%s\n' "$(cluster_mode)"
        printf 'cluster_name=%s\n' "$CLUSTER_NAME"
        printf 'kube_context=%s\n' "$(kube config current-context 2>/dev/null || printf unknown)"
        printf 'kube_server_version=%s\n' "$server"
        printf 'kind_cni=%s\n' "$([ "$(cluster_mode)" = kind ] && printf '%s' "$KIND_CNI" || printf owner)"
        printf 'revision=%s\n' "$REVISION"
        printf 'internal_ca_sha256=%s\n' "$fingerprint"
        printf 'sequencer_key_source=%s\n' "$SEQUENCER_KEY_SOURCE"
        printf 'sequencer_public_key=%s\n' "$(cat "$CA_DIR/sequencer.pub.hex")"
        printf 'sequencer_id=%s\n' "$SEQUENCER_ID"
        printf 'receipt_authority_replica_id=%s\n' "$(cat "$SECRETS_DIR/receipt-authority-replica-id")"
        printf 'node_network_id=%s\n' "$NODE_NETWORK_ID"
        printf 'node_asset_id=%s\n' "$NODE_ASSET_ID"
        printf 'genesis_guarantor_id=%s\n' "$NODE_GUARANTOR_ID"
        printf 'paxeer_chain_id=%s\n' "$PAXEER_CHAIN_ID"
        printf 'paxeer_deployer=%s\n' "$(cat "$SECRETS_DIR/paxeer-deployer.address")"
        printf 'paxeer_guarantor_bond=%s\n' "$GUARANTOR_BOND"
        printf 'paxeer_checkpoint_registry=%s\n' "$CHECKPOINT_REGISTRY"
        printf 'paxeer_blueprint=%s\n' "$(jq -r '.blueprint' "$WORK_DIR/paxeer/deployment.json")"
        printf 'test_auth_token_source=%s\n' "$TEST_AUTH_SOURCE"
        printf 'test_source_did=%s\n' "$TEST_SOURCE_DID"
        printf 'test_destination_did=%s\n' "$TEST_DESTINATION_DID"
        printf 'faucet_host=%s\n' "$FAUCET_HOST"
        printf 'developer_host=%s\n' "$DEVELOPER_HOST"
        while read -r name canonical ref id; do printf 'image %s=%s %s\n' "$name" "$ref" "$id"; done < "$WORK_DIR/images"
        local input
        for input in "${MISSING_INPUTS[@]}"; do printf 'missing_input=%s\n' "$input"; done
    } > "$IDENTITY_FILE"
    printf 'beta-cluster: cluster identity\n' >&2
    sed 's/^/beta-cluster:   /' "$IDENTITY_FILE" >&2
}

boundary_checks() {
    local node script="$REPO_ROOT/platform/hosted/registry/node-provision-build-boundary.sh"
    log "boundary checks: gateway hosted boundary"
    (
        set -a
        # shellcheck disable=SC1090
        . "$ENV_FILE"
        set +a
        : "${LAYERX_RECEIPT_VERIFY_BIN:=$REPO_ROOT/platform/target/release/layerx}"
        export LAYERX_RECEIPT_VERIFY_BIN
        sh "$REPO_ROOT/platform/hosted/gateway/tests/hosted-boundary.sh"
    )
    log "boundary checks: webhooks fault injection"
    (
        set -a
        # shellcheck disable=SC1090
        . "$ENV_FILE"
        set +a
        export PATH="$TOOLS_DIR:$PATH"
        cd "$REPO_ROOT"
        (umask 077; kube config view --raw > "$WORK_DIR/kubeconfig-developer")
        "$TOOLS_DIR/kubectl" --kubeconfig "$WORK_DIR/kubeconfig-developer" config set-context --current --namespace "$DEVELOPER_NAMESPACE" > /dev/null
        export KUBECONFIG="$WORK_DIR/kubeconfig-developer"
        bash "$REPO_ROOT/platform/hosted/webhooks/tests/fault-injection.sh"
    )
    log "boundary checks: registry node boundary provisioning"
    if [ "$(cluster_mode)" = kind ]; then
        for node in $(kind_nodes); do
            case "$node" in *control-plane*) continue ;; esac
            docker exec -e LAYERX_REGISTRY_MAX_BUILDS=4 -e LAYERX_REGISTRY_BUILD_QUOTA_BYTES=5368709120 -e LAYERX_REGISTRY_BUILD_QUOTA_INODES=65536 \
                "$node" /usr/libexec/layerx/node-provision-build-boundary.sh
            docker exec "$node" sh -c 'test "$(stat -c %u:%g /sys/fs/cgroup/layerx-program-registry)" = 4030:4030 && mountpoint -q /var/lib/layerx-program-registry-builds/slot-0'
        done
    else
        for node in $(kube get nodes -l "$BOUNDARY_LABEL=v1" -o name); do
            kube debug "$node" --profile=sysadmin --image=busybox:1.37.0 --quiet -- chroot /host sh -c \
                'LAYERX_REGISTRY_MAX_BUILDS=4 LAYERX_REGISTRY_BUILD_QUOTA_BYTES=5368709120 LAYERX_REGISTRY_BUILD_QUOTA_INODES=65536 /usr/libexec/layerx/node-provision-build-boundary.sh && mountpoint -q /var/lib/layerx-program-registry-builds/slot-0'
        done
    fi
    log "boundary checks passed ($script exercised on every registry node)"
}

custody_profile_validate() {
    [ -n "$CUSTODY_PROFILE" ] || return 0
    [ -f "$CUSTODY_PROFILE" ] && [ ! -L "$CUSTODY_PROFILE" ] && [ -r "$CUSTODY_PROFILE" ] \
        || fail "LAYERX_BETA_CUSTODY_PROFILE must name a readable regular file, not a symlink"
    [ "$(stat -c %s "$CUSTODY_PROFILE")" -eq 207 ] \
        || fail "LAYERX_BETA_CUSTODY_PROFILE must contain exactly 207 bytes"
    CUSTODY_PROFILE=$(readlink -f "$CUSTODY_PROFILE")
}

require_foundry() {
    [ -x "$FOUNDRY_BIN/forge" ] && [ -x "$FOUNDRY_BIN/cast" ] || fail "pinned forge and cast are not installed at $FOUNDRY_BIN (LAYERX_BETA_FOUNDRY_BIN); deploy-contracts.sh needs them"
}

beta_cluster_up() {
    local run_boundary_checks=$1
    require_tool docker curl openssl jq python3 git sha256sum tar base64
    require_foundry
    custody_profile_validate
    MISSING_INPUTS=()
    REVISION=$(revision)
    mkdir -p "$WORK_DIR" "$LOG_DIR"
    PULL_POLICY=IfNotPresent
    [ "$(cluster_mode)" = owner ] && PULL_POLICY=Always
    preflight_disk
    tools_install
    build_images
    cluster_create
    load_images
    node_boundary_install
    ca_generate
    secrets_generate
    secrets_apply
    manifests_render
    builder_release_publish
    trusted_boundary_apply
    TESTNET_URL="https://localhost:$TESTNET_PORT"
    GATEWAY_URL="https://localhost:$GATEWAY_PORT"
    FAUCET_URL="https://localhost:$FAUCET_PORT"
    DEVELOPER_URL="https://localhost:19450"
    NODE_URL="https://localhost:19446"
    AGENT_URL="https://localhost:19447"
    PAXEER_URL="https://localhost:19449"
    PAXEER_OBSERVER_URL="https://localhost:19452"
    IDENTITY_URL="https://localhost:$IDENTITY_PORT"
    HUMAN_URL="https://localhost:19453"
    wait_for_pod_ready "$TESTNET_NAMESPACE" app=paxeer 600
    port_forward paxeer-boundary "$TESTNET_NAMESPACE" paxeer-boundary 19449 9443
    port_forward paxeer-observer-boundary "$TESTNET_NAMESPACE" paxeer-observer-boundary 19452 9443
    paxeer_origins_write
    wait_for_node_genesis
    paxeer_contracts_deploy
    settlement_publish
    wait_for_pod_ready "$TESTNET_NAMESPACE" app=layerx-identity 300
    port_forward identity "$TESTNET_NAMESPACE" layerx-identity "$IDENTITY_PORT" 9443
    identity_provision
    manifests_apply
    port_forward testnet "$TESTNET_NAMESPACE" layerx-testnet-public "$TESTNET_PORT" 443
    port_forward gateway "$TESTNET_NAMESPACE" layerx-gateway "$GATEWAY_PORT" 443
    port_forward faucet "$TESTNET_NAMESPACE" layerx-faucet-public "$FAUCET_PORT" 443
    wait_for_pod_ready "$TESTNET_NAMESPACE" app=layerx-gateway 600
    internal_principals_provision
    internal_apply
    port_forward human "$TESTNET_NAMESPACE" layerx-human 19453 9443
    port_forward developer "$DEVELOPER_NAMESPACE" layerx-webhooks 19450 443
    port_forward pending-core "$TESTNET_NAMESPACE" layerx-pending-core 19446 9443
    port_forward agent-boundary "$TESTNET_NAMESPACE" layerx-agent-boundary 19447 9443
    env_write
    identity_write
    wait_ready || fail "beta cluster did not reach journey readiness; see the missing owner inputs above"
    log "every journey ready: $(jq -r '[.journeys[] | .journey] | join(",")' "$WORK_DIR/readyz.json")"
    log "environment exported to $ENV_FILE"
    if [ "$run_boundary_checks" = 1 ]; then boundary_checks; fi
}

beta_cluster_down() {
    require_tool docker git
    local mode name canonical ref id
    mode=$(state_get mode || true)
    port_forwards_stop
    if [ -x "$TOOLS_DIR/kind" ] && [ "${mode:-kind}" = kind ] && "$TOOLS_DIR/kind" get clusters 2>/dev/null | grep -qx "$CLUSTER_NAME"; then
        log "deleting kind cluster $CLUSTER_NAME"
        "$TOOLS_DIR/kind" delete cluster --name "$CLUSTER_NAME"
    elif [ "$mode" = owner ] && [ -x "$TOOLS_DIR/kubectl" ] && [ -r "${LAYERX_BETA_KUBECONFIG:-/nonexistent}" ]; then
        KUBECONFIG_FILE=$LAYERX_BETA_KUBECONFIG
        log "deleting beta namespaces from the owner cluster"
        kube delete namespace "$TESTNET_NAMESPACE" "$DEVELOPER_NAMESPACE" "$INTERNAL_NAMESPACE" --ignore-not-found --wait=true > /dev/null
    fi
    if [ -f "$WORK_DIR/images" ]; then
        while read -r name canonical ref id; do
            docker image rm --force "$ref" > /dev/null 2>&1 || true
        done < "$WORK_DIR/images"
    fi
    docker image prune --force --filter "label=$IMAGE_LABEL=$CLUSTER_NAME" > /dev/null 2>&1 || true
    docker image ls --filter "label=$IMAGE_LABEL=$CLUSTER_NAME" --format '{{.ID}}' | sort -u | xargs -r docker image rm --force > /dev/null 2>&1 || true
    rm -rf "$WORK_DIR"
    if [ "${LAYERX_BETA_KEEP_TOOLS:-0}" != 1 ]; then
        rm -f "$TOOLS_DIR/kind" "$TOOLS_DIR/kubectl" "$TOOLS_DIR"/calico-*.yaml
        rmdir "$TOOLS_DIR" 2>/dev/null || true
    fi
    log "teardown complete"
}

beta_cluster_render() {
    require_tool openssl jq python3 git
    require_foundry
    custody_profile_validate
    MISSING_INPUTS=()
    REVISION=$(revision)
    PULL_POLICY=IfNotPresent
    mkdir -p "$WORK_DIR"
    : > "$WORK_DIR/images"
    local name canonical dockerfile
    for name in "${IMAGE_NAMES[@]}"; do
        read -r canonical dockerfile <<<"$(image_source "$name")"
        [ -f "$REPO_ROOT/$dockerfile" ] || fail "missing $dockerfile"
        printf '%s %s %s unbuilt\n' "$name" "$canonical" "$(image_ref "$name")" >> "$WORK_DIR/images"
    done
    ca_generate
    secrets_generate
    manifests_render
    log "rendered manifests under $MANIFESTS_DIR and beta CA under $CA_DIR (nothing applied)"
}

main() {
    local command=${1:-} boundary=0
    shift || true
    case "$command" in
        images)
            require_tool docker git tar
            REVISION=$(revision)
            preflight_disk
            build_images
            ;;
        up)
            for argument in "$@"; do
                case "$argument" in
                    --boundary-checks) boundary=1 ;;
                    *) fail "unknown argument $argument" ;;
                esac
            done
            beta_cluster_up "$boundary"
            ;;
        test-guarantor-sequences) guarantor_sequence_test ;;
        down) beta_cluster_down ;;
        render) beta_cluster_render ;;
        *) sed -n '2,41p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2; exit 64 ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
