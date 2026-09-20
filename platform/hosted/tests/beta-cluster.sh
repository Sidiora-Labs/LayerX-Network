#!/usr/bin/env bash
# LayerX beta cluster bring-up and teardown.
#
# Usage:
#   beta-cluster.sh up [--boundary-checks]
#   beta-cluster.sh down
#   beta-cluster.sh render
#   beta-cluster.sh images
#   beta-cluster.sh publish-images <check|dry-run|push|promote|self-test> [publisher options]
#
# publish-images names the mode of platform/hosted/tests/publish-images.sh on behalf of its caller: check and
# self-test reach no registry, dry-run resolves the release binding and writes the publication plan, push
# publishes the immutable revision tag of every image and promote repoints the release and the moving beta
# tags once every published digest verifies. Options after the mode are forwarded to the publisher unchanged.
#
# Inputs (environment variables, all optional unless stated):
#   LAYERX_BETA_KUBECONFIG              owner cluster kubeconfig; unset selects a disposable local kind cluster
#   LAYERX_BETA_CLUSTER_NAME            kind cluster name (default layerx-beta)
#   LAYERX_BETA_IMAGE_REGISTRY          registry prefix images are pushed to for an owner cluster; unset loads
#                                       the locally built images into the kind nodes
#   LAYERX_BETA_BUILDER_ENVIRONMENT_DIR optional hermetic builder root filesystem (regular files and directories
#                                       only, entrypoint bin/layerx-build) published into the
#                                       layerx-program-builder-release volume the program registry mounts. Unset,
#                                       up constructs that root filesystem itself from the committed recipe with
#                                       platform/hosted/registry/builder-environment/build-env.sh and uses the
#                                       rootfs of the constructed entry, which needs docker and an uncommitted
#                                       change free programs/vendor and recipe tree
#   LAYERX_BETA_BUILDER_ENVIRONMENT_CACHE_DIR  directory the constructed builder environments live in, one entry
#                                       per committed recipe tree, reused by a later up and not deleted by down
#                                       (default build/builder-environment)
#   LAYERX_BETA_SEQUENCER_KEY_FILE      PEM ed25519 private key of the beta sequencer; generated when unset. Its
#                                       32-byte seed is the node's sequencer key, so the node, the receipt
#                                       authority, the gateway and the registry share one sequencer identity
#   LAYERX_BETA_FOUNDRY_BIN             directory holding the pinned forge and cast that
#                                       platform/hosted/paxeer/deploy-contracts.sh requires (default /root/.foundry/bin)
#   LAYERX_BETA_FAUCET_HOST             public faucet hostname (default faucet.testnet.layerx.network)
#   LAYERX_BETA_DEVELOPER_HOST          public developer hostname (default developers.testnet.layerx.network)
#   LAYERX_BETA_RELAY_HOST              public relay/archive hostname (default relay.testnet.layerx.network)
#   LAYERX_BETA_RELAY_UPSTREAM          comma-separated public HTTPS relay/archive origins the beta relay reads
#                                       canonical history from; unset leaves the colocated canonical availability
#                                       log of the sequencer node as its only source
#   LAYERX_BETA_RELAY_SUBMISSION_UPSTREAM  comma-separated public HTTPS endpoints the relay forwards original
#                                       signed activities to (origin, /v1/activities or /rpc). Cluster-internal
#                                       addresses are refused by the relay address policy in
#                                       platform/relay_archive/protocol.py, so this endpoint has to be a public
#                                       one; unset leaves POST /v1/activities without a forwarding destination
#                                       and the bring-up records the missing owner input
#   LAYERX_BETA_RELAY_PEER_SEED         comma-separated public HTTPS relay/archive peer-discovery seeds; unset
#                                       keeps peer discovery disabled
#   LAYERX_BETA_KIND_CNI                calico (default, enforces NetworkPolicy) or kindnet
#   LAYERX_BETA_READY_TIMEOUT           seconds to wait for every journey to report ready (default 900)
#   LAYERX_BETA_MIN_FREE_GIB            free disk required before building the images of a local cluster
#                                       bring-up (default 24)
#   LAYERX_BETA_IMAGE_MIN_FREE_GIB      free disk required by `beta-cluster.sh images`, which builds the image
#                                       set without creating a cluster or loading it into kind nodes; a job
#                                       that only builds and publishes images sets its own bound here
#                                       (default LAYERX_BETA_MIN_FREE_GIB)
#   LAYERX_BETA_INTEROP_CONFORMANCE_X402  optional overrides of the five first-party adapter conformance
#   LAYERX_BETA_INTEROP_CONFORMANCE_AP2   suites derived from interop/specs/conformance, whose identifier,
#   LAYERX_BETA_INTEROP_CONFORMANCE_UCP   vector count and SHA-256 come from the very vector files the
#   LAYERX_BETA_INTEROP_CONFORMANCE_VISA_TAP  adapter tests read, as
#   LAYERX_BETA_INTEROP_CONFORMANCE_FIAT  '<suite-identifier>,<vector-count>,<suite-sha256>'. No upstream
#                                       publishes a suite for UCP, Visa TAP or the fiat provider callbacks
#                                       (interop/specs/vendor/CONFORMANCE.md), so unset means derived from
#                                       this checkout and a declared value pins an imported suite instead
#   LAYERX_BETA_INTEROP_CONFORMANCE_HTTP  optional overrides of the three first-party transport binding
#   LAYERX_BETA_INTEROP_CONFORMANCE_MCP   suites, as '<suite-sha256>'; unset derives the digest from
#   LAYERX_BETA_INTEROP_CONFORMANCE_A2A   interop/specs/conformance/transport-<binding>, and the binding
#                                       version and specification digest are derived from
#                                       interop/specs/vendor/x402/transports
#   LAYERX_BETA_INTEROP_AP2_KEYS        optional JSON counterparty trust roots that pin real external
#   LAYERX_BETA_INTEROP_AP2_ASSETS      counterparties: the AP2 mandate issuer keys and asset bindings, the
#   LAYERX_BETA_INTEROP_VISA_AGENTS     Visa TAP agent registry keys and merchant targets, and the fiat
#   LAYERX_BETA_INTEROP_VISA_TARGETS    provider callback keys. Unset, the bring-up generates them for this
#   LAYERX_BETA_INTEROP_FIAT_PROVIDERS  testnet's own test clients and names them layerx-beta-*
#                                       (interop/deploy/gateway/README.md)
#   LAYERX_BETA_INTEROP_X402_SUPPORTED  optional overrides of the two in-cluster trust roots, which default
#   LAYERX_BETA_INTEROP_UCP_PAYMENT_HANDLER  to this cluster's own facilitator declaration and the UCP
#                                       payment handler of the vendored UCP revision
#   LAYERX_BETA_INTEROP_MANIFEST_FILE   optional interop gateway manifest that overrides any rendered field
#                                       (interop/deploy/gateway/README.md)
#   LAYERX_BETA_TESTNET_PORT            host ports of the testnet, gateway and faucet port-forwards
#   LAYERX_BETA_GATEWAY_PORT            (defaults 19443, 19444, 19445)
#   LAYERX_BETA_FAUCET_PORT
#   LAYERX_BETA_HUMAN_WEB_PORT          443 (default) or empty. The browser origin of the human web application
#                                       is https://human.testnet.layerx.network and carries no port, so the
#                                       passkey ceremony configuration, the human service allowed origin and
#                                       human/apps/web/e2e/software-authenticator.ts only accept it on 443, and
#                                       the owner adds `127.0.0.1 human.testnet.layerx.network` to /etc/hosts and
#                                       trusts the beta internal CA. Set it empty on a host where
#                                       human/apps/web/e2e/run-production-browser.sh serves that same origin from
#                                       its own authbind listener, which leaves the forward and its readiness
#                                       gate out; any other value is refused
#   LAYERX_BETA_RAMP_PORT               host port of the reference ramp port-forward (default 19459)
#   LAYERX_BETA_RAMP_WORKER_ID          reference ramp worker identity (default layerx-beta-ramp-1)
#   LAYERX_BETA_RAMP_FEE_LIMIT          LayerX activity fee limit of the ramp operator (default 1000)
#   The reference fiat ramp is optional: when no LAYERX_BETA_RAMP_* variable is set the bring-up records
#   one missing owner input naming every ramp input and leaves the ramp image, workload, port-forward and
#   sandbox journey out; when some are set it refuses to start and names each one still missing; when all
#   are set it brings the ramp up. Values:
#   LAYERX_BETA_RAMP_OPERATOR_PRINCIPAL_ID, LAYERX_BETA_RAMP_OPERATOR_DID,
#   LAYERX_BETA_RAMP_OPERATOR_SIGNER_KEY_HANDLE, LAYERX_BETA_RAMP_PROVIDER_ENDPOINT,
#   LAYERX_BETA_RAMP_PROVIDER_CALLBACK_PUBLIC_KEY, LAYERX_BETA_RAMP_COMPLIANCE_ENDPOINT,
#   LAYERX_BETA_RAMP_COMPLIANCE_PUBLIC_KEY, LAYERX_BETA_RAMP_SIGNER_ENDPOINT,
#   LAYERX_BETA_RAMP_SIGNER_PUBLIC_KEY, LAYERX_BETA_RAMP_PAXEER_WALLET_ADDRESS,
#   LAYERX_BETA_RAMP_PAXEER_VAULT_ID, LAYERX_BETA_RAMP_PAXEER_SIGNER_KEY_HANDLE. Readable private files:
#   LAYERX_BETA_RAMP_OUTBOUND_CA_PEM_FILE, LAYERX_BETA_RAMP_OUTBOUND_IDENTITY_PKCS12_FILE,
#   LAYERX_BETA_RAMP_OUTBOUND_IDENTITY_PASSWORD_FILE, LAYERX_BETA_RAMP_PROVIDER_TOKEN_FILE,
#   LAYERX_BETA_RAMP_COMPLIANCE_TOKEN_FILE, LAYERX_BETA_RAMP_SIGNER_TOKEN_FILE,
#   LAYERX_BETA_RAMP_GATEWAY_KEY_FILE, LAYERX_BETA_RAMP_PAXEER_CUSTODY_TOKEN_FILE,
#   LAYERX_BETA_RAMP_QUOTES_FILE. The sandbox journey inputs LAYERX_BETA_RAMP_CUSTOMER_TOKEN,
#   LAYERX_BETA_RAMP_OFF_GRANT_JSON, LAYERX_BETA_RAMP_ON_ACCOUNT_SEQUENCE and
#   LAYERX_BETA_RAMP_OFF_RECEIVER_SEQUENCE are recorded as missing inputs when unset; setting any of them,
#   or LAYERX_BETA_RAMP_ON_QUOTE_ID or LAYERX_BETA_RAMP_OFF_QUOTE_ID, also asks for the ramp
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
#   LAYERX_BETA_STATUS_PUBLISH_URL      https endpoint of the separately operated status publisher that the
#                                       layerx-testnet-status-publisher CronJob PUTs the testnet status to;
#                                       unset drops that CronJob from the applied manifests and reports the
#                                       variable as a missing owner input
#   LAYERX_BETA_EXPLORER_PROBE_PROGRAM  optional override of the program the explorer index probes before it
#                                       serves; unset reads the program of the reference escrow deployment
#                                       the bring-up publishes to the program registry
#   LAYERX_BETA_EXPLORER_NAMING_PROGRAM optional override of the naming program the Human web application
#                                       resolves names through; unset reads the program of the reference
#                                       naming deployment the bring-up publishes to the program registry
#   LAYERX_BETA_KEEP_TOOLS              set to 1 to keep the pinned kind/kubectl downloads on teardown
#
# Mirror publisher inputs. The bring-up always publishes every LayerX batch archive to an EVM chain; it
# additionally publishes to Solana when the Solana inputs below are set, and mirrors to the EVM chain alone
# when none of them is, recording the Solana inputs as a missing owner input. The publisher and its signer
# are containers of the layerx-node pod, so they reach the node LNI socket and the signer socket over pod
# volumes and need no host paths. The publisher keys live in the layerx-mirror-signer Secret this script
# generates and are served by interop/crates/layerx-mirror-signer under the fixed handles
# mirror/ethereum/beta and mirror/solana/beta, so no owner-operated signer service is involved.
#
# Solana mirror inputs (optional as a group: set all of them to mirror to Solana, none to mirror to the EVM
# chain only; setting some but not all is refused by name):
#   LAYERX_BETA_MIRROR_SOLANA_RPC_URL   two independent Solana JSON-RPC backends the publisher reaches a
#   LAYERX_BETA_MIRROR_SOLANA_RPC_URL_SECONDARY      strict majority across
#   LAYERX_BETA_MIRROR_SOLANA_RPC_CA_FILE            PEM trust anchor for both Solana endpoints
#   LAYERX_BETA_MIRROR_SOLANA_RPC_TOKEN_FILE         bearer token file of each Solana endpoint
#   LAYERX_BETA_MIRROR_SOLANA_RPC_SECONDARY_TOKEN_FILE
#   LAYERX_BETA_MIRROR_SOLANA_KEYPAIR_FILE           64-byte Solana keypair file of the publisher; it is
#                                       the Ed25519 key material the mirror signer serves and must be the
#                                       publisher the solana-mirror deployment record names
#   LAYERX_BETA_MIRROR_SOLANA_DEPLOYMENT_FILE        optional deployment record of the solana-mirror
#                                       program carrying every field solana-deployment.json requires; unset
#                                       builds and deploys the program with deploy-solana-mirror.sh and
#                                       writes the record from that deployment
#   LAYERX_BETA_MIRROR_SOLANA_DEPLOY_RPC_URL         endpoint deploy-solana-mirror.sh broadcasts the program
#                                       deployment through (default LAYERX_BETA_MIRROR_SOLANA_RPC_URL); the
#                                       Solana CLI cannot send a bearer token, so this endpoint must carry
#                                       its own credential in its URL
#   LAYERX_BETA_MIRROR_SOLANA_TOOLCHAIN_BIN          directory holding the pinned solana, solana-keygen and
#                                       cargo-build-sbf used to build and deploy the program
#   LAYERX_BETA_MIRROR_ETHEREUM_RPC_URL when set, the mirror publishes to an owner EVM chain instead of the
#                                       in-cluster Paxeer chain and additionally requires
#                                       LAYERX_BETA_MIRROR_ETHEREUM_RPC_URL_SECONDARY,
#                                       LAYERX_BETA_MIRROR_ETHEREUM_RPC_CA_FILE,
#                                       LAYERX_BETA_MIRROR_ETHEREUM_RPC_TOKEN_FILE,
#                                       LAYERX_BETA_MIRROR_ETHEREUM_RPC_SECONDARY_TOKEN_FILE,
#                                       LAYERX_BETA_MIRROR_ETHEREUM_CHAIN_ID and
#                                       LAYERX_BETA_MIRROR_ETHEREUM_DEPLOYER_KEY_FILE
#   LAYERX_BETA_MIRROR_ETHEREUM_KEY_FILE             0x-prefixed 32-byte secp256k1 private key of the
#                                       Ethereum publisher; unset generates one and keeps it in the
#                                       layerx-mirror-signer Secret
#   LAYERX_BETA_MIRROR_ETHEREUM_SIGNER_PUBLIC_KEY    when set, the compressed secp256k1 key the Ethereum
#                                       publisher key must carry; the bring-up refuses a mismatch
#   LAYERX_BETA_MIRROR_PUBLISHER_GAS_WEI             gas the bring-up tops the Ethereum publisher up to
#                                       before the first publication (default 1000000000000000000)
#
# The trusted-boundary services (node with core boundary, receipt authority and agent boundary; identity;
# Paxeer chain with its boundary) are built from the repository, applied before the testnet, gateway,
# registry and developer manifests, and bound together in this order: the Paxeer chain starts with the
# generated deployer address, the node bootstraps its genesis, deploy-contracts.sh deploys the settlement
# contracts from the node's genesis artifacts through the Paxeer boundary, and the resulting GuarantorBond
# and CheckpointRegistry addresses are published to the node as the layerx-node-settlement ConfigMap the
# sequencer supervisor waits for before starting layerxd --serve.
#
# Two reference programs are deployed through the program registry deployment ingress before the explorer
# observation is published: programs/sdk/rust/examples/escrow, whose deployment record carries the program
# the explorer index probes before it serves, and programs/sdk/rust/examples/naming, whose program id is
# published as the naming-program key of the layerx-explorer-index ConfigMap and bound into the Human web
# container as LAYERX_EXPLORER_NAMING_PROGRAM. Both are built for wasm32-unknown-unknown from this
# repository, so that Rust target has to be installed.
#
# The explorer index resolves names by signing noncommitting reads of that naming program as its own chain
# identity. The bring-up generates that ed25519 key with the other secrets, admits did:layerx:<public key>
# to the node's identity file in the same fresh-genesis append that admits the Human owner, and publishes
# the seed and the sequencer public key in the layerx-explorer-index Secret. It is not an owner input.
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
RELAY_HOST=${LAYERX_BETA_RELAY_HOST:-relay.testnet.layerx.network}
HUMAN_WEB_HOST=human.testnet.layerx.network
TESTNET_HOST=testnet.layerx.network
GATEWAY_HOST=api.testnet.layerx.network
KIND_CNI=${LAYERX_BETA_KIND_CNI:-calico}
READY_TIMEOUT=${LAYERX_BETA_READY_TIMEOUT:-900}
MIN_FREE_GIB=${LAYERX_BETA_MIN_FREE_GIB:-24}
IMAGE_MIN_FREE_GIB=${LAYERX_BETA_IMAGE_MIN_FREE_GIB:-$MIN_FREE_GIB}
TESTNET_PORT=${LAYERX_BETA_TESTNET_PORT:-19443}
GATEWAY_PORT=${LAYERX_BETA_GATEWAY_PORT:-19444}
FAUCET_PORT=${LAYERX_BETA_FAUCET_PORT:-19445}
HUMAN_WEB_PORT=${LAYERX_BETA_HUMAN_WEB_PORT-443}
RAMP_PORT=${LAYERX_BETA_RAMP_PORT:-19459}
RAMP_HOST=ramp.testnet.layerx.network
RAMP_WORKER_ID=${LAYERX_BETA_RAMP_WORKER_ID:-layerx-beta-ramp-1}
RAMP_FEE_LIMIT=${LAYERX_BETA_RAMP_FEE_LIMIT:-1000}
RAMP_ENABLED=1
TESTNET_NAMESPACE=layerx-testnet
DEVELOPER_NAMESPACE=layerx-developer
IMAGE_LABEL=io.layerx.beta-cluster
BOUNDARY_LABEL=layerx.io/program-registry-boundary

source "$SCRIPT_DIR/beta-images.sh"
source "$REPO_ROOT/platform/hosted/human/provision.sh"
TRUSTED_BOUNDARY_SERVICES=(layerx-pending-core layerx-pending-core-admin paxeer-boundary layerx-identity layerx-receipt-authority layerx-agent-boundary)
INTERNAL_NAMESPACE=layerx-internal
FOUNDRY_BIN=${LAYERX_BETA_FOUNDRY_BIN:-/root/.foundry/bin}
BUILDER_ENVIRONMENT_RECIPE=platform/hosted/registry/builder-environment
BUILDER_ENVIRONMENT_CACHE_DIR=${LAYERX_BETA_BUILDER_ENVIRONMENT_CACHE_DIR:-$REPO_ROOT/build/builder-environment}
CUSTODY_PROFILE=${LAYERX_BETA_CUSTODY_PROFILE:-}
STATUS_PUBLISH_URL=${LAYERX_BETA_STATUS_PUBLISH_URL:-}
STATUS_PUBLISHER_REPORTED=0
EXPLORER_OBSERVATION_PUBLISHED=0
IDENTITY_PORT=19451
INTEROP_PORT=19458
EXPLORER_INDEX_PORT=19460
RAMP_VALUE_INPUTS=(LAYERX_BETA_RAMP_OPERATOR_PRINCIPAL_ID LAYERX_BETA_RAMP_OPERATOR_DID
    LAYERX_BETA_RAMP_OPERATOR_SIGNER_KEY_HANDLE LAYERX_BETA_RAMP_PROVIDER_ENDPOINT
    LAYERX_BETA_RAMP_PROVIDER_CALLBACK_PUBLIC_KEY LAYERX_BETA_RAMP_COMPLIANCE_ENDPOINT
    LAYERX_BETA_RAMP_COMPLIANCE_PUBLIC_KEY LAYERX_BETA_RAMP_SIGNER_ENDPOINT LAYERX_BETA_RAMP_SIGNER_PUBLIC_KEY
    LAYERX_BETA_RAMP_PAXEER_WALLET_ADDRESS LAYERX_BETA_RAMP_PAXEER_VAULT_ID
    LAYERX_BETA_RAMP_PAXEER_SIGNER_KEY_HANDLE)
RAMP_FILE_INPUTS=(LAYERX_BETA_RAMP_OUTBOUND_CA_PEM_FILE LAYERX_BETA_RAMP_OUTBOUND_IDENTITY_PKCS12_FILE
    LAYERX_BETA_RAMP_OUTBOUND_IDENTITY_PASSWORD_FILE LAYERX_BETA_RAMP_PROVIDER_TOKEN_FILE
    LAYERX_BETA_RAMP_COMPLIANCE_TOKEN_FILE LAYERX_BETA_RAMP_SIGNER_TOKEN_FILE LAYERX_BETA_RAMP_GATEWAY_KEY_FILE
    LAYERX_BETA_RAMP_PAXEER_CUSTODY_TOKEN_FILE LAYERX_BETA_RAMP_QUOTES_FILE)
RAMP_OPTIONAL_INPUTS=(LAYERX_BETA_RAMP_PORT LAYERX_BETA_RAMP_WORKER_ID LAYERX_BETA_RAMP_FEE_LIMIT
    LAYERX_BETA_RAMP_ON_QUOTE_ID LAYERX_BETA_RAMP_OFF_QUOTE_ID LAYERX_BETA_RAMP_CUSTOMER_TOKEN
    LAYERX_BETA_RAMP_OFF_GRANT_JSON LAYERX_BETA_RAMP_ON_ACCOUNT_SEQUENCE
    LAYERX_BETA_RAMP_OFF_RECEIVER_SEQUENCE)
PAXEER_CHAIN_ID=125
MIRROR_SIGNER_SOCKET=/run/mirror-signer/signer.sock
MIRROR_ETHEREUM_KEY_HANDLE=mirror/ethereum/beta
MIRROR_SOLANA_KEY_HANDLE=mirror/solana/beta
MIRROR_SOLANA=0
MIRROR_SOLANA_INPUTS=(LAYERX_BETA_MIRROR_SOLANA_RPC_URL LAYERX_BETA_MIRROR_SOLANA_RPC_URL_SECONDARY
    LAYERX_BETA_MIRROR_SOLANA_RPC_CA_FILE LAYERX_BETA_MIRROR_SOLANA_RPC_TOKEN_FILE
    LAYERX_BETA_MIRROR_SOLANA_RPC_SECONDARY_TOKEN_FILE LAYERX_BETA_MIRROR_SOLANA_KEYPAIR_FILE)
NODE_MANIFEST="$REPO_ROOT/platform/hosted/node/deployment.yaml"
NODE_NETWORK_ID=$(sed -n 's/^  network-id: "\([0-9]*\)"$/\1/p' "$NODE_MANIFEST")
NODE_ASSET_ID=$(sed -n 's/^  asset-id: "\([0-9a-f]*\)"$/\1/p' "$NODE_MANIFEST")
PAXEER_RELAY_PORT=$(sed -n 's/^  paxeer-relay-port: "\([0-9]*\)"$/\1/p' "$NODE_MANIFEST")
GUARANTOR_CHECKPOINT_AUTHORITY_KEY_FILE=$(sed -n 's/^ *- {name: LAYERX_GUARANTOR_CHECKPOINT_AUTHORITY_KEY_FILE, value: \([^}]*\)}$/\1/p' "$NODE_MANIFEST" | sort -u)
NODE_DATA_DIR=/var/lib/layerx/node
RELAY_NODE_SOURCE_DIR=/var/lib/layerx/node-source/node
RELAY_DATA_DIR=/var/lib/layerx/relay-archive
RELAY_TLS_DIR=/run/layerx/tls
RELAY_CODEC=/usr/local/bin/layerx-archive-codec

log() { printf 'beta-cluster: %s\n' "$*" >&2; }
fail() { printf 'beta-cluster: error: %s\n' "$*" >&2; exit 1; }

require_tool() {
    local tool
    for tool in "$@"; do
        command -v "$tool" >/dev/null 2>&1 || fail "required host tool '$tool' is not installed"
    done
}

revision() {
    local rev
    case "${LAYERX_BETA_IMAGE_SOURCE:-build}" in
        build) ;;
        ghcr)
            rev=${LAYERX_BETA_IMAGE_TAG:-beta}
            [[ $rev =~ ^[a-zA-Z0-9_][a-zA-Z0-9_.-]{0,127}$ ]] || fail "invalid LAYERX_BETA_IMAGE_TAG"
            printf '%s' "$rev"
            return
            ;;
        *) fail "LAYERX_BETA_IMAGE_SOURCE must be build or ghcr" ;;
    esac
    rev=$(git -C "$REPO_ROOT" rev-parse --short=12 HEAD)
    if [ -n "$(git -C "$REPO_ROOT" status --porcelain --untracked-files=no -- platform)" ]; then
        rev="$rev-dirty"
    fi
    printf '%s' "$rev"
}

image_ref() {
    local registry=${LAYERX_BETA_IMAGE_REGISTRY:-layerx-beta}
    if [ "${LAYERX_BETA_IMAGE_SOURCE:-build}" = ghcr ]; then registry=layerx-beta; fi
    printf '%s/%s:%s' "$registry" "$1" "$REVISION"
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
    local required=$1 bound=$2 purpose=$3 docker_root free dir
    [[ $required =~ ^[0-9]+$ ]] || fail "$bound must be a whole number of GiB, got '$required'"
    docker_root=$(docker info --format '{{.DockerRootDir}}')
    for dir in "$REPO_ROOT/build" "$docker_root"; do
        free=$(free_gib "$dir")
        if [ "$free" -lt "$required" ]; then
            fail "insufficient free disk under $dir: ${free} GiB free, ${required} GiB required ($bound) to $purpose"
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

image_selected() {
    # image_selected NAME: an image whose workload this bring-up leaves out is not built or pulled
    case "$1" in
        layerx-reference-ramp) [ "$RAMP_ENABLED" = 1 ] ;;
        *) return 0 ;;
    esac
}

build_images() {
    local name canonical dockerfile ref id
    local -a build_args
    mkdir -p "$LOG_DIR"
    : > "$WORK_DIR/images"
    build_context
    for name in "${IMAGE_NAMES[@]}"; do
        image_selected "$name" || continue
        read -r canonical dockerfile <<<"$(image_source "$name")"
        ref=$(image_ref "$name")
        read -r -a build_args <<<"$(image_build_args "$name")"
        log "building $ref from $dockerfile"
        docker build --file "$dockerfile" --tag "$ref" --label "$IMAGE_LABEL=$CLUSTER_NAME" "${build_args[@]}" - < "$WORK_DIR/context.tar" \
            > "$LOG_DIR/build-$name.log" 2>&1 || { tail -n 40 "$LOG_DIR/build-$name.log" >&2; fail "image build failed for $name (log $LOG_DIR/build-$name.log)"; }
        id=$(docker image inspect --format '{{.Id}}' "$ref")
        if [ "$name" = paxd ]; then
            docker run --rm --entrypoint /bin/bash "$ref" -c \
                'test -r /opt/layerx/init-chain.sh && test -s /opt/layerx/contracts/BetaUsdl.runtime.hex && command -v paxd && command -v jq' \
                > "$LOG_DIR/contents-$name.log" 2>&1 || fail "Paxeer initialization image is incomplete"
        fi
        printf '%s %s %s %s\n' "$name" "$canonical" "$ref" "$id" >> "$WORK_DIR/images"
    done
}

pull_images() {
    local name canonical dockerfile remote digest ref id expected publication candidate
    publication=${LAYERX_BETA_PUBLICATION_DIR:?verified release publication directory is required for GHCR images}
    candidate=${LAYERX_BETA_RELEASE_CANDIDATE:?the exact source revision of the GHCR images is required}
    bash "$SCRIPT_DIR/publish-images.sh" --phase verify --release-candidate "$candidate" --output "$publication"
    mkdir -p "$LOG_DIR"
    : > "$WORK_DIR/images"
    for name in "${IMAGE_NAMES[@]}"; do
        image_selected "$name" || continue
        read -r canonical dockerfile <<<"$(image_source "$name")"
        remote="ghcr.io/sidiora-labs/$name"
        digest=$(registry_image_digest "$remote:$REVISION")
        expected=$(awk -v name="$name" -v repository="$remote" '$1 == name && $2 == repository {print $3}' "$publication/digests.txt")
        [ "$digest" = "$expected" ] || fail "selected tag does not name the attested release image for $name"
        log "pulling $remote:$REVISION at $digest"
        docker pull "$remote@$digest" > "$LOG_DIR/pull-$name.log" 2>&1 \
            || fail "image pull failed for $name (log $LOG_DIR/pull-$name.log)"
        docker image inspect "$remote@$digest" --format '{{json .RepoDigests}}' \
            | jq -e --arg expected "$remote@$digest" 'index($expected) != null' >/dev/null \
            || fail "pulled digest differs from registry manifest for $name"
        ref=$(image_ref "$name")
        docker tag "$remote@$digest" "$ref"
        id=$(docker image inspect --format '{{.Id}}' "$ref")
        printf '%s %s %s %s\n' "$name" "$canonical" "$ref" "$id" >> "$WORK_DIR/images"
        printf '%s %s\n' "$remote:$REVISION" "$digest" >> "$LOG_DIR/pulled-digests.log"
    done
}

prepare_images() {
    case "${LAYERX_BETA_IMAGE_SOURCE:-build}" in
        build) build_images ;;
        ghcr) require_tool jq; pull_images ;;
        *) fail "LAYERX_BETA_IMAGE_SOURCE must be build or ghcr" ;;
    esac
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
    local name canonical ref id node normalized digest pin observed repository
    : > "$WORK_DIR/image-pins"
    while read -r name canonical ref id; do
        if [ "$(cluster_mode)" = owner ]; then
            [ "${LAYERX_BETA_IMAGE_SOURCE:-build}" = ghcr ] \
                || fail "owner clusters require release images published through the GHCR SBOM and attestation gate"
            repository="ghcr.io/sidiora-labs/$name"
            pin=$(docker image inspect "$ref" --format '{{json .RepoDigests}}' \
                | jq -er --arg repository "$repository@" '[.[] | select(startswith($repository))] | unique | if length == 1 then .[0] else error("missing unique registry digest") end') \
                || fail "no immutable GHCR reference for $name"
        else
            log "loading $ref into kind nodes"
            "$TOOLS_DIR/kind" load docker-image --name "$CLUSTER_NAME" "$ref" > "$LOG_DIR/load-$name.log" 2>&1 \
                || fail "kind load failed for $ref"
            normalized=$ref
            case "${ref%%/*}" in *.*|*:*|localhost) ;; *) normalized="docker.io/$ref" ;; esac
            digest=
            for node in $(kind_nodes); do
                observed=$(docker exec "$node" ctr -n k8s.io images ls \
                    | awk -v reference="$normalized" '$1 == reference {print $3}')
                [[ $observed =~ ^sha256:[0-9a-f]{64}$ ]] || fail "kind did not retain a manifest digest for $ref"
                [ -z "$digest" ] || [ "$digest" = "$observed" ] || fail "kind nodes loaded different manifests for $ref"
                digest=$observed
                pin="${normalized%:*}@$digest"
                docker exec "$node" ctr -n k8s.io images tag --force "$normalized" "$pin" > /dev/null \
                    || fail "kind could not retain immutable reference $pin"
            done
            [ -n "$digest" ] || fail "no kind node loaded $ref"
        fi
        [[ $pin =~ @sha256:[0-9a-f]{64}$ ]] || fail "invalid immutable image reference for $name"
        printf '%s %s\n' "$name" "$pin" >> "$WORK_DIR/image-pins"
    done < "$WORK_DIR/images"
}

node_boundary_install() {
    local node script unit
    script="$REPO_ROOT/platform/hosted/registry/node-provision-build-boundary.sh"
    unit="$REPO_ROOT/platform/hosted/registry/layerx-program-registry-boundary.service"
    if [ "$(cluster_mode)" = owner ]; then
        if [ -z "$(kube get nodes -l "$BOUNDARY_LABEL=v2" -o name)" ]; then
            fail "no node of the owner cluster carries $BOUNDARY_LABEL=v2; the owner installs $unit and $script on the registry nodes before labelling them"
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
        docker exec "$node" mountpoint -q /var/lib/layerx-program-registry-builds/slot-0
        kube label node "$node" "$BOUNDARY_LABEL=v2" --overwrite > /dev/null
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
        if [ -n "$subject_alt" ]; then printf 'subjectAltName=%s\n' "$subject_alt"; fi
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

issue_server_identity() {
    local name=$1 cn=$2 subject_alt=$3 dir
    dir="$CA_DIR/$name"
    issue_cert "$name" "$cn" serverAuth "$subject_alt"
    write_token "$dir/password"
    (umask 077; openssl pkcs12 -export -inkey "$dir/key.pem" -in "$dir/cert.pem" -certfile "$CA_DIR/ca.crt" \
        -name "$cn" -passout "file:$dir/password" -out "$dir/server.p12")
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
    issue_cert human-web layerx-human-web serverAuth \
        "DNS:layerx-human-web.$svc,DNS:layerx-human-web.$TESTNET_NAMESPACE.svc,DNS:layerx-human-web,DNS:human.testnet.layerx.network,DNS:localhost,IP:127.0.0.1"
    issue_cert explorer-index layerx-explorer-index serverAuth \
        "DNS:layerx-explorer-index.$svc,DNS:layerx-explorer-index.$TESTNET_NAMESPACE.svc,DNS:layerx-explorer-index,DNS:localhost,IP:127.0.0.1"
    issue_cert faucet layerx-faucet serverAuth \
        "DNS:layerx-faucet-public.$svc,DNS:layerx-faucet-public,DNS:$FAUCET_HOST,DNS:localhost,IP:127.0.0.1"
    issue_cert registry layerx-program-registry serverAuth \
        "DNS:layerx-program-registry.$svc,DNS:layerx-program-registry"
    issue_cert relay-archive layerx-relay-archive serverAuth \
        "DNS:layerx-relay-archive.$svc,DNS:layerx-relay-archive.$TESTNET_NAMESPACE.svc,DNS:layerx-relay-archive,DNS:$RELAY_HOST,DNS:localhost,IP:127.0.0.1"
    issue_cert faucet-redis layerx-faucet-redis serverAuth "DNS:layerx-faucet-redis.$svc,DNS:layerx-faucet-redis"
    issue_cert gateway-redis layerx-gateway-redis serverAuth "DNS:layerx-gateway-redis.$svc,DNS:layerx-gateway-redis"
    issue_cert interop-gateway layerx-interop-gateway serverAuth \
        "DNS:layerx-interop-gateway.$svc,DNS:layerx-interop-gateway.$TESTNET_NAMESPACE.svc,DNS:layerx-interop-gateway,DNS:localhost,IP:127.0.0.1"
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
        "DNS:layerx-receipt-authority.$svc,DNS:layerx-receipt-authority.$TESTNET_NAMESPACE.svc,DNS:layerx-receipt-authority,DNS:authority.$internal,DNS:authority.$INTERNAL_NAMESPACE.svc,DNS:localhost,IP:127.0.0.1"
    issue_cert agent-boundary layerx-agent-boundary serverAuth \
        "DNS:layerx-agent-boundary.$svc,DNS:layerx-agent-boundary.$TESTNET_NAMESPACE.svc,DNS:layerx-agent-boundary,DNS:component.$internal,DNS:component.$INTERNAL_NAMESPACE.svc,DNS:localhost,IP:127.0.0.1"
    issue_cert agentd layerx-agentd serverAuth \
        "DNS:layerx-agentd.$svc,DNS:layerx-agentd.$TESTNET_NAMESPACE.svc,DNS:layerx-agentd,DNS:localhost,IP:127.0.0.1"
    issue_cert agentd-client layerx-agentd-client clientAuth ""
    issue_cert identity layerx-identity serverAuth \
        "DNS:layerx-identity.$svc,DNS:layerx-identity.$TESTNET_NAMESPACE.svc,DNS:layerx-identity,DNS:identity.$internal,DNS:identity.$INTERNAL_NAMESPACE.svc,DNS:localhost,IP:127.0.0.1"
    issue_cert paxeer-boundary paxeer-boundary serverAuth \
        "DNS:paxeer-boundary.$svc,DNS:paxeer-boundary.$TESTNET_NAMESPACE.svc,DNS:paxeer-boundary,DNS:paxeer-observer-boundary.$svc,DNS:paxeer-observer-boundary.$TESTNET_NAMESPACE.svc,DNS:paxeer-observer-boundary,DNS:paxeer.$svc,DNS:localhost,IP:127.0.0.1"
    issue_cert guarantor-1 layerx-guarantor-1 serverAuth,clientAuth "DNS:localhost,IP:127.0.0.1"
    issue_cert guarantor-2 layerx-guarantor-2 serverAuth,clientAuth "DNS:localhost,IP:127.0.0.1"
    issue_client_identity gateway-client layerx-gateway
    issue_client_identity interop-client layerx-interop-gateway
    issue_client_identity developer-client layerx-developer
    issue_client_identity registry-event-client layerx-registry-events
    issue_client_identity human-event-client layerx-human-events
    issue_server_identity ramp layerx-reference-ramp \
        "DNS:layerx-reference-ramp.$svc,DNS:layerx-reference-ramp.$TESTNET_NAMESPACE.svc,DNS:layerx-reference-ramp,DNS:layerx-reference-ramp-operator.$svc,DNS:layerx-reference-ramp-operator,DNS:$RAMP_HOST,DNS:localhost,IP:127.0.0.1"
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

interop_beta_roots_generate() {
    # The interop counterparties of a private testnet are its own test clients, so the bring-up
    # generates their trust roots and interop/deploy/gateway/render.py renders the public halves
    # under layerx-beta-* names; LAYERX_BETA_INTEROP_{AP2_KEYS,AP2_ASSETS,VISA_AGENTS,VISA_TARGETS,
    # FIAT_PROVIDERS} pin real external counterparties instead.
    local d=$1 use_case decimals currency port audience principal expires
    for use_case in checkout-mandate payment-mandate merchant-checkout; do
        (umask 077; openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 \
            -out "$d/interop-ap2-$use_case.key" 2>/dev/null)
        openssl pkey -in "$d/interop-ap2-$use_case.key" -pubout -outform DER 2>/dev/null \
            | tail -c 65 | od -An -v -tx1 | tr -d ' \n' > "$d/interop-ap2-$use_case.pub.hex"
        [ "$(wc -c < "$d/interop-ap2-$use_case.pub.hex")" -eq 130 ] \
            || fail "the generated AP2 $use_case key is not an uncompressed P-256 public key"
    done
    (umask 077; openssl genpkey -algorithm ed25519 -out "$d/interop-visa-agent.key" 2>/dev/null)
    ed25519_public_hex "$d/interop-visa-agent.key" > "$d/interop-visa-agent.pub.hex"
    [ "$(wc -c < "$d/interop-visa-agent.pub.hex")" -eq 64 ] \
        || fail "the generated Visa TAP agent key is not an ed25519 key"
    (umask 077; openssl genpkey -algorithm ed25519 -out "$d/interop-fiat-provider.key" 2>/dev/null)
    ed25519_public_hex "$d/interop-fiat-provider.key" > "$d/interop-fiat-provider.pub.hex"
    [ "$(wc -c < "$d/interop-fiat-provider.pub.hex")" -eq 64 ] \
        || fail "the generated fiat provider key is not an ed25519 key"
    decimals=$(sed -n 's/^ASSET_DECIMALS=\([0-9]*\)$/\1/p' "$REPO_ROOT/platform/hosted/node/bootstrap.sh")
    currency=$(sed -n 's/^ASSET_CURRENCY=\([A-Z]*\)$/\1/p' "$REPO_ROOT/platform/hosted/node/bootstrap.sh")
    [ -n "$decimals" ] && [ -n "$currency" ] || fail "the node bootstrap does not declare the asset currency and decimals"
    port=$(sed -n 's/^ *- {name: LAYERX_INTEROP_LISTEN, value: "0.0.0.0:\([0-9]*\)"}$/\1/p' \
        "$REPO_ROOT/platform/hosted/interop/deployment.yaml")
    [ -n "$port" ] || fail "the interop deployment does not declare LAYERX_INTEROP_LISTEN"
    audience="https://layerx-interop-gateway.$TESTNET_NAMESPACE.svc.cluster.local:$port"
    principal=$(printf '%s' "$TEST_SOURCE_DID" | sha256sum | cut -d ' ' -f 1)
    expires=$(( $(date -u +%s) + 31536000 ))
    (umask 077; jq -n \
        --arg checkout "$(cat "$d/interop-ap2-checkout-mandate.pub.hex")" \
        --arg payment "$(cat "$d/interop-ap2-payment-mandate.pub.hex")" \
        --arg merchant "$(cat "$d/interop-ap2-merchant-checkout.pub.hex")" \
        --arg agent "$(cat "$d/interop-visa-agent.pub.hex")" \
        --arg provider "$(cat "$d/interop-fiat-provider.pub.hex")" \
        --arg principal "$principal" \
        --arg actor "$(cat "$d/test-source-signer.pub.hex")" \
        --arg payee "$(cat "$d/test-destination-signer.pub.hex")" \
        --arg asset "$NODE_ASSET_ID" \
        --arg audience "$audience" \
        --arg currency "$currency" \
        --argjson decimals "$decimals" \
        --argjson expires "$expires" \
        '{ap2_keys: {"checkout-mandate": $checkout, "payment-mandate": $payment,
            "merchant-checkout": $merchant},
          visa_agent_public_key: $agent, visa_agent_expires_at: $expires,
          fiat_provider_public_key: $provider, principal_digest: $principal,
          layerx_agent: $actor, payer_account: $actor, payee_account: $payee,
          asset: $asset, audience: $audience, currency: $currency, asset_decimals: $decimals}' \
        > "$d/interop-beta-roots.json")
}

ed25519_public_hex() {
    openssl pkey -in "$1" -pubout -outform DER 2>/dev/null | tail -c 32 | od -An -v -tx1 | tr -d ' \n'
}

explorer_read_principal_generate() {
    # explorer_read_principal_generate SECRETS_DIR -> the explorer index's own read identity. The bring-up
    # generates and keeps this key like every other service principal; it is never an owner input.
    local d=$1
    (umask 077; openssl genpkey -algorithm ed25519 -out "$d/explorer-read.key" 2>/dev/null)
    ed25519_public_hex "$d/explorer-read.key" > "$d/explorer-read.pub.hex"
    [ "$(wc -c < "$d/explorer-read.pub.hex")" -eq 64 ] || fail "explorer read principal key is not an ed25519 key"
    (umask 077; openssl pkey -in "$d/explorer-read.key" -outform DER 2>/dev/null | tail -c 32 | od -An -v -tx1 | tr -d ' \n' > "$d/explorer-read.seed.hex")
    [ "$(wc -c < "$d/explorer-read.seed.hex")" -eq 64 ] || fail "explorer read principal key seed is not 32 bytes"
}

secp256k1_public_key() {
    # secp256k1_public_key KEY_FILE -> compressed secp256k1 public key of that private key
    local key
    key=$(tr -d '\r\n' < "$1")
    "$FOUNDRY_BIN/cast" wallet public-key --private-key "$key" 2>/dev/null | python3 -c '
import sys
raw = bytes.fromhex(sys.stdin.read().strip().removeprefix("0x"))
if len(raw) == 65 and raw[0] == 4:
    raw = raw[1:]
if len(raw) != 64:
    raise SystemExit("cast did not print an uncompressed secp256k1 public key")
print(("02" if raw[63] % 2 == 0 else "03") + raw[:32].hex())
'
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
entry = struct.pack(">HIQ", 3, int(sys.argv[4]), 1) + sequencer_id + public_key + struct.pack(">QQBQ", 1, 1 << 40, 0, 0)
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

genesis_metadata_generate() {
    # The node's LXGB v2 genesis metadata, produced by the in-repo encoder from the inputs this
    # bring-up already fixed: the cluster asset of the node manifest, the symbol and decimals
    # bootstrap.sh declares for it, the treasury identity bootstrap.sh registers at genesis, and
    # the withdrawal and module fees the node manifest makes bootstrap.sh apply. The salt is
    # retained so the ConfigMap is reproducible.
    local d="$SECRETS_DIR" bootstrap="$REPO_ROOT/platform/hosted/node/bootstrap.sh" bytes symbol decimals
    [ -f "$d/node-treasury.key" ] && [ ! -L "$d/node-treasury.key" ] \
        || fail "the node treasury key must be generated before the genesis metadata: $d/node-treasury.key"
    symbol=$(sed -n 's/^ASSET_SYMBOL=//p' "$bootstrap")
    decimals=$(sed -n 's/^ASSET_DECIMALS=//p' "$bootstrap")
    [ -n "$symbol" ] && [ -n "$decimals" ] \
        || fail "bootstrap.sh does not declare the beta asset symbol and decimals: $bootstrap"
    bytes=$(python3 - "$REPO_ROOT" "$NODE_MANIFEST" "$NODE_ASSET_ID" "$d/node-treasury.key" \
        "$d/node-genesis-salt" "$d/node-genesis-metadata.lxgb" "$symbol" "$decimals" <<'PYGENESISMETA'
import importlib.util
import os
from pathlib import Path
import sys

import yaml
from cryptography.hazmat.primitives.asymmetric import ed25519
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

root, manifest, asset_id, treasury, salt_file, output = (
    Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], Path(sys.argv[4]), Path(sys.argv[5]), Path(sys.argv[6]))
symbol, decimals = sys.argv[7], int(sys.argv[8])

# The node image ships genesis_fees.py and genesis-module-fees.json from these paths, so the fee
# configuration read here is the one bootstrap.sh reads inside the pod.
IMAGE_SOURCES = {'/opt/layerx/genesis-module-fees.json': 'platform/hosted/node/genesis-module-fees.json'}
METADATA_MOUNT = '/run/layerx/genesis/metadata.lxgb'


def module(name, relative):
    spec = importlib.util.spec_from_file_location(name, root / relative)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


lxgb = module('lxgb_metadata', 'tests/support/lxgb_metadata.py')
fees = module('genesis_fees', 'platform/hosted/node/genesis_fees.py')
lxgb.check()

arguments = None
for document in yaml.safe_load_all(manifest.read_text()):
    if not document or document.get('kind') != 'StatefulSet':
        continue
    for container in document['spec']['template']['spec']['containers']:
        if container['name'] == 'layerxd':
            arguments = [str(value) for value in container['args']]
if arguments is None:
    raise SystemExit('the node manifest has no layerxd container')


def argument(name):
    if arguments.count(name) != 1 or arguments.index(name) + 1 == len(arguments):
        raise SystemExit('the node manifest does not pass a single %s to bootstrap.sh' % name)
    return arguments[arguments.index(name) + 1]


if argument('--genesis-metadata') != METADATA_MOUNT:
    raise SystemExit('the node manifest reads its genesis metadata from %s, not %s'
                     % (argument('--genesis-metadata'), METADATA_MOUNT))
withdrawal_price = int(argument('--withdrawal-fee'))
module_fees = argument('--module-fees')
if module_fees not in IMAGE_SOURCES:
    raise SystemExit('the node manifest names an unpublished module fee file ' + module_fees)
prices = fees.module_prices(root / IMAGE_SOURCES[module_fees])

seed = treasury.read_bytes()
if len(seed) != 32:
    seed = bytes.fromhex(seed.decode().strip())
issuer = ed25519.Ed25519PrivateKey.from_private_bytes(seed).public_key().public_bytes(
    Encoding.Raw, PublicFormat.Raw)

if salt_file.exists():
    salt = salt_file.read_bytes()
    if len(salt) != 32:
        raise SystemExit('the retained genesis salt must hold exactly 32 bytes: ' + str(salt_file))
else:
    salt = os.urandom(32)
    with os.fdopen(os.open(salt_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'wb') as handle:
        handle.write(salt)

asset = bytes.fromhex(asset_id)
record = lxgb.metadata(asset, issuer, salt, symbol, decimals)
metadata = fees.module_metadata(record, withdrawal_price, prices)
if fees.module_metadata(metadata, withdrawal_price, prices) != metadata:
    raise SystemExit('the genesis metadata is not canonical under the node fee configuration')
with os.fdopen(os.open(output, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), 'wb') as handle:
    handle.write(metadata)
print(len(metadata))
PYGENESISMETA
    ) || fail "LXGB v2 genesis metadata generation failed"
    [ "$bytes" -gt 219 ] || fail "the generated genesis metadata is shorter than the request bound: $bytes bytes"
    log "genesis metadata written to $d/node-genesis-metadata.lxgb ($bytes bytes)"
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
    write_token "$d/gateway-faucet.token"
    write_token "$d/registry-request.token"
    write_token "$d/registry-publication.token"
    write_token "$d/registry-authority.token"
    write_token "$d/registry-identity.token"
    write_token "$d/explorer-program.token"
    explorer_read_principal_generate "$d"
    local producer
    for producer in gateway registry human; do
        write_token "$d/$producer-event-producer.token"
    done
    write_token "$d/provisioning.key"
    write_token "$d/cursor.key"
    printf 'layerx-faucet' > "$d/faucet-redis.username"
    write_token "$d/faucet-redis.password"
    write_redis_acl "$d/faucet-redis.acl" layerx-faucet "$(cat "$d/faucet-redis.password")"
    printf 'layerx-gateway' > "$d/gateway-redis.username"
    write_token "$d/gateway-redis.password"
    write_redis_acl "$d/gateway-redis.acl" layerx-gateway "$(cat "$d/gateway-redis.password")"
    printf 'layerx-interop' > "$d/interop-redis.username"
    write_token "$d/interop-redis.password"
    (umask 077; printf 'user layerx-interop on >%s ~gateway:* &* +@all\n' \
        "$(cat "$d/interop-redis.password")" >> "$d/gateway-redis.acl")
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
    module_registry_generate > "$d/module-registry.json"
    if [ -n "$CUSTODY_PROFILE" ]; then cp "$CUSTODY_PROFILE" "$d/custody.profile"; fi
    (umask 077; cp "$CA_DIR/sequencer.seed.hex" "$d/node-sequencer.key")
    (umask 077; random_hex 32 > "$d/node-treasury.key")
    genesis_metadata_generate
    write_token "$d/node-program.token"
    write_token "$d/node-replica.token"
    write_token "$d/ramp-authority.token"
    write_token "$d/ramp-operator-control.token"
    mkdir -p "$d/identity-tokens"
    chmod 0700 "$d/identity-tokens"
    cp "$d/gateway-identity.token" "$d/identity-tokens/gateway"
    cp "$d/registry-identity.token" "$d/identity-tokens/registry"
    cp "$d/developer-identity.token" "$d/identity-tokens/webhooks"
    cp "$d/identity-client.token" "$d/identity-tokens/faucet"
    local service
    for service in dashboard testnet ramp provisioning registrar; do
        write_token "$d/identity-tokens/$service"
    done
    write_token "$d/identity-store.key"
    evm_key_generate paxeer-deployer
    evm_key_generate paxeer-final-proposer
    evm_key_generate paxeer-final-executor
    evm_key_generate paxeer-emergency-council
    evm_key_generate paxeer-guarantor-controller
    evm_key_generate paxeer-guarantor-second-controller
    evm_key_generate paxeer-checkpoint-submitter
    if [ -n "${LAYERX_BETA_MIRROR_ETHEREUM_KEY_FILE:-}" ]; then
        [ -r "$LAYERX_BETA_MIRROR_ETHEREUM_KEY_FILE" ] \
            || fail "LAYERX_BETA_MIRROR_ETHEREUM_KEY_FILE=$LAYERX_BETA_MIRROR_ETHEREUM_KEY_FILE is not readable"
        (umask 077; tr -d '\r\n' < "$LAYERX_BETA_MIRROR_ETHEREUM_KEY_FILE" > "$d/mirror-ethereum-publisher.key")
    else
        evm_key_generate mirror-ethereum-publisher
    fi
    [[ $(cat "$d/mirror-ethereum-publisher.key") =~ ^0x[0-9a-fA-F]{64}$ ]] \
        || fail "the Ethereum mirror publisher key must be an 0x-prefixed 32-byte secp256k1 private key"
    secp256k1_public_key "$d/mirror-ethereum-publisher.key" > "$d/mirror-ethereum-publisher.pub.hex" \
        || fail "the Ethereum mirror publisher public key could not be derived"
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
    TEST_SOURCE_DID=${LAYERX_BETA_TEST_SOURCE_DID:-did:layerx:$(cat "$d/test-source-signer.pub.hex")}
    TEST_DESTINATION_DID=${LAYERX_BETA_TEST_DESTINATION_DID:-did:layerx:$(cat "$d/test-destination-signer.pub.hex")}
    [ "$TEST_SOURCE_DID" = "did:layerx:$(cat "$d/test-source-signer.pub.hex")" ] || fail "smoke source DID must be derived from its generated signer"
    [ "$TEST_DESTINATION_DID" = "did:layerx:$(cat "$d/test-destination-signer.pub.hex")" ] || fail "smoke destination DID must be derived from its generated signer"
    interop_beta_roots_generate "$d"
    source "$REPO_ROOT/platform/hosted/human/material.sh"
    human_secrets_generate
    [ "$TEST_SOURCE_DID" != "$TEST_DESTINATION_DID" ] || fail "the smoke source and destination DIDs must differ"
    TEST_AMOUNT=${LAYERX_BETA_TEST_AMOUNT:-1}
    [[ $TEST_AMOUNT =~ ^[1-9][0-9]*$ ]] || fail "LAYERX_BETA_TEST_AMOUNT must be a positive decimal"
}

module_registry_generate() {
    local bootstrap="$REPO_ROOT/platform/hosted/node/bootstrap.sh" asset symbol currency decimals
    asset=$(sed -n 's/^ASSET_ID="\([0-9a-f]*\)"$/\1/p' "$bootstrap")
    [ "$NODE_ASSET_ID" = "$asset" ] || fail "node manifest asset differs from bootstrap asset"
    symbol=$(sed -n 's/^ASSET_SYMBOL=//p' "$bootstrap")
    currency=$(sed -n 's/^ASSET_CURRENCY=//p' "$bootstrap")
    decimals=$(sed -n 's/^ASSET_DECIMALS=//p' "$bootstrap")
    local -a args=(generate --network-id "$NODE_NETWORK_ID" --protocol-version 3
        --asset "$NODE_ASSET_ID" --symbol "$symbol" --currency "$currency" --decimals "$decimals")
    local module
    while IFS= read -r module || [ -n "$module" ]; do
        args+=(--enable-module "$module")
    done < "$REPO_ROOT/platform/hosted/node/genesis-modules.conf"
    local -a mounts=()
    if [ -n "$CUSTODY_PROFILE" ]; then
        mounts+=(--mount "type=bind,src=$(realpath "$CUSTODY_PROFILE"),dst=/run/custody.profile,readonly")
        args+=(--custody-profile /run/custody.profile)
    fi
    docker run --rm --network none --read-only --user "$(id -u):$(id -g)" \
        --cap-drop ALL --security-opt no-new-privileges \
        "${mounts[@]}" --entrypoint /usr/local/bin/layerx-module-registry \
        "$(image_ref layerx-node)" "${args[@]}"
}

module_registry_verify() {
    local actor
    actor=$(sed -n 's/^LAYERX_NODE_TREASURY_DID=//p' "$WORK_DIR/genesis/node.env")
    kube -n "$TESTNET_NAMESPACE" exec layerx-node-0 -c registry-check -- \
        /usr/local/bin/layerx-module-registry read-node --socket /run/layerx/node/layerxd.lni.sock \
        --network-id "$NODE_NETWORK_ID" --protocol-version 3 --actor "$actor" > "$WORK_DIR/node-module-registry.json" \
        || fail "node preparation module registry unavailable"
    kube -n "$TESTNET_NAMESPACE" get configmap layerx-core-module-registry -o json \
        | jq -ej '.data["registry.json"]' > "$WORK_DIR/published-module-registry.json"
    python3 - "$SECRETS_DIR/module-registry.json" "$WORK_DIR/published-module-registry.json" "$WORK_DIR/node-module-registry.json" <<'PYREG'
import json, pathlib, sys
paths = [pathlib.Path(p) for p in sys.argv[1:]]
local, published, node = [json.loads(p.read_text()) for p in paths]
if paths[0].read_bytes() != paths[1].read_bytes() or published['modules'] != node['modules']:
    raise SystemExit('beta-cluster: error: node preparation module ids or ordinals differ from published registry')
PYREG
    log "node preparation module ids and ordinals match; assets are not compared because LNI carries none"
}

material_save() {
    python3 - "$SECRETS_DIR/retained-context.json" "$SEQUENCER_KEY_SOURCE" "$SEQUENCER_ID" \
        "$TEST_AUTH_SOURCE" "$TEST_SOURCE_DID" "$TEST_DESTINATION_DID" "$TEST_AMOUNT" \
        "$GUARANTOR_BOND" "$CHECKPOINT_REGISTRY" "$CUSTODY_PROFILE" <<'PYCTX'
import json, os, sys
with open(sys.argv[1], 'w') as output:
    os.chmod(sys.argv[1], 0o600)
    json.dump(sys.argv[2:], output)
PYCTX
    retained_material_inventory save
}

material_prepare() {
    source "$REPO_ROOT/platform/hosted/human/material.sh"
    case "${LAYERX_BETA_RETAIN_MATERIAL:-0}" in
        0)
            ca_generate
            secrets_generate
            ;;
        1)
            retained_material_inventory check || fail "retained material refused: inventory validation failed"
            KUBECONFIG_FILE=${LAYERX_BETA_KUBECONFIG:-$WORK_DIR/kubeconfig}
            [ -x "$TOOLS_DIR/kubectl" ] && [ -r "$KUBECONFIG_FILE" ] \
                || fail "retained material refused: live cluster kubeconfig and kubectl are required"
            retained_material_live_check
            local -a values
            mapfile -t values < <(python3 - "$SECRETS_DIR/retained-context.json" <<'PYCTX'
import json, sys
values = json.load(open(sys.argv[1]))
if len(values) != 9 or any(not isinstance(v, str) or '\n' in v or '\r' in v for v in values):
    raise SystemExit('invalid retained context')
print('\n'.join(values))
PYCTX
            )
            [ "${#values[@]}" = 9 ] || fail "retained material refused: invalid context"
            SEQUENCER_KEY_SOURCE=${values[0]}; SEQUENCER_ID=${values[1]}
            TEST_AUTH_SOURCE=${values[2]}; TEST_SOURCE_DID=${values[3]}
            TEST_DESTINATION_DID=${values[4]}; TEST_AMOUNT=${values[5]}
            GUARANTOR_BOND=${values[6]}; CHECKPOINT_REGISTRY=${values[7]}
            [ "$CUSTODY_PROFILE" = "${values[8]}" ] || fail "retained material refused: custody profile selection changed"
            if [ -n "$CUSTODY_PROFILE" ]; then
                cmp -s "$CUSTODY_PROFILE" "$SECRETS_DIR/custody.profile" \
                    || fail "retained material refused: custody profile bytes changed"
            fi
            local file
            for file in "$WORK_DIR/paxeer/settlement.env" "$WORK_DIR/paxeer/deployment.json" \
                "$SECRETS_DIR/environment-tree-digest" "$SECRETS_DIR/bwrap-digest" "$SECRETS_DIR/cgroup-exec-digest" \
                "$WORK_DIR/internal-principals/credentials.json" "$WORK_DIR/internal-principals/payments.credential" \
                "$WORK_DIR/internal-principals/programs.credential"; do
                [ -f "$file" ] && [ ! -L "$file" ] || fail "retained material refused: missing $file"
            done
            module_registry_generate > "$WORK_DIR/retained-registry-check.json"
            cmp -s "$SECRETS_DIR/module-registry.json" "$WORK_DIR/retained-registry-check.json" \
                || fail "retained material refused: configured module registry changed"
            ;;
        *) fail "LAYERX_BETA_RETAIN_MATERIAL must be 0 or 1" ;;
    esac
}

retained_principals_apply() {
    local service dir="$WORK_DIR/internal-principals"
    for service in payments programs; do
        apply_secret "$INTERNAL_NAMESPACE" "layerx-internal-$service-runtime" \
            --from-file=server.der="$CA_DIR/internal-$service/cert.der" --from-file=server-key.der="$CA_DIR/internal-$service/key.der" \
            --from-file=ca.der="$CA_DIR/ca.der" --from-file=upstream-ca.der="$CA_DIR/ca.der" \
            --from-file=token="$SECRETS_DIR/developer-${service%s}.token" \
            --from-file=credentials.json="$dir/credentials.json" --from-file=principal.credential="$dir/$service.credential"
    done
}

retained_material_test() (
    set -euo pipefail
    local directory
    directory=$(mktemp -d)
    trap 'rm -rf "$directory"' EXIT
    source "$REPO_ROOT/platform/hosted/human/material.sh"
    CA_DIR="$directory/ca"
    SECRETS_DIR="$directory/secrets"
    mkdir -m 0700 "$CA_DIR" "$SECRETS_DIR"
    if retained_material_inventory check > "$directory/refusal" 2>&1; then
        fail "incomplete retained material accepted"
    fi
    grep -Fq 'retained material refused: missing inventory' "$directory/refusal"
    for operation in beta_cluster_up beta_cluster_render; do
        if (export LAYERX_BETA_RETAIN_MATERIAL=1; "$operation" 0) > "$directory/refusal" 2>&1; then
            fail "incomplete retained material accepted by $operation"
        fi
        grep -Fq 'retained material refused: missing inventory' "$directory/refusal"
        [ -d "$CA_DIR" ] && [ -d "$SECRETS_DIR" ] || fail "retained material was removed"
    done
    printf 'retained material incomplete-directory refusal passed for inventory, up and render\n'
)

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
    local producer client
    local -a identity_material
    for producer in gateway registry human; do
        client="$producer-event-client"
        if [ "$producer" = gateway ]; then client=gateway-client; fi
        identity_material=()
        if [ "$producer" = registry ]; then identity_material+=(--from-file=identity-token="$s/registry-identity.token"); fi
        apply_secret "$ns" "layerx-$producer-event-producer" \
            --from-file=ca.der="$c/ca.der" --from-file=client.p12="$c/$client/client.p12" \
            --from-file=password="$c/$client/password" --from-file=token="$s/$producer-event-producer.token" \
            --from-file=webhook-token="$s/developer-source-trigger.token" \
            "${identity_material[@]}"
    done
    local service token
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" != 1 ]; then
        printf '{}\n' > "$s/human-credentials.json"
    fi
    for service in journeys payments approvals programs; do
        token=${service%s}
        producer=human
        if [ "$service" = payments ]; then producer=gateway; fi
        if [ "$service" = programs ]; then producer=registry; fi
        local allow_digest=false
        if [ "$producer" != human ]; then allow_digest=true; fi
        (umask 077; printf '[{"token_file":"/run/layerx/producer-token","allow_principal_digest":%s}]\n' "$allow_digest" > "$s/$service-producers.json")
        apply_secret "$INTERNAL_NAMESPACE" "layerx-internal-$service-runtime" \
            --from-file=server.der="$c/internal-$service/cert.der" --from-file=server-key.der="$c/internal-$service/key.der" \
            --from-file=ca.der="$c/ca.der" --from-file=upstream-ca.der="$c/ca.der" --from-file=token="$s/developer-$token.token" \
            --from-file=credentials.json="$s/human-credentials.json" \
            --from-file=producers.json="$s/$service-producers.json" --from-file=producer-token="$s/$producer-event-producer.token"
    done
    apply_secret "$ns" layerx-human-tls --from-file=server.crt.der="$c/human/cert.der" \
        --from-file=server.key.der="$c/human/key.der" --from-file=ca.crt="$c/ca.crt"
    apply_secret "$ns" layerx-internal-ca --from-file=ca.crt.der="$c/ca.der" --from-file=ca.crt="$c/ca.crt"
    apply_secret "$ns" layerx-human-web-tls --from-file=tls.crt="$c/human-web/cert.pem" --from-file=tls.key="$c/human-web/key.pem"
    [ -s "$s/explorer-read.seed.hex" ] && [ -s "$s/explorer-read.pub.hex" ] && [ -s "$s/sequencer-public-key" ] \
        || fail "explorer read principal material is missing: $s/explorer-read.seed.hex, $s/explorer-read.pub.hex and $s/sequencer-public-key are generated with fresh material"
    apply_secret "$ns" layerx-explorer-index --from-file=program-token="$s/explorer-program.token" \
        --from-file=read-key="$s/explorer-read.seed.hex" --from-file=sequencer-public-key="$s/sequencer-public-key"
    apply_secret "$ns" layerx-explorer-index-tls --from-file=tls.crt="$c/explorer-index/cert.pem" --from-file=tls.key="$c/explorer-index/key.pem"
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
    apply_secret "$ns" layerx-gateway-faucet-client --from-file=token="$s/gateway-faucet.token"
    apply_secret "$ns" layerx-gateway-redis-tls --from-file=tls.crt="$c/gateway-redis/cert.pem" \
        --from-file=tls.key="$c/gateway-redis/key.pem" --from-file=ca.crt="$c/ca.crt"
    apply_secret "$ns" layerx-gateway-redis-auth --from-file=users.acl="$s/gateway-redis.acl"
    apply_secret "$ns" layerx-gateway-redis-client --from-file=username="$s/gateway-redis.username" --from-file=password="$s/gateway-redis.password"
    apply_secret "$ns" layerx-gateway-key-provisioning --from-file=key="$s/provisioning.key"
    apply_secret "$ns" layerx-interop-gateway-tls --from-file=server.crt.der="$c/interop-gateway/cert.der" \
        --from-file=server.key.der="$c/interop-gateway/key.der"
    apply_secret "$ns" layerx-interop-client-identity --from-file=client.p12="$c/interop-client/client.p12" \
        --from-file=password="$c/interop-client/password"
    apply_secret "$ns" layerx-interop-redis-client --from-file=username="$s/interop-redis.username" \
        --from-file=password="$s/interop-redis.password"
    apply_configmap "$ns" layerx-core-module-registry --from-file=registry.json="$s/module-registry.json"
    apply_secret "$ns" layerx-program-registry-request-client --from-file=token="$s/registry-request.token"
    apply_secret "$ns" layerx-program-registry-publication-operator --from-file=token="$s/registry-publication.token"
    apply_secret "$ns" layerx-program-registry-server-tls --from-file=tls.crt.der="$c/registry/cert.der" --from-file=tls.key.der="$c/registry/key.der"
    apply_secret "$ns" layerx-program-registry-node-client --from-file=token="$s/registry-node.token"
    apply_secret "$ns" layerx-program-registry-authority-client --from-file=token="$s/registry-authority.token"
    apply_secret "$ns" layerx-sequencer-trust-history --from-file=history="$s/trust-history"
    apply_configmap "$ns" layerx-receipt-authority --from-file=replica-id="$s/receipt-authority-replica-id"
    apply_secret "$ns" layerx-node-keys --from-file=sequencer.key="$s/node-sequencer.key" --from-file=treasury.key="$s/node-treasury.key"
    [ -f "$s/node-genesis-metadata.lxgb" ] && [ ! -L "$s/node-genesis-metadata.lxgb" ] \
        || fail "the LXGB v2 genesis metadata the node mounts is missing: $s/node-genesis-metadata.lxgb"
    apply_configmap "$ns" layerx-node-genesis-metadata --from-file=metadata.lxgb="$s/node-genesis-metadata.lxgb"
    publication_binding_publish
    if [ -n "$CUSTODY_PROFILE" ]; then
        apply_configmap "$ns" layerx-node-custody-profile --from-file=profile="$CUSTODY_PROFILE"
    fi
    apply_secret "$ns" layerx-node-tokens --from-file=program-token="$s/node-program.token" --from-file=replica-token="$s/node-replica.token"
    apply_secret "$ns" layerx-pending-core-tls --from-file=server.crt.der="$c/pending-core/cert.der" --from-file=server.key.der="$c/pending-core/key.der"
    apply_secret "$ns" layerx-pending-core-admin-tls --from-file=server.crt.der="$c/pending-core-admin/cert.der" --from-file=server.key.der="$c/pending-core-admin/key.der"
    apply_secret "$ns" layerx-receipt-authority-tls --from-file=server.crt.der="$c/receipt-authority/cert.der" --from-file=server.key.der="$c/receipt-authority/key.der"
    apply_secret "$ns" layerx-agent-boundary-tls --from-file=server.crt.der="$c/agent-boundary/cert.der" --from-file=server.key.der="$c/agent-boundary/key.der"
    apply_secret "$ns" layerx-agentd-tls --from-file=tls.crt="$c/agentd/cert.pem" --from-file=tls.key="$c/agentd/key.pem"
    apply_secret "$ns" layerx-webhooks-authority-client --from-file=token="$s/developer-authority.token"
    apply_secret "$ns" layerx-identity-server-tls --from-file=server.crt.der="$c/identity/cert.der" --from-file=server.key.der="$c/identity/key.der"
    apply_secret "$ns" layerx-identity-service-tokens --from-file="$s/identity-tokens"
    apply_secret "$ns" layerx-identity-store-key --from-file=key="$s/identity-store.key"
    apply_secret "$ns" paxeer-boundary-tls --from-file=server.crt.der="$c/paxeer-boundary/cert.der" --from-file=server.key.der="$c/paxeer-boundary/key.der"
    for identity in 1 2; do
        apply_secret "$ns" "layerx-guarantor-$identity-tls" --from-file=tls.crt="$c/guarantor-$identity/cert.pem" \
            --from-file=tls.key="$c/guarantor-$identity/key.pem" --from-file=ca.crt="$c/ca.crt"
    done
    apply_secret "$ns" paxeer-checkpoint-submitter --from-file=key="$s/paxeer-checkpoint-submitter.key"
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

publication_policy_create() {
    local operation=$1 output=$2 temporary
    shift 2
    temporary=$(mktemp -d "$WORK_DIR/publication-policy.XXXXXXXX")
    chmod 0700 "$temporary"
    python3 "$REPO_ROOT/platform/hosted/tests/publication-policy.py" "$operation" "$temporary/policy.json" "$@"
    if [ -e "$output" ] || [ -L "$output" ]; then
        [ -f "$output" ] && [ ! -L "$output" ] && cmp -s "$temporary/policy.json" "$output" \
            || fail "publication policy changed: $operation"
    else
        mv "$temporary/policy.json" "$output"
    fi
}

publication_binding_publish() {
    local recipient policy="$SECRETS_DIR/publication-binding-policy.json"
    recipient=$(cat "$SECRETS_DIR/paxeer-deployer.address")
    publication_policy_create treasury "$policy" "$NODE_NETWORK_ID" "$NODE_ASSET_ID" "${recipient#0x}"
    local -a authorization=()
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" = 1 ]; then
        [ -f "$SECRETS_DIR/publication-authorization.json" ] && [ ! -L "$SECRETS_DIR/publication-authorization.json" ] \
            || fail 'retained checkpoint publication authorization is missing'
        authorization=(--from-file=authorization.json="$SECRETS_DIR/publication-authorization.json")
    fi
    apply_secret "$TESTNET_NAMESPACE" layerx-node-publication \
        --from-file=binding-policy.json="$policy" "${authorization[@]}"
}

publication_authorization_publish() {
    local recipient public vault
    recipient=$(cat "$SECRETS_DIR/paxeer-deployer.address")
    public=$(sed -n 's/^LAYERX_NODE_TREASURY_PUBLIC_KEY=//p' "$WORK_DIR/genesis/node.env")
    vault=$(jq -er '.vault' "$WORK_DIR/human-evidence-input/owner-custody.json")
    publication_policy_create authorization "$SECRETS_DIR/publication-authorization.json" \
        "$NODE_NETWORK_ID" "$PAXEER_CHAIN_ID" "$GUARANTOR_BOND" "$CHECKPOINT_REGISTRY" "$vault" \
        "$public" "$NODE_ASSET_ID" "${recipient#0x}"
    apply_secret "$TESTNET_NAMESPACE" layerx-node-publication \
        --from-file=binding-policy.json="$SECRETS_DIR/publication-binding-policy.json" \
        --from-file=authorization.json="$SECRETS_DIR/publication-authorization.json"
}

builder_release_publish() {
    local ns="$TESTNET_NAMESPACE" ref deployment_ref digest bwrap_digest cgroup_digest sums
    ref=$(image_ref layerx-program-registry)
    sums=$(docker run --rm --entrypoint /bin/sh "$ref" -c 'sha256sum /usr/bin/bwrap /usr/bin/layerx-cgroup-exec')
    bwrap_digest=$(printf '%s\n' "$sums" | awk '$2 == "/usr/bin/bwrap" { print $1 }')
    cgroup_digest=$(printf '%s\n' "$sums" | awk '$2 == "/usr/bin/layerx-cgroup-exec" { print $1 }')
    [ "${#bwrap_digest}" -eq 64 ] && [ "${#cgroup_digest}" -eq 64 ] || fail "could not read bwrap and layerx-cgroup-exec digests from $ref"
    printf '%s' "$bwrap_digest" > "$SECRETS_DIR/bwrap-digest"
    printf '%s' "$cgroup_digest" > "$SECRETS_DIR/cgroup-exec-digest"
    require_builder_environment
    digest=$(environment_digest "$LAYERX_BETA_BUILDER_ENVIRONMENT_DIR") || fail "builder environment digest failed"
    printf '%s' "$digest" > "$SECRETS_DIR/environment-tree-digest"
    apply_configmap "$ns" layerx-program-builder-release --from-file=environment-tree-digest="$SECRETS_DIR/environment-tree-digest" \
        --from-file=bwrap-digest="$SECRETS_DIR/bwrap-digest" --from-file=cgroup-exec-digest="$SECRETS_DIR/cgroup-exec-digest"
    deployment_ref=$(awk '$1 == "layerx-program-registry" {print $2}' "$WORK_DIR/image-pins")
    [[ $deployment_ref =~ @sha256:[0-9a-f]{64}$ ]] || fail "missing immutable builder loader image"
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
  nodeSelector: {$BOUNDARY_LABEL: "v2"}
  securityContext: {runAsNonRoot: true, runAsUser: 4030, runAsGroup: 4030, fsGroup: 4030}
  containers:
    - name: loader
      image: $deployment_ref
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
    local src=$1 dst=$2 name canonical ref id pin
    cp "$src" "$dst"
    while read -r name canonical ref id; do
        pin=$ref
        if [ "$id" != unbuilt ]; then
            pin=$(awk -v name="$name" '$1 == name {print $2}' "$WORK_DIR/image-pins")
            [[ $pin =~ @sha256:[0-9a-f]{64}$ ]] || fail "missing immutable deployment digest for $name"
        fi
        sed -i "s|image: $canonical\$|image: $pin|" "$dst"
    done < "$WORK_DIR/images"
    sed -i "s|imagePullPolicy: Always|imagePullPolicy: $PULL_POLICY|" "$dst"
    sed -i "s|developers\.layerx\.example|$DEVELOPER_HOST|g" "$dst"
    if grep -E 'image: ghcr.io/[^@[:space:]]*:[^@[:space:]]+$' "$dst"; then
        fail "rendered manifest $dst still references a mutable GHCR image"
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
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" != 1 ]; then
        openssl x509 -inform DER -in "$CA_DIR/ca.der" -out "$CA_DIR/ca.pem"
    fi
    jq -n --arg primary "$PAXEER_URL" --arg observer "$PAXEER_OBSERVER_URL" \
        --arg ca "$CA_DIR/ca.pem" --arg key "$SECRETS_DIR/paxeer-deployer.key" \
        '{rpc_origins: [$primary, $observer], ca_bundle: $ca, key_file: $key,
          backends: [{container: "paxd", home: "/var/lib/paxeer"},
                     {container: "paxd-observer", home: "/var/lib/paxeer-observer"}]}' \
        > "$WORK_DIR/paxeer/rpc-origins.json"
    local comet_chain_id
    comet_chain_id=$(kube -n "$TESTNET_NAMESPACE" exec paxeer-0 -c paxd -- jq -er .chain_id /var/lib/paxeer/config/genesis.json)
    python3 "$SCRIPT_DIR/paxeer-identity.py" "$WORK_DIR/paxeer/rpc-origins.json" "$comet_chain_id"
    python3 "$SCRIPT_DIR/custody-identity.py" "$WORK_DIR/paxeer/rpc-origins.json"
    log "Paxeer origins: $PAXEER_URL $PAXEER_OBSERVER_URL; CA bundle: $CA_DIR/ca.pem; inputs: $WORK_DIR/paxeer/rpc-origins.json"
}

status_publisher_render() {
    local pattern='^https://[A-Za-z0-9._~:/?#@!$&()*+,;=%-]+$'
    if [ -n "$STATUS_PUBLISH_URL" ]; then
        [[ $STATUS_PUBLISH_URL =~ $pattern ]] \
            || fail "LAYERX_BETA_STATUS_PUBLISH_URL must be the https URL of the owner status publisher"
    elif [ "$STATUS_PUBLISHER_REPORTED" = 0 ]; then
        MISSING_INPUTS+=("LAYERX_BETA_STATUS_PUBLISH_URL (https endpoint of the separately operated status publisher; the layerx-testnet-status-publisher CronJob is not applied without it)")
        STATUS_PUBLISHER_REPORTED=1
    fi
    python3 - "$MANIFESTS_DIR/testnet.yaml" "$STATUS_PUBLISH_URL" <<'PYSTATUS'
import sys
import yaml
path, url = sys.argv[1], sys.argv[2]
name = 'layerx-testnet-status-publisher'
with open(path) as source:
    documents = list(yaml.safe_load_all(source))
publisher = [document for document in documents
             if document.get('kind') == 'CronJob' and document['metadata']['name'] == name]
if len(publisher) != 1:
    raise SystemExit('the testnet manifest must declare exactly one ' + name + ' CronJob')
if url:
    container, = publisher[0]['spec']['jobTemplate']['spec']['template']['spec']['containers']
    variable, = [entry for entry in container['env'] if entry['name'] == 'LAYERX_STATUS_PUBLISH_URL']
    variable['value'] = url
else:
    documents.remove(publisher[0])
    for document in documents:
        if document.get('kind') != 'NetworkPolicy' or document['spec'].get('ingress') is None:
            continue
        kept = []
        for rule in document['spec']['ingress']:
            if not rule.get('from'):
                kept.append(rule)
                continue
            peers = [peer for peer in rule['from']
                     if peer.get('podSelector', {}).get('matchLabels', {}).get('app') != name]
            if peers:
                kept.append({**rule, 'from': peers})
        document['spec']['ingress'] = kept
with open(path, 'w') as output:
    yaml.safe_dump_all(documents, output, sort_keys=False)
PYSTATUS
    if [ -z "$STATUS_PUBLISH_URL" ]; then
        log "status publisher: LAYERX_BETA_STATUS_PUBLISH_URL is unset; the layerx-testnet-status-publisher CronJob is left out of $MANIFESTS_DIR/testnet.yaml"
    else
        log "status publisher: the layerx-testnet-status-publisher CronJob publishes to $STATUS_PUBLISH_URL"
    fi
}

relay_archive_config_write() {
    # relay_archive_config_write OUTPUT NETWORK_ID GENESIS_SHA256 SEQUENCER_ID SEQUENCER_PUBLIC_KEY FIRST_BATCH LAST_BATCH PUBLIC_URL
    [ "$#" -eq 8 ] || fail "relay_archive_config_write needs output, network id, genesis digest, sequencer id, sequencer public key, first batch, last batch and public URL"
    LAYERX_RELAY_SOURCE_DIR="$RELAY_NODE_SOURCE_DIR" LAYERX_RELAY_DATA_DIR="$RELAY_DATA_DIR" \
        LAYERX_RELAY_TLS_DIR="$RELAY_TLS_DIR" LAYERX_RELAY_CODEC="$RELAY_CODEC" \
        python3 - "$@" <<'PYRELAY'
import json
import os
import re
import sys
import urllib.parse

(destination, network_id, genesis_sha256, sequencer_id, sequencer_public_key,
 first_batch, last_batch, public_url) = sys.argv[1:9]
source_dir = os.environ["LAYERX_RELAY_SOURCE_DIR"]
data_dir = os.environ["LAYERX_RELAY_DATA_DIR"]
tls_dir = os.environ["LAYERX_RELAY_TLS_DIR"]
codec = os.environ["LAYERX_RELAY_CODEC"]


def refuse(detail):
    raise SystemExit("beta-cluster: error: relay/archive configuration refused: " + detail)


def pin(value, name):
    if re.fullmatch(r"[0-9a-f]{64}", value) is None or value == "0" * 64:
        refuse(name + " must be a non-zero lowercase 64-character hexadecimal pin")
    return value


def cursor(value, name):
    if re.fullmatch(r"(0|[1-9][0-9]*)", value) is None or int(value) > 0xFFFFFFFFFFFFFFFF:
        refuse(name + " must be a canonical unsigned decimal batch cursor")
    return int(value)


def origins(variable, allowed_paths):
    raw = os.environ.get(variable, "")
    values = [entry.strip() for entry in raw.split(",") if entry.strip()]
    result = []
    for value in values:
        try:
            parsed = urllib.parse.urlsplit(value)
            host, port = parsed.hostname, parsed.port
        except ValueError:
            refuse(variable + " carries a malformed URL")
        if (parsed.scheme != "https" or not host or parsed.username is not None
                or parsed.password is not None or parsed.query or parsed.fragment
                or parsed.path.rstrip("/") not in allowed_paths):
            refuse(variable + " must list credential-free HTTPS URLs with a supported path "
                   + "(" + ", ".join(sorted(path or "/" for path in allowed_paths)) + ")")
        if port is not None and not 1 <= port <= 65535:
            refuse(variable + " carries an invalid port")
        normalized = value.rstrip("/")
        if normalized not in result:
            result.append(normalized)
    return result


if re.fullmatch(r"[1-9][0-9]*", network_id) is None or not 1 <= int(network_id) <= 0xFFFFFFFF:
    refuse("the node network id is out of range")
public = urllib.parse.urlsplit(public_url)
if public.scheme != "https" or not public.hostname or public.path.rstrip("/"):
    refuse("LAYERX_BETA_RELAY_HOST must produce a bare HTTPS origin, got " + public_url)
first = cursor(first_batch, "sequencer_first_batch")
last = cursor(last_batch, "sequencer_last_batch")
if first > last or first == 0:
    refuse("the sequencer batch authorization range is empty")
seeds = origins("LAYERX_BETA_RELAY_PEER_SEED", ("",))
document = {
    "network_id": int(network_id),
    "genesis_sha256": pin(genesis_sha256, "genesis_sha256"),
    "sequencer_id": pin(sequencer_id, "sequencer_id"),
    "sequencer_public_key": pin(sequencer_public_key, "sequencer_public_key"),
    "sequencer_first_batch": str(first),
    "sequencer_last_batch": str(last),
    "genesis_manifest": source_dir + "/genesis/genesis.manifest",
    "genesis_snapshot": source_dir + "/genesis/00000000000000000000.lxs",
    "source_log": source_dir + "/checkpoints/da-bodies.log",
    "data_dir": data_dir,
    "listen": "0.0.0.0:9443",
    "public_url": public_url.rstrip("/"),
    "codec": codec,
    "allow_loopback_dev": False,
    "upstreams": origins("LAYERX_BETA_RELAY_UPSTREAM", ("",)),
    "submission_upstreams": origins(
        "LAYERX_BETA_RELAY_SUBMISSION_UPSTREAM", ("", "/v1/activities", "/rpc")),
    "tls_cert": tls_dir + "/tls.crt",
    "tls_key": tls_dir + "/tls.key",
    "ca_file": "/etc/ssl/certs/ca-certificates.crt",
    "peer_discovery": {
        "enabled": bool(seeds),
        "seeds": seeds,
        "advertise_ttl_seconds": 300,
        "refresh_interval_seconds": 60,
        "max_peers": 64,
        "max_advertised_peers": 32,
        "allow_loopback_dev": False,
    },
}
descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(descriptor, "w", encoding="utf-8") as output:
    json.dump(document, output, sort_keys=True, indent=2)
    output.write("\n")
PYRELAY
}

relay_archive_apply() {
    local configuration="$WORK_DIR/relay-archive.json" manifest="$WORK_DIR/genesis/genesis.manifest" pin
    grep -Fq -- "- $NODE_DATA_DIR" "$NODE_MANIFEST" \
        || fail "the node no longer runs with --data-dir $NODE_DATA_DIR; the relay/archive canonical source paths must follow it"
    if [ -z "${LAYERX_BETA_RELAY_SUBMISSION_UPSTREAM:-}" ]; then
        MISSING_INPUTS+=("LAYERX_BETA_RELAY_SUBMISSION_UPSTREAM: relay/archive activity forwarding; supply a public HTTPS endpoint such as https://$GATEWAY_HOST/v1/activities because platform/relay_archive/protocol.py refuses private cluster addresses")
        log "relay/archive activity forwarding is unconfigured: set LAYERX_BETA_RELAY_SUBMISSION_UPSTREAM to a public HTTPS endpoint"
    fi
    mkdir -p "$WORK_DIR/genesis"
    node_file_fetch "$NODE_DATA_DIR/genesis/genesis.manifest" "$manifest"
    pin=$(sha256sum "$manifest" | cut -d ' ' -f 1)
    [[ $pin =~ ^[0-9a-f]{64}$ ]] || fail "the node genesis manifest has no SHA-256 digest"
    relay_archive_config_write "$configuration" "$NODE_NETWORK_ID" "$pin" \
        "$(cat "$SECRETS_DIR/sequencer-id")" "$(cat "$SECRETS_DIR/sequencer-public-key")" \
        "$(cat "$SECRETS_DIR/sequencer-first-batch")" "$(cat "$SECRETS_DIR/sequencer-last-batch")" \
        "https://$RELAY_HOST"
    apply_secret "$TESTNET_NAMESPACE" layerx-relay-archive-config --from-file=relay-archive.json="$configuration"
    apply_tls_secret "$TESTNET_NAMESPACE" layerx-relay-archive-tls relay-archive
    kube apply -f "$MANIFESTS_DIR/relay-archive.yaml" > /dev/null
    wait_for_pod_ready "$TESTNET_NAMESPACE" app=layerx-relay-archive 600
    log "relay/archive serving https://$RELAY_HOST pinned to genesis manifest $pin"
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
    movement = next(container for container in pod["containers"] if container["name"] == "human-movement")
    movement["env"].append({"name": "LAYERX_HUMAN_MOVEMENT_PROVIDER_CUSTODY_PROFILE", "value": "/run/layerx/custody.profile"})
    movement["volumeMounts"].append({"name": "custody-profile", "mountPath": "/run/layerx/custody.profile", "subPath": "profile", "readOnly": True})
    pod["volumes"].append({"name": "custody-profile", "configMap": {"name": "layerx-node-custody-profile"}})
with open(path, "w") as output:
    yaml.safe_dump_all(documents, output, sort_keys=False)
PY
    fi
    python3 - "$MANIFESTS_DIR/node.yaml" <<'PYREG'
import sys, yaml
path = sys.argv[1]
with open(path) as source:
    documents = list(yaml.safe_load_all(source))
for document in documents:
    if document.get('kind') != 'StatefulSet':
        continue
    pod = document['spec']['template']['spec']
    daemon = next(c for c in pod['containers'] if c['name'] == 'layerxd')
    pod['containers'].append({
        'name': 'registry-check', 'image': daemon['image'],
        'imagePullPolicy': daemon['imagePullPolicy'],
        'command': ['sh', '-c', 'exec sleep infinity'],
        'securityContext': {'runAsNonRoot': True, 'runAsUser': 4021, 'runAsGroup': 4020,
                            'allowPrivilegeEscalation': False, 'readOnlyRootFilesystem': True,
                            'capabilities': {'drop': ['ALL']}},
        'resources': {'requests': {'cpu': '10m', 'memory': '16Mi'},
                      'limits': {'cpu': '100m', 'memory': '64Mi'}},
        'volumeMounts': [{'name': 'run', 'mountPath': '/run/layerx', 'readOnly': True}],
    })
with open(path, 'w') as output:
    yaml.safe_dump_all(documents, output, sort_keys=False)
PYREG
    render_manifest "$REPO_ROOT/platform/hosted/identity/deployment.yaml" "$MANIFESTS_DIR/identity.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/paxeer/deployment.yaml" "$MANIFESTS_DIR/paxeer.yaml"
    paxeer_observer_render
    render_manifest "$REPO_ROOT/platform/hosted/testnet/deployment.yaml" "$MANIFESTS_DIR/testnet.yaml"
    status_publisher_render
    render_manifest "$REPO_ROOT/platform/hosted/gateway/deployment.yaml" "$MANIFESTS_DIR/gateway.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/interop/deployment.yaml" "$MANIFESTS_DIR/interop.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/registry/journal-pvc.yaml" "$MANIFESTS_DIR/registry-journal.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/registry/deployment.yaml" "$MANIFESTS_DIR/registry.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/human/deployment.yaml" "$MANIFESTS_DIR/human.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/human/web-deployment.yaml" "$MANIFESTS_DIR/human-web.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/internal/deployment.yaml" "$MANIFESTS_DIR/internal.yaml"
    render_manifest "$REPO_ROOT/platform/hosted/webhooks/deployment.yaml" "$MANIFESTS_DIR/developer.yaml"
    render_manifest "$REPO_ROOT/platform/relay_archive/deployment.yaml" "$MANIFESTS_DIR/relay-archive.yaml"
    sed -i "s|relay\.layerx\.example|$RELAY_HOST|g" "$MANIFESTS_DIR/relay-archive.yaml"
    grep -Fq "host: $RELAY_HOST" "$MANIFESTS_DIR/relay-archive.yaml" \
        || fail "the relay/archive manifest host could not be bound to $RELAY_HOST"
    if [ "$RAMP_ENABLED" = 1 ]; then
        render_manifest "$REPO_ROOT/platform/ramps/deployment.yaml" "$MANIFESTS_DIR/ramp.yaml"
    fi
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
    kube apply -f "$MANIFESTS_DIR/registry-journal.yaml" > /dev/null
    kube apply -f "$MANIFESTS_DIR/paxeer.yaml" > /dev/null
    kube apply -f "$MANIFESTS_DIR/identity.yaml" > /dev/null
    kube -n "$ns" delete deployment layerx-human --ignore-not-found --wait=true > /dev/null
    kube apply -f "$MANIFESTS_DIR/human.yaml" > /dev/null
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" = 1 ]; then
        human_secrets_apply
        kube apply -f "$MANIFESTS_DIR/node.yaml" > /dev/null
    else
        python3 "$REPO_ROOT/platform/hosted/human/bootstrap.py" "$MANIFESTS_DIR/node.yaml" "$MANIFESTS_DIR/node-bootstrap.yaml"
        kube apply -f "$MANIFESTS_DIR/node-bootstrap.yaml" > /dev/null
    fi
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

# The Human web container reads LAYERX_EXPLORER_NAMING_PROGRAM from the layerx-explorer-index ConfigMap, so
# its workload is applied only once explorer_observation_publish has published that key.
human_web_apply() {
    kube apply -f "$MANIFESTS_DIR/human-web.yaml" > /dev/null
    [ "$EXPLORER_OBSERVATION_PUBLISHED" = 1 ] || return 0
    wait_for_pod_ready "$TESTNET_NAMESPACE" app=layerx-human-web 300
}

# registry_deployment_produce [ARTIFACT REQUEST]
# Without arguments it builds the reference escrow program and the smoke binaries and produces the signed
# deployment activity of that artifact as the Human evidence input. With arguments it produces the signed
# deployment activity of an already built ARTIFACT into REQUEST.
registry_deployment_produce() (
    set -euo pipefail
    umask 077
    local temporary producer
    local artifact=${1:-"$WORK_DIR/program-target/wasm32-unknown-unknown/release/layerx_reference_escrow.wasm"}
    local request=${2:-"$WORK_DIR/human-evidence-input/program-deployment.lxa"}
    if [ "$#" -eq 0 ]; then
        CARGO_TARGET_DIR="$WORK_DIR/program-target" make -C "$REPO_ROOT" programs-reference-escrow >&2
        CARGO_TARGET_DIR="$WORK_DIR/smoke-target" cargo build --manifest-path "$REPO_ROOT/platform/Cargo.toml" \
            -p layerx-platform-cli --bin layerx --example hosted-send >&2
    fi
    [ -s "$artifact" ] || fail "the program artifact $artifact was not built"
    mkdir -p "$(dirname "$request")"
    [ ! -e "$request" ] && [ ! -L "$request" ] || fail 'deployment input exists; reconcile before retry'
    temporary=$(mktemp "$(dirname "$request")/.$(basename "$request").XXXXXXXX")
    trap 'rm -f "$temporary"' EXIT
    producer=$(cat <<'PYREGDEPLOY'
import hashlib, json, os, socket, stat, struct, subprocess, sys, time
from pathlib import Path

def protected(path, mode=0o600):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as handle:
        info = os.fstat(handle.fileno())
        assert stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
        assert stat.S_IMODE(info.st_mode) == mode and info.st_nlink == 1
        value = handle.read(65537)
        assert 0 < len(value) <= 65536
        return value

def signer(path, request):
    connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    connection.settimeout(30)
    try:
        connection.connect(path)
        connection.sendall(request.encode('ascii') + b'\n')
        buffered = b''
        while b'\n' not in buffered and len(buffered) <= 4096:
            chunk = connection.recv(4096)
            if not chunk:
                break
            buffered += chunk
    finally:
        connection.close()
    reply = json.loads(buffered.split(b'\n', 1)[0].decode())
    assert 'error' not in reply, 'the treasury signer refused a deployment request'
    return reply

def run(args, descriptors=()):
    result = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            pass_fds=descriptors, check=False)
    assert result.returncode == 0, 'node deployment producer command refused'
    return result.stdout

def blob(value):
    return struct.pack('>I', len(value)) + value

node = dict(line.split('=', 1) for line in protected(sys.argv[1], 0o600).decode().splitlines())
network = int(sys.argv[2])
assert network > 0
wasm = sys.stdin.buffer.read(524289)
assert wasm.startswith(b'\0asm\x01\0\0\0') and 0 < len(wasm) <= 524184
assert 'LAYERX_NODE_TREASURY_KEY_FILE' not in node, 'the node environment still names treasury key material'
treasury = node['LAYERX_NODE_TREASURY_SIGNER_SOCKET']
identity = signer(treasury, 'public-key')
public = bytes.fromhex(identity['public_key'])
assert len(public) == 32 and public.hex() == node['LAYERX_NODE_TREASURY_PUBLIC_KEY']
did = node['LAYERX_NODE_TREASURY_DID'].encode()
assert did == b'did:layerx:' + public.hex().encode() and identity['did'] == did.decode()
state = json.loads(run([sys.argv[3], 'read-state', '--socket', node['LAYERX_NODE_LNI_SOCKET'],
                       '--network-id', str(network), '--protocol-version', '3', '--actor', did.decode()]))
assert state['network_id'] == network and state['protocol_version'] == 3
assert state['evidence'] == 'authenticated_node_snapshot'
sequence = state['account_sequence']
assert type(sequence) is int and 0 <= sequence < 2**64
payload = (os.urandom(32) + struct.pack('>HBB', 2, 0, 0) + bytes(32)
           + hashlib.sha256(wasm).digest() + blob(wasm))
now = time.time_ns() // 1000000
fields = (b'\x01' + struct.pack('>H', 3) + b'\x02' + struct.pack('>I', network)
          + b'\x03' + struct.pack('>I', (9 << 16) | 1) + b'\x04' + blob(did)
          + b'\x05' + blob(public) + b'\x06' + struct.pack('>Q', sequence)
          + b'\x07' + struct.pack('>QQ', now - 30000, now + 120000)
          + b'\x08' + blob(os.urandom(32)) + b'\x09' + bytes(16)
          + b'\x0a' + blob(hashlib.sha256(b'LXP/v1/payload-hash\0' + payload).digest())
          + b'\x0b' + blob(payload))
unsigned = struct.pack('>HHB', 3, 0x1001, 11) + fields
preimage = hashlib.sha256(b'LXP/v1/signature-preimage\0' + unsigned).digest()
signed_reply = signer(treasury, 'sign ' + preimage.hex())
assert signed_reply['digest'] == preimage.hex() and signed_reply['public_key'] == public.hex()
signature = bytes.fromhex(signed_reply['signature'])
assert len(signature) == 64
signed = struct.pack('>HHB', 3, 0x1001, 12) + fields + b'\x0c' + blob(signature)
assert len(signed) <= 1048576
sys.stdout.buffer.write(signed)
PYREGDEPLOY
)
    kube -n "$TESTNET_NAMESPACE" exec -i layerx-node-0 -c layerxd -- \
        python3 -c "$producer" /var/lib/layerx/node/node.env "$NODE_NETWORK_ID" /usr/local/bin/layerxctl \
        < "$artifact" > "$temporary"
    python3 - "$temporary" "$request" <<'PYPUBLISH'
import os, sys
with open(sys.argv[1], 'rb') as handle:
    assert 0 < os.fstat(handle.fileno()).st_size <= 1048576
    os.fsync(handle.fileno())
os.link(sys.argv[1], sys.argv[2])
os.unlink(sys.argv[1])
fd = os.open(os.path.dirname(sys.argv[2]), os.O_RDONLY | os.O_DIRECTORY)
try:
    os.fsync(fd)
finally:
    os.close(fd)
PYPUBLISH
)

# The reference naming program is deployed by the node treasury, the same publication authority the reference
# escrow deployment of human_journal_deploy uses: the registry admits a deployment only against protocol
# evidence its node boundary produced, and the node treasury signer is the only deployment principal the
# bring-up holds inside the node pod. human_evidence_provision calls it inside two bounds. It runs after
# human_native_provision, because the owner admission there requires a fresh native genesis head and this
# deployment commits an activity that advances the head. It runs before human_journal_deploy, because that
# step materialises the registry journal exactly once and the export to $WORK_DIR/registry-journal has to
# hold both deployment pairs: explorer_observation_publish reads the naming program id from that export.
naming_program_deploy() {
    local example="$REPO_ROOT/programs/sdk/rust/examples/naming"
    local artifact="$WORK_DIR/program-target/wasm32-unknown-unknown/release/layerx_reference_naming.wasm"
    local request="$WORK_DIR/naming-program/naming-deployment.lxa"
    local response="$WORK_DIR/naming-deployment-result.json" status
    if command -v rustup > /dev/null 2>&1; then
        rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown \
            || fail "the reference naming program needs the wasm32-unknown-unknown Rust target; run 'rustup target add wasm32-unknown-unknown' and retry"
    fi
    CARGO_TARGET_DIR="$WORK_DIR/program-target" sh "$example/build.sh" >&2 \
        || fail "the reference naming program did not build; run 'CARGO_TARGET_DIR=$WORK_DIR/program-target sh $example/build.sh' and retry"
    registry_deployment_produce "$artifact" "$request"
    port_forward registry "$TESTNET_NAMESPACE" layerx-program-registry 19455 9420
    status=$(curl --silent --show-error --max-time 120 --max-filesize 1048576 --noproxy '*' \
        --cacert "$CA_DIR/ca.crt" --cert "$CA_DIR/gateway-client/cert.pem" --key "$CA_DIR/gateway-client/key.pem" \
        --connect-to 'layerx-program-registry:9420:127.0.0.1:19455' \
        --header "Authorization: Bearer $(cat "$SECRETS_DIR/registry-request.token")" \
        --header 'Content-Type: application/octet-stream' --data-binary "@$request" \
        --output "$response" --write-out '%{http_code}' \
        'https://layerx-program-registry:9420/__registry/deployments')
    [ "$status" = 200 ] || fail "the program registry refused the reference naming deployment with status $status; see $response"
    jq -e '.state == "deployed" and (.receipt_digest | test("^[0-9a-f]{64}$"))' "$response" > /dev/null \
        || fail "the program registry did not admit the reference naming deployment; see $response"
    log "reference naming program deployment admitted under receipt $(jq -r .receipt_digest "$response")"
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
    if rg -q '^LAYERX_NODE_GENESIS_HANDOVER_TRUST=' "$WORK_DIR/genesis/node.env"; then
        node_file_fetch "$data/genesis/genesis-handover-trust.lxt" "$WORK_DIR/genesis/genesis-handover-trust.lxt"
    fi
    NODE_GUARANTOR_ID=$(sed -n 's/^LAYERX_NODE_GENESIS_GUARANTOR_ID=//p' "$WORK_DIR/genesis/node.env")
    NODE_GUARANTOR_PUBLIC_KEY=$(sed -n 's/^LAYERX_NODE_GENESIS_GUARANTOR_PUBLIC_KEY=//p' "$WORK_DIR/genesis/node.env")
    NODE_SECOND_GUARANTOR_ID=$(sed -n 's/^LAYERX_NODE_SECOND_GUARANTOR_ID=//p' "$WORK_DIR/genesis/node.env")
    NODE_SECOND_GUARANTOR_PUBLIC_KEY=$(sed -n 's/^LAYERX_NODE_SECOND_GUARANTOR_PUBLIC_KEY=//p' "$WORK_DIR/genesis/node.env")
    [[ $NODE_SECOND_GUARANTOR_ID =~ ^[0-9a-f]{64}$ ]] || fail "node.env carries no second guarantor id"
    [[ $NODE_SECOND_GUARANTOR_PUBLIC_KEY =~ ^0[23][0-9a-f]{64}$ ]] || fail "node.env carries no second guarantor public key"
    [ "$NODE_SECOND_GUARANTOR_ID" != "$NODE_GUARANTOR_ID" ] || fail "guarantor identities must differ"
    NODE_SEQUENCER_ID=$(sed -n 's/^LAYERX_NODE_SEQUENCER_ID=//p' "$WORK_DIR/genesis/node.env")
    NODE_SEQUENCER_PUBLIC_KEY=$(sed -n 's/^LAYERX_NODE_SEQUENCER_PUBLIC_KEY=//p' "$WORK_DIR/genesis/node.env")
    [[ $NODE_GUARANTOR_ID =~ ^[0-9a-f]{64}$ ]] || fail "node.env carries no genesis guarantor id"
    [[ $NODE_GUARANTOR_PUBLIC_KEY =~ ^0[23][0-9a-f]{64}$ ]] || fail "node.env carries no compressed genesis guarantor public key"
    [ "$NODE_SEQUENCER_ID" = "$SEQUENCER_ID" ] || fail "the node derived sequencer id $NODE_SEQUENCER_ID but the registry trust history carries $SEQUENCER_ID"
    [ "$NODE_SEQUENCER_PUBLIC_KEY" = "$(cat "$CA_DIR/sequencer.pub.hex")" ] || fail "the node sequencer public key differs from the generated sequencer key"
    [ "$GUARANTOR_CHECKPOINT_AUTHORITY_KEY_FILE" = /var/lib/guarantor-submitter/checkpoint-authority.pem ] \
        || fail "the node manifest no longer binds every guarantor to one checkpoint authority key file"
    kube -n "$TESTNET_NAMESPACE" exec layerx-node-0 -c guarantor-1 -- \
        /opt/layerx/guarantor.sh --checkpoint-authority-public \
        "$GUARANTOR_CHECKPOINT_AUTHORITY_KEY_FILE" > "$WORK_DIR/genesis/checkpoint-authority.public.hex"
    local checkpoint_authority
    checkpoint_authority=$(deposit_root_authority "$WORK_DIR/genesis")
    apply_secret "$TESTNET_NAMESPACE" layerx-guarantor-checkpoint-authority \
        --from-file=public.hex="$WORK_DIR/genesis/checkpoint-authority.public.hex"
    log "guarantor checkpoint authority $checkpoint_authority signs the deposit roots of this cluster"
}

# The guarantor that publishes a checkpoint signs its deposit-root registration with the Ed25519 key at
# $GUARANTOR_CHECKPOINT_AUTHORITY_KEY_FILE: cmd/layerx-guarantor/authorization.py signs with the policy
# field deposit_authority_key_file, which platform/hosted/tests/publication-policy.py sets to that path,
# and every guarantor container mounts it from the one shared guarantor-submitter volume subpath. The
# vault deposit-root authority must therefore be that key's public half.
deposit_root_authority() {
    local source="$1/checkpoint-authority.public.hex" key
    [ -r "$source" ] || fail "the guarantor checkpoint authority public key is unavailable at $source"
    key=$(tr -d '[:space:]' < "$source")
    [[ $key =~ ^0x[0-9a-f]{64}$ ]] || fail "the guarantor checkpoint authority public key at $source is not a 32-byte Ed25519 key"
    [ "$key" != "0x$(printf '%064d' 0)" ] || fail "the deposit-root authority public key must be nonzero"
    printf '%s\n' "$key"
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

deposit_root_authority_test() (
    set -euo pipefail
    local dir key der expected policy scenario
    dir=$(mktemp -d)
    trap 'rm -rf "$dir"' EXIT
    umask 077
    mkdir -p "$dir/genesis"
    bash "$REPO_ROOT/platform/hosted/node/guarantor.sh" --checkpoint-authority-public \
        "$dir/checkpoint-authority.pem" > "$dir/genesis/checkpoint-authority.public.hex"
    key=$(deposit_root_authority "$dir/genesis")
    [[ $key =~ ^0x[0-9a-fA-F]{64}$ ]] || fail "the extracted deposit-root authority is not a 32-byte key"
    [ "$key" != "0x$(printf '%064d' 0)" ] || fail "the extracted deposit-root authority is zero"
    der=$(openssl pkey -in "$dir/checkpoint-authority.pem" -pubout -outform DER | od -An -v -tx1 | tr -d ' \n')
    [ "${#der}" -eq 88 ] && [ "${der:0:24}" = 302a300506032b6570032100 ] \
        || fail "the guarantor checkpoint authority key is not Ed25519"
    expected="0x${der:24}"
    [ "$key" = "$expected" ] || fail "the deposit-root authority $key differs from the signing key public half $expected"
    policy="$dir/authorization.json"
    python3 "$REPO_ROOT/platform/hosted/tests/publication-policy.py" authorization "$policy" \
        "$NODE_NETWORK_ID" "$PAXEER_CHAIN_ID" 0x"$(printf '%040d' 1)" 0x"$(printf '%040d' 2)" \
        0x"$(printf '%040d' 3)" "$(printf '%064d' 4)" "$NODE_ASSET_ID" "$(printf '%040d' 5)"
    [ "$(jq -r '.deposit_authority_key_file' "$policy")" = "$GUARANTOR_CHECKPOINT_AUTHORITY_KEY_FILE" ] \
        || fail "the guarantor publication policy signs deposit roots with a different key file than the bring-up reads"
    for scenario in missing malformed zero; do
        case "$scenario" in
            missing) rm -f "$dir/genesis/checkpoint-authority.public.hex" ;;
            malformed) printf '0x%s\n' "${key:2:62}" > "$dir/genesis/checkpoint-authority.public.hex" ;;
            zero) printf '0x%064d\n' 0 > "$dir/genesis/checkpoint-authority.public.hex" ;;
        esac
        if (deposit_root_authority "$dir/genesis") > "$dir/refusal.log" 2>&1; then
            fail "a $scenario deposit-root authority was accepted"
        fi
    done
    log "deposit-root authority $key extracted from the guarantor checkpoint authority key with three refusals"
)

genesis_metadata_test() (
    set -euo pipefail
    local builder directory withdrawal_fee module_fees
    builder="${LAYERX_TEST_NATIVE_BIN_DIR:-$REPO_ROOT/build/bin}/layerx-genesis-build"
    [ -x "$builder" ] || fail "the native genesis builder is missing: $builder (run: make layerx-genesis-build)"
    require_tool openssl python3
    directory=$(mktemp -d)
    trap 'rm -rf "$directory"' EXIT
    umask 077
    SECRETS_DIR="$directory/secrets"
    mkdir -m 0700 "$SECRETS_DIR"
    (umask 077; random_hex 32 > "$SECRETS_DIR/node-treasury.key")
    genesis_metadata_generate
    cp "$SECRETS_DIR/node-genesis-metadata.lxgb" "$directory/first.lxgb"
    rm "$SECRETS_DIR/node-genesis-metadata.lxgb"
    genesis_metadata_generate
    cmp -s "$directory/first.lxgb" "$SECRETS_DIR/node-genesis-metadata.lxgb" \
        || fail "the retained genesis salt did not reproduce the same metadata"
    withdrawal_fee=$(python3 - "$NODE_MANIFEST" --withdrawal-fee <<'PYARG'
import sys
import yaml
name = sys.argv[2]
for document in yaml.safe_load_all(open(sys.argv[1]).read()):
    if document and document.get('kind') == 'StatefulSet':
        for container in document['spec']['template']['spec']['containers']:
            if container['name'] == 'layerxd':
                arguments = [str(value) for value in container['args']]
                print(arguments[arguments.index(name) + 1])
PYARG
    )
    module_fees="$REPO_ROOT/platform/hosted/node/genesis-module-fees.json"
    python3 "$REPO_ROOT/platform/hosted/node/genesis_fees.py" "$SECRETS_DIR/node-genesis-metadata.lxgb" \
        "$withdrawal_fee" --module-fees "$module_fees" --check \
        || fail "bootstrap.sh fee validation refused the generated genesis metadata"
    python3 - "$REPO_ROOT" "$NODE_MANIFEST" "$SCRIPT_DIR/beta-cluster.sh" "$NODE_NETWORK_ID" "$NODE_ASSET_ID" \
        "$SECRETS_DIR/node-genesis-metadata.lxgb" "$builder" "$directory/builder" <<'PYGENESISTEST'
import hashlib
import importlib.util
import os
from pathlib import Path
import subprocess
import sys

import yaml
from cryptography.hazmat.primitives.asymmetric import ec, ed25519
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

root, manifest, script = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
network_id, asset_id = int(sys.argv[4]), sys.argv[5]
metadata = Path(sys.argv[6]).read_bytes()
builder, work = Path(sys.argv[7]), Path(sys.argv[8])

CONFIGMAP = 'layerx-node-genesis-metadata'
KEY = 'metadata.lxgb'
MOUNT = '/run/layerx/genesis/metadata.lxgb'

statefulset = next(document for document in yaml.safe_load_all(manifest.read_text())
                   if document and document.get('kind') == 'StatefulSet')
pod = statefulset['spec']['template']['spec']
layerxd = next(container for container in pod['containers'] if container['name'] == 'layerxd')
mount = next(entry for entry in layerxd['volumeMounts'] if entry['mountPath'] == MOUNT)
if mount.get('subPath') != KEY:
    raise SystemExit('the node manifest mounts %s from subPath %r, not %r' % (MOUNT, mount.get('subPath'), KEY))
volume = next(entry for entry in pod['volumes'] if entry['name'] == mount['name'])
source = volume['configMap']
if source['name'] != CONFIGMAP or source.get('optional'):
    raise SystemExit('the node manifest mounts ConfigMap %r, not a required %r' % (source['name'], CONFIGMAP))
if [item['key'] for item in source['items']] != [KEY]:
    raise SystemExit('the node manifest reads keys %r from %s' % (source['items'], CONFIGMAP))
applied = 'apply_configmap "$ns" %s --from-file=%s=' % (CONFIGMAP, KEY)
if applied not in script.read_text():
    raise SystemExit('beta-cluster.sh does not publish %s with key %s' % (CONFIGMAP, KEY))

def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, root / relative)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


lxgb = load('lxgb_metadata', 'tests/support/lxgb_metadata.py')
vector = lxgb.check()
if len(vector) != lxgb.VECTOR_LENGTH or hashlib.sha256(vector).hexdigest() != lxgb.VECTOR_SHA256:
    raise SystemExit('the pinned lxgb metadata fixture vector changed')

declared = dict(line.split('=', 1) for line in (root / 'platform/hosted/node/bootstrap.sh').read_text().splitlines()
                if line.startswith(('ASSET_SYMBOL=', 'ASSET_DECIMALS=')))
if int.from_bytes(metadata[:2], 'big') != 1:
    raise SystemExit('the genesis metadata does not carry exactly one asset record')
asset_record = metadata[4:4 + int.from_bytes(metadata[2:4], 'big')]
symbol_length = asset_record[34]
symbol = asset_record[35:35 + symbol_length].decode('ascii')
decimals = asset_record[35 + symbol_length]
if asset_record[2:34].hex() != asset_id:
    raise SystemExit('the genesis metadata describes asset %s, not %s' % (asset_record[2:34].hex(), asset_id))
if symbol != declared['ASSET_SYMBOL'] or decimals != int(declared['ASSET_DECIMALS']):
    raise SystemExit('the genesis metadata describes %s with %d decimals, not the %s with %s decimals '
                     'bootstrap.sh declares'
                     % (symbol, decimals, declared['ASSET_SYMBOL'], declared['ASSET_DECIMALS']))

producer = load('prepare_beta', 'platform/hosted/paxeer/prepare-beta.py')

work.mkdir(mode=0o700)
signer = work / 'signer'
with os.fdopen(os.open(signer, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'wb') as handle:
    handle.write(ed25519.Ed25519PrivateKey.generate().private_bytes_raw())
members = []
for index in range(1, 4):
    public = ec.generate_private_key(ec.SECP256K1()).public_key().public_bytes(
        Encoding.X962, PublicFormat.CompressedPoint)
    members.append({'guarantor_id': index.to_bytes(32, 'big').hex(), 'public_key': public.hex()})
request = producer.genesis_request(members, network_id, asset_id, 1700000000000, metadata)
if not request.endswith(metadata) or request[:5] != b'LXGB\x02':
    raise SystemExit('the genesis request did not carry the generated metadata suffix')

for label, payload, accepted in (('accepted', request, True), ('truncated', request[:-1], False),
                                 ('trailing', request + b'\0', False),
                                 ('stripped', request[:-len(metadata)], False)):
    path = work / (label + '.lxgb')
    path.write_bytes(payload)
    result = subprocess.run([str(builder), str(path), str(signer), str(work / label)],
                            cwd=root, capture_output=True)
    if (result.returncode == 0) != accepted:
        raise SystemExit('%s: native builder exit %d' % (label, result.returncode))
    if (work / label / 'genesis.manifest').exists() != accepted:
        raise SystemExit('%s: signed genesis artifacts were %s' % (label, 'not written' if accepted else 'written'))
print('beta-cluster: genesis metadata %d bytes describing %s with %d decimals accepted by the native builder; '
      '%s/%s matches the node manifest and the pinned fixture vector %s is unchanged'
      % (len(metadata), symbol, decimals, CONFIGMAP, KEY, lxgb.VECTOR_SHA256))
PYGENESISTEST
    log "genesis metadata producer, fee validation and native builder acceptance passed"
)

paxeer_contracts_deploy() {
    [ "${LAYERX_BETA_FORBIDDEN_CHAIN_ID:-}" != "$PAXEER_CHAIN_ID" ] \
        || fail "contract transactions on chain $PAXEER_CHAIN_ID are forbidden by LAYERX_BETA_FORBIDDEN_CHAIN_ID"
    local dir="$WORK_DIR/paxeer" signer bond_amount count index id public controller previous_id="" authority
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
    authority=$(deposit_root_authority "$WORK_DIR/genesis")
    log "deploying the settlement contracts from the node genesis through the Paxeer boundary with deposit-root authority $authority"
    if ! LAYERX_PAXEER_BOUNDARY_URL="$PAXEER_URL" LAYERX_PAXEER_BOUNDARY_CA_DER="$CA_DIR/ca.der" LAYERX_PAXEER_CHAIN_ID="$PAXEER_CHAIN_ID" \
        LAYERX_PAXEER_CHECKPOINT_SUBMITTER_KEY_FILE="$SECRETS_DIR/paxeer-checkpoint-submitter.key" \
        LAYERX_PAXEER_DEPLOYER_KEY_FILE="$SECRETS_DIR/paxeer-deployer.key" LAYERX_PAXEER_GENESIS_DIR="$WORK_DIR/genesis" \
        LAYERX_PAXEER_DEPLOYMENT_INPUT="$dir/deployment-input.json" LAYERX_PAXEER_GUARANTORS="$dir/guarantors.json" \
        LAYERX_PAXEER_GUARANTOR_KEYS_DIR="$dir/guarantor-keys" LAYERX_PAXEER_DEPLOYMENT_RECORD="$dir/deployment.json" \
        LAYERX_PAXEER_SETTLEMENT_JSON="$dir/checkpoint-settlement.json" LAYERX_PAXEER_SETTLEMENT_DOMAIN=beta \
        LAYERX_PAXEER_FOUNDRY_BIN="$FOUNDRY_BIN" \
        bash "$REPO_ROOT/platform/hosted/paxeer/deploy-contracts.sh" bootstrap "$authority" > "$LOG_DIR/deploy-contracts.log" 2>&1; then
        tail -n 40 "$LOG_DIR/deploy-contracts.log" >&2
        fail "deploy-contracts.sh bootstrap failed (log $LOG_DIR/deploy-contracts.log)"
    fi
    GUARANTOR_BOND=$(jq -r '.addresses.guarantor_bond' "$dir/deployment.json")
    CHECKPOINT_REGISTRY=$(jq -r '.addresses.checkpoint_registry' "$dir/deployment.json")
    [[ $GUARANTOR_BOND =~ ^0x[0-9a-fA-F]{40}$ ]] && [[ $CHECKPOINT_REGISTRY =~ ^0x[0-9a-fA-F]{40}$ ]] || fail "the deployment record lacks the GuarantorBond and CheckpointRegistry addresses"
    [ "$(jq -r '.network_id' "$dir/deployment.json")" = "$NODE_NETWORK_ID" ] || fail "the deployed CheckpointRegistry network id differs from the node network id $NODE_NETWORK_ID"
    jq -e --arg authority "$authority" '.deposit_root_authority.public_key == $authority and .deposit_root_authority.executed == true' \
        "$dir/deployment.json" > /dev/null || fail "the deployment record does not carry the executed guarantor deposit-root authority $authority"
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
    apply_configmap "$ns" layerx-node-settlement --from-file=settlement.env="$WORK_DIR/paxeer/settlement.env" \
        --from-file=checkpoint-settlement.json="$WORK_DIR/paxeer/checkpoint-settlement.json"
    publication_authorization_publish
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
    node_exec sh -ec 'for identity in 1 2; do install -m 0440 /var/lib/layerx/node/genesis/genesis.registration "/var/lib/layerx/guarantor-$identity/identity/genesis.registration"; done'
}

mirror_publish() {
    local dir="$WORK_DIR/mirror" ns="$TESTNET_NAMESPACE" variable field
    local ethereum_chain_id ethereum_primary ethereum_secondary publisher_address publisher_public_key
    local solana_public solana_program solana_program_data solana_loader solana_code_hash solana_genesis
    local publisher_balance publisher_gas solana_record solana_deploy_url
    local -a missing=() solana_present=() solana_absent=()
    for variable in "${MIRROR_SOLANA_INPUTS[@]}"; do
        if [ -n "${!variable:-}" ]; then solana_present+=("$variable"); else solana_absent+=("$variable"); fi
    done
    if [ "${#solana_present[@]}" -eq 0 ]; then
        MIRROR_SOLANA=0
        MISSING_INPUTS+=("${MIRROR_SOLANA_INPUTS[*]}: the Solana mirror target; the bring-up mirrors every batch archive to the EVM chain only until all of them are set")
        log "mirror: no Solana inputs are set, so the publisher mirrors to the EVM chain only"
    elif [ "${#solana_absent[@]}" -ne 0 ]; then
        fail "the Solana mirror target needs every one of its owner inputs or none of them; set ${solana_absent[*]} as well, or unset ${solana_present[*]}"
    else
        MIRROR_SOLANA=1
    fi
    if [ -n "${LAYERX_BETA_MIRROR_ETHEREUM_RPC_URL:-}" ]; then
        for variable in LAYERX_BETA_MIRROR_ETHEREUM_RPC_URL_SECONDARY LAYERX_BETA_MIRROR_ETHEREUM_RPC_CA_FILE \
            LAYERX_BETA_MIRROR_ETHEREUM_RPC_TOKEN_FILE LAYERX_BETA_MIRROR_ETHEREUM_RPC_SECONDARY_TOKEN_FILE \
            LAYERX_BETA_MIRROR_ETHEREUM_CHAIN_ID LAYERX_BETA_MIRROR_ETHEREUM_DEPLOYER_KEY_FILE; do
            [ -n "${!variable:-}" ] || missing+=("$variable")
        done
    fi
    [ "${#missing[@]}" -eq 0 ] \
        || fail "the mirror publisher needs these owner inputs before the beta cluster can publish LayerX archives: ${missing[*]}"
    mkdir -p "$dir/secrets"
    chmod 0700 "$dir" "$dir/secrets"

    if [ "$MIRROR_SOLANA" = 1 ]; then
        for variable in LAYERX_BETA_MIRROR_SOLANA_RPC_CA_FILE LAYERX_BETA_MIRROR_SOLANA_RPC_TOKEN_FILE \
            LAYERX_BETA_MIRROR_SOLANA_RPC_SECONDARY_TOKEN_FILE LAYERX_BETA_MIRROR_SOLANA_KEYPAIR_FILE; do
            [ -r "${!variable}" ] || fail "$variable=${!variable} is not readable"
        done
        if [ -n "${LAYERX_BETA_MIRROR_SOLANA_DEPLOYMENT_FILE:-}" ]; then
            [ -r "$LAYERX_BETA_MIRROR_SOLANA_DEPLOYMENT_FILE" ] \
                || fail "LAYERX_BETA_MIRROR_SOLANA_DEPLOYMENT_FILE=$LAYERX_BETA_MIRROR_SOLANA_DEPLOYMENT_FILE is not readable"
            solana_record=$LAYERX_BETA_MIRROR_SOLANA_DEPLOYMENT_FILE
        else
            [ -n "${LAYERX_BETA_MIRROR_SOLANA_TOOLCHAIN_BIN:-}" ] \
                || fail "LAYERX_BETA_MIRROR_SOLANA_TOOLCHAIN_BIN is unset; it must hold the pinned solana, solana-keygen and cargo-build-sbf so interop/deploy/mirror/deploy-solana-mirror.sh can build and deploy the solana-mirror program, or set LAYERX_BETA_MIRROR_SOLANA_DEPLOYMENT_FILE to a record produced elsewhere"
            solana_deploy_url=${LAYERX_BETA_MIRROR_SOLANA_DEPLOY_RPC_URL:-$LAYERX_BETA_MIRROR_SOLANA_RPC_URL}
            solana_record="$dir/solana-deployment.json"
            log "mirror: deploying the solana-mirror program through $solana_deploy_url"
            if ! LAYERX_SOLANA_MIRROR_RPC_URL="$solana_deploy_url" \
                LAYERX_SOLANA_MIRROR_KEYPAIR_FILE="$LAYERX_BETA_MIRROR_SOLANA_KEYPAIR_FILE" \
                LAYERX_SOLANA_MIRROR_DEPLOYMENT_RECORD="$solana_record" \
                LAYERX_SOLANA_MIRROR_TOOLCHAIN_BIN="$LAYERX_BETA_MIRROR_SOLANA_TOOLCHAIN_BIN" \
                bash "$REPO_ROOT/interop/deploy/mirror/deploy-solana-mirror.sh" \
                > "$LOG_DIR/mirror-solana-deploy.log" 2>&1; then
                tail -n 40 "$LOG_DIR/mirror-solana-deploy.log" >&2
                fail "the solana-mirror program could not be deployed (log $LOG_DIR/mirror-solana-deploy.log)"
            fi
        fi

        for field in genesis_hash program_id publisher_ed25519_public_key program_data_account \
            upgradeable_loader_id program_elf_sha256 deployment_signature rooted_slot; do
            jq -er --arg field "$field" 'has($field) and (.[$field] | tostring | length > 0)' \
                "$solana_record" > /dev/null \
                || fail "the Solana deployment record $solana_record records no $field; interop/deploy/mirror/solana-deployment.json lists every required post-deploy record"
        done
        solana_genesis=$(jq -r '.genesis_hash' "$solana_record")
        solana_program=$(jq -r '.program_id' "$solana_record")
        solana_program_data=$(jq -r '.program_data_account' "$solana_record")
        solana_loader=$(jq -r '.upgradeable_loader_id' "$solana_record")
        solana_code_hash=$(jq -r '.program_elf_sha256' "$solana_record")
        solana_public=$(python3 "$REPO_ROOT/platform/hosted/tests/mirror-identity.py" solana-public-key "$LAYERX_BETA_MIRROR_SOLANA_KEYPAIR_FILE") \
            || fail "LAYERX_BETA_MIRROR_SOLANA_KEYPAIR_FILE is not a 64-byte Solana keypair"
        [ "$solana_public" = "$(jq -r '.publisher_ed25519_public_key' "$solana_record")" ] \
            || fail "the Solana deployment record $solana_record was produced for another publisher than LAYERX_BETA_MIRROR_SOLANA_KEYPAIR_FILE"
    fi

    [ -r "$SECRETS_DIR/mirror-ethereum-publisher.pub.hex" ] \
        || fail "the Ethereum mirror publisher key is missing; the bring-up generates it in secrets_generate"
    publisher_public_key=$(cat "$SECRETS_DIR/mirror-ethereum-publisher.pub.hex")
    [[ $publisher_public_key =~ ^0[23][0-9a-fA-F]{64}$ ]] \
        || fail "the Ethereum mirror publisher key is not a compressed secp256k1 public key"
    if [ -n "${LAYERX_BETA_MIRROR_ETHEREUM_SIGNER_PUBLIC_KEY:-}" ]; then
        [ "${LAYERX_BETA_MIRROR_ETHEREUM_SIGNER_PUBLIC_KEY#0x}" = "$publisher_public_key" ] \
            || fail "LAYERX_BETA_MIRROR_ETHEREUM_SIGNER_PUBLIC_KEY is not the public key of the mirror publisher key"
    fi
    publisher_address=$(PATH="$FOUNDRY_BIN:$PATH" python3 "$REPO_ROOT/platform/hosted/paxeer/settlement-domain.py" \
        signer "0x$publisher_public_key") \
        || fail "the mirror publisher address could not be derived from its public key"
    if [ "$MIRROR_SOLANA" = 1 ]; then
        apply_secret "$ns" layerx-mirror-signer \
            --from-file=ethereum.key="$SECRETS_DIR/mirror-ethereum-publisher.key" \
            --from-file=solana.json="$LAYERX_BETA_MIRROR_SOLANA_KEYPAIR_FILE"
    else
        apply_secret "$ns" layerx-mirror-signer \
            --from-file=ethereum.key="$SECRETS_DIR/mirror-ethereum-publisher.key"
    fi

    if [ -n "${LAYERX_BETA_MIRROR_ETHEREUM_RPC_URL:-}" ]; then
        ethereum_chain_id=$LAYERX_BETA_MIRROR_ETHEREUM_CHAIN_ID
        ethereum_primary=$LAYERX_BETA_MIRROR_ETHEREUM_RPC_URL
        ethereum_secondary=$LAYERX_BETA_MIRROR_ETHEREUM_RPC_URL_SECONDARY
        cp "$LAYERX_BETA_MIRROR_ETHEREUM_RPC_CA_FILE" "$dir/ethereum-ca.pem"
        (umask 077; cp "$LAYERX_BETA_MIRROR_ETHEREUM_RPC_TOKEN_FILE" "$dir/secrets/ethereum-a.token")
        (umask 077; cp "$LAYERX_BETA_MIRROR_ETHEREUM_RPC_SECONDARY_TOKEN_FILE" "$dir/secrets/ethereum-b.token")
        log "mirror: publishing to the owner Ethereum chain $ethereum_chain_id at $ethereum_primary"
        LAYERX_MIRROR_DEPLOYER_KEY_FILE=$LAYERX_BETA_MIRROR_ETHEREUM_DEPLOYER_KEY_FILE
    else
        [ "${LAYERX_BETA_FORBIDDEN_CHAIN_ID:-}" != "$PAXEER_CHAIN_ID" ] \
            || fail "contract transactions on chain $PAXEER_CHAIN_ID are forbidden by LAYERX_BETA_FORBIDDEN_CHAIN_ID"
        ethereum_chain_id=$PAXEER_CHAIN_ID
        ethereum_primary="https://paxeer-boundary.$ns.svc.cluster.local:9443"
        ethereum_secondary="https://paxeer-observer-boundary.$ns.svc.cluster.local:9443"
        cp "$CA_DIR/ca.pem" "$dir/ethereum-ca.pem"
        write_token "$dir/secrets/ethereum-a.token"
        write_token "$dir/secrets/ethereum-b.token"
        log "mirror: publishing to the in-cluster Paxeer EVM chain $PAXEER_CHAIN_ID"
        LAYERX_MIRROR_DEPLOYER_KEY_FILE=$SECRETS_DIR/paxeer-deployer.key
    fi
    openssl x509 -in "$dir/ethereum-ca.pem" -outform DER -out "$dir/secrets/ethereum.ca.der"
    if [ "$MIRROR_SOLANA" = 1 ]; then
        openssl x509 -in "$LAYERX_BETA_MIRROR_SOLANA_RPC_CA_FILE" -outform DER -out "$dir/secrets/solana.ca.der"
        (umask 077; cp "$LAYERX_BETA_MIRROR_SOLANA_RPC_TOKEN_FILE" "$dir/secrets/solana-a.token")
        (umask 077; cp "$LAYERX_BETA_MIRROR_SOLANA_RPC_SECONDARY_TOKEN_FILE" "$dir/secrets/solana-b.token")
    fi

    if ! LAYERX_MIRROR_RPC_URL="${LAYERX_BETA_MIRROR_ETHEREUM_RPC_URL:-$PAXEER_URL}" \
        LAYERX_MIRROR_RPC_CA_PEM="$dir/ethereum-ca.pem" LAYERX_MIRROR_CHAIN_ID="$ethereum_chain_id" \
        LAYERX_MIRROR_DEPLOYER_KEY_FILE="$LAYERX_MIRROR_DEPLOYER_KEY_FILE" \
        LAYERX_MIRROR_PUBLISHER_ADDRESS="$publisher_address" \
        LAYERX_MIRROR_DEPLOYMENT_RECORD="$dir/ethereum-deployment.json" \
        LAYERX_MIRROR_FOUNDRY_BIN="$FOUNDRY_BIN" \
        bash "$REPO_ROOT/interop/deploy/mirror/deploy-ethereum-mirror.sh" > "$LOG_DIR/mirror-deploy.log" 2>&1; then
        tail -n 40 "$LOG_DIR/mirror-deploy.log" >&2
        fail "the Ethereum mirror archive contract could not be deployed (log $LOG_DIR/mirror-deploy.log)"
    fi

    publisher_gas=${LAYERX_BETA_MIRROR_PUBLISHER_GAS_WEI:-1000000000000000000}
    [[ $publisher_gas =~ ^[1-9][0-9]*$ ]] \
        || fail "LAYERX_BETA_MIRROR_PUBLISHER_GAS_WEI must be a positive decimal amount of wei"
    publisher_balance=$(SSL_CERT_FILE="$dir/ethereum-ca.pem" "$FOUNDRY_BIN/cast" balance \
        --rpc-url "$ethereum_primary" "$publisher_address") \
        || fail "the mirror publisher balance could not be read from $ethereum_primary"
    if [ "$(printf '%s\n' "$publisher_balance" "$publisher_gas" | sort -n | head -1)" != "$publisher_gas" ]; then
        SSL_CERT_FILE="$dir/ethereum-ca.pem" "$FOUNDRY_BIN/cast" send --json --timeout 120 \
            --rpc-url "$ethereum_primary" --chain "$ethereum_chain_id" \
            --private-key "$(cat "$LAYERX_MIRROR_DEPLOYER_KEY_FILE")" --value "$publisher_gas" \
            "$publisher_address" > "$dir/publisher-gas.json" \
            || fail "the mirror publisher could not be funded for gas on chain $ethereum_chain_id"
        [ "$(jq -r '.status' "$dir/publisher-gas.json")" = "0x1" ] \
            || fail "the mirror publisher gas transfer failed on chain $ethereum_chain_id"
        log "mirror: funded the publisher $publisher_address with $publisher_gas wei of gas"
    fi

    local -a solana_arguments=()
    if [ "$MIRROR_SOLANA" = 1 ]; then
        solana_arguments=(
            --solana-endpoint "$LAYERX_BETA_MIRROR_SOLANA_RPC_URL,solana-a,/run/secrets/solana.ca.der,/run/secrets/solana-a.token"
            --solana-endpoint "$LAYERX_BETA_MIRROR_SOLANA_RPC_URL_SECONDARY,solana-b,/run/secrets/solana.ca.der,/run/secrets/solana-b.token"
            --solana-genesis-hash "$solana_genesis"
            --solana-archive-program "$solana_program"
            --solana-upgradeable-loader "$solana_loader"
            --solana-program-data-account "$solana_program_data"
            --solana-program-code-hash "$solana_code_hash"
            --solana-signer-key-handle "$MIRROR_SOLANA_KEY_HANDLE"
            --solana-signer-public-key "$solana_public"
            --solana-signer-socket "$MIRROR_SIGNER_SOCKET"
        )
    fi
    python3 "$REPO_ROOT/interop/deploy/mirror/render-config.py" \
        --output "$dir/config.json" \
        --state-directory /var/lib/layerx-mirror \
        --first-batch-number 1 \
        --status-listen 127.0.0.1:9091 \
        --lni-socket /run/layerx/node/layerxd.lni.sock \
        --network-id "$NODE_NETWORK_ID" \
        --protocol-version 3 \
        --ethereum-endpoint "$ethereum_primary,ethereum-a,/run/secrets/ethereum.ca.der,/run/secrets/ethereum-a.token" \
        --ethereum-endpoint "$ethereum_secondary,ethereum-b,/run/secrets/ethereum.ca.der,/run/secrets/ethereum-b.token" \
        --ethereum-chain-id "$ethereum_chain_id" \
        --ethereum-genesis-hash "$(jq -r '.genesis_hash' "$dir/ethereum-deployment.json")" \
        --ethereum-archive-contract "$(jq -r '.contract_address' "$dir/ethereum-deployment.json")" \
        --ethereum-archive-code-hash "$(jq -r '.runtime_code_keccak256' "$dir/ethereum-deployment.json")" \
        --ethereum-signer-key-handle "$MIRROR_ETHEREUM_KEY_HANDLE" \
        --ethereum-signer-public-key "$publisher_public_key" \
        --ethereum-signer-socket "$MIRROR_SIGNER_SOCKET" \
        "${solana_arguments[@]}" \
        || fail "the mirror publisher configuration could not be rendered from the deployed identities"

    apply_secret "$ns" layerx-mirror-config --from-file=config.json="$dir/config.json"
    local -a rpc_credentials=(
        --from-file=ethereum.ca.der="$dir/secrets/ethereum.ca.der"
        --from-file=ethereum-a.token="$dir/secrets/ethereum-a.token"
        --from-file=ethereum-b.token="$dir/secrets/ethereum-b.token"
    )
    if [ "$MIRROR_SOLANA" = 1 ]; then
        rpc_credentials+=(
            --from-file=solana.ca.der="$dir/secrets/solana.ca.der"
            --from-file=solana-a.token="$dir/secrets/solana-a.token"
            --from-file=solana-b.token="$dir/secrets/solana-b.token"
        )
    fi
    apply_secret "$ns" layerx-mirror-rpc-credentials "${rpc_credentials[@]}"

    if [ "$MIRROR_SOLANA" = 1 ]; then
        printf 'Ethereum %s on chain %s, Solana program %s\n' \
            "$(jq -r '.contract_address' "$dir/ethereum-deployment.json")" "$ethereum_chain_id" "$solana_program" \
            > "$dir/summary"
    else
        printf 'Ethereum %s on chain %s, no Solana mirror target configured\n' \
            "$(jq -r '.contract_address' "$dir/ethereum-deployment.json")" "$ethereum_chain_id" \
            > "$dir/summary"
    fi
    log "mirror publication material published: $(cat "$dir/summary")"
}

mirror_ready() {
    # The mirror publisher is a container of the layerx-node pod, so its status
    # endpoint stays on the pod loopback and is probed with its own --probe.
    local deadline=$((SECONDS + READY_TIMEOUT))
    while :; do
        if kube -n "$TESTNET_NAMESPACE" exec layerx-node-0 -c mirror-publisher -- \
            /usr/local/bin/layerx-mirror-publisher --probe 127.0.0.1:9091 > /dev/null 2>&1; then
            log "mirror publisher ready: $(cat "$WORK_DIR/mirror/summary")"
            return 0
        fi
        [ "$SECONDS" -lt "$deadline" ] || break
        sleep 5
    done
    kube -n "$TESTNET_NAMESPACE" logs layerx-node-0 -c mirror-publisher --tail=40 >&2 || true
    kube -n "$TESTNET_NAMESPACE" logs layerx-node-0 -c mirror-signer --tail=40 >&2 || true
    fail "the mirror publisher did not report ready on its own status endpoint within ${READY_TIMEOUT}s"
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
    local dir="$WORK_DIR/identity" status tenant="${LAYERX_BETA_HUMAN_TENANT:-beta}"
    local provision="$REPO_ROOT/platform/hosted/human/provision.py"
    umask 077
    mkdir -p "$dir"
    chmod 0700 "$dir"
    jq -n --arg tenant "$tenant" --arg sub "$TEST_SOURCE_DID" --arg key "$(cat "$SECRETS_DIR/test-source-signer.pub.hex")" \
        '{tenant: $tenant, sub: $sub, allowed_signer_public_keys: [$key]}' > "$dir/source-principal.json"
    status=$(identity_request POST /v1/principals "$dir/source-principal.json" "$dir/source-principal.response.json")
    [ "$status" = 201 ] || [ "$status" = 200 ] || fail "identity refused the smoke source principal with status $status: $(cat "$dir/source-principal.response.json")"
    jq -n --arg tenant "$tenant" --arg sub "$TEST_DESTINATION_DID" --arg key "$(cat "$SECRETS_DIR/test-destination-signer.pub.hex")" \
        '{tenant: $tenant, sub: $sub, allowed_signer_public_keys: [$key]}' > "$dir/destination-principal.json"
    status=$(identity_request POST /v1/principals "$dir/destination-principal.json" "$dir/destination-principal.response.json")
    [ "$status" = 201 ] || [ "$status" = 200 ] || fail "identity refused the smoke destination principal with status $status: $(cat "$dir/destination-principal.response.json")"
    python3 "$provision" --preserve-binding --work-dir "$WORK_DIR" \
        --request "$dir/source-principal.json" --response "$dir/source-principal.response.json" \
        --output "$dir/source-binding.json"
    python3 "$provision" --preserve-binding --work-dir "$WORK_DIR" \
        --request "$dir/destination-principal.json" --response "$dir/destination-principal.response.json" \
        --output "$dir/destination-binding.json"
    if [ "$TEST_AUTH_SOURCE" = identity-provisioning ]; then
        jq -n --arg sub "$TEST_SOURCE_DID" '{sub: $sub}' > "$dir/source-session.json"
        (umask 077; : > "$dir/source-session.response.json")
        status=$(identity_request POST /v1/sessions "$dir/source-session.json" "$dir/source-session.response.json")
        [ "$status" = 201 ] || [ "$status" = 200 ] || fail "identity refused the smoke source session with status $status"
        (umask 077; jq -r '.token' "$dir/source-session.response.json" > "$SECRETS_DIR/test-auth.token")
        grep -Eq '^ses_[0-9a-f]{32}\.[0-9a-f]{64}$' "$SECRETS_DIR/test-auth.token" || fail "identity returned no session token for the smoke source"
        rm -f "$dir/source-session.response.json"
    fi
    jq -n --arg sub "$TEST_DESTINATION_DID" '{sub: $sub}' > "$dir/destination-session.json"
    status=$(identity_request POST /v1/sessions "$dir/destination-session.json" "$dir/destination-session.response.json")
    [ "$status" = 201 ] || [ "$status" = 200 ] || fail "identity refused the smoke destination session with status $status"
    (umask 077; jq -er '.token' "$dir/destination-session.response.json" > "$SECRETS_DIR/test-destination-auth.token")
    grep -Eq '^ses_[0-9a-f]{32}\.[0-9a-f]{64}$' "$SECRETS_DIR/test-destination-auth.token" || fail "identity returned no destination session token"
    rm -f "$dir/destination-session.response.json"
    log "identity provisioned $TEST_SOURCE_DID and $TEST_DESTINATION_DID (session token source: $TEST_AUTH_SOURCE)"
}

internal_principals_provision() {
    local service scope status producer dir="$WORK_DIR/internal-principals"
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
        if [ "$service" = programs ]; then
            (umask 077; jq -n --arg sub "$TEST_SOURCE_DID" '{sub: $sub, revoked: false}' > "$dir/publication-binding.json")
            status=$(curl --silent --show-error --max-time 30 --cacert "$CA_DIR/ca.crt" \
                --header "Authorization: Bearer $(cat "$SECRETS_DIR/identity-tokens/provisioning")" \
                --header "LayerX-Key: $(cat "$dir/$service.credential")" --header 'Content-Type: application/json' \
                --data-binary "@$dir/publication-binding.json" --output "$dir/publication-binding.response.json" \
                --write-out '%{http_code}' "$IDENTITY_URL/v1/publication-keys")
            [ "$status" = 200 ] || fail "identity refused program publication key binding with status $status"
        fi
        (umask 077; jq -n --arg sub "$TEST_SOURCE_DID" '{($sub): "/run/layerx/principal.credential"}' > "$dir/credentials.json")
        producer=gateway
        if [ "$service" = programs ]; then producer=registry; fi
        apply_secret "$INTERNAL_NAMESPACE" "layerx-internal-$service-runtime" \
            --from-file=producers.json="$SECRETS_DIR/$service-producers.json" \
            --from-file=producer-token="$SECRETS_DIR/$producer-event-producer.token" \
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

interop_inputs_require() {
    local manifest=${LAYERX_BETA_INTEROP_MANIFEST_FILE:-}
    [ -z "$manifest" ] || [ -r "$manifest" ] || fail "LAYERX_BETA_INTEROP_MANIFEST_FILE=$manifest is not readable"
    python3 "$REPO_ROOT/interop/deploy/gateway/render.py" --check \
        || fail "the interop gateway configuration lacks deployment inputs named above; interop/deploy/gateway/README.md documents each variable"
}

interop_runtime_render() {
    interop_inputs_require
    local network
    network=$(sed -n 's/^ *- {name: LAYERX_INTEROP_NETWORK_ID, value: \([A-Za-z0-9_.-]*\)}$/\1/p' \
        "$REPO_ROOT/platform/hosted/interop/deployment.yaml")
    [ -n "$network" ] || fail "the interop deployment does not declare LAYERX_INTEROP_NETWORK_ID"
    [ -r "$SECRETS_DIR/interop-beta-roots.json" ] \
        || fail "the generated interop beta trust roots are missing; the bring-up generates them in secrets_generate"
    python3 "$REPO_ROOT/interop/deploy/gateway/render.py" --network-id "$network" \
        --sequencer-public-key-file "$SECRETS_DIR/sequencer-public-key" \
        --beta-roots-file "$SECRETS_DIR/interop-beta-roots.json" \
        --out "$SECRETS_DIR/interop-config.json" \
        || fail "the interop gateway runtime configuration was refused"
    python3 - "$SECRETS_DIR/module-registry.json" "$SECRETS_DIR/interop-modules.json" <<'PYINTEROP'
import json
import pathlib
import sys

registry, modules_out = (pathlib.Path(value) for value in sys.argv[1:])
modules_out.write_text(json.dumps({"modules": json.loads(registry.read_text())["modules"]}, indent=2) + "\n")
PYINTEROP
    [ -s "$SECRETS_DIR/interop-config.json" ] || fail "the interop gateway runtime configuration was not rendered"
}

interop_gateway_apply() {
    interop_runtime_render
    apply_secret "$TESTNET_NAMESPACE" layerx-interop-runtime \
        --from-file=config.json="$SECRETS_DIR/interop-config.json" \
        --from-file=registry.json="$SECRETS_DIR/interop-modules.json"
    kube apply -f "$MANIFESTS_DIR/interop.yaml" > /dev/null
    wait_for_pod_ready "$TESTNET_NAMESPACE" app=layerx-interop-gateway 600
    port_forward interop "$TESTNET_NAMESPACE" layerx-interop-gateway "$INTEROP_PORT" 443
    interop_gateway_ready
}

interop_gateway_ready() {
    local deadline=$((SECONDS + 300)) status
    while :; do
        status=$(curl --silent --show-error --max-time 10 --cacert "$CA_DIR/ca.crt" \
            --output "$WORK_DIR/interop-readyz.json" --write-out '%{http_code}' "$INTEROP_URL/readyz" 2>/dev/null) \
            || status=unreachable
        if [ "$status" = 200 ] \
            && jq -e '.status == "ready" and all(.components[]; . == "ready")' "$WORK_DIR/interop-readyz.json" > /dev/null 2>&1; then
            log "interop gateway ready at $INTEROP_URL"
            return 0
        fi
        if [ "$SECONDS" -ge "$deadline" ]; then
            [ -s "$WORK_DIR/interop-readyz.json" ] && cat "$WORK_DIR/interop-readyz.json" >&2
            fail "the interop gateway did not report ready at $INTEROP_URL/readyz within 300s (last HTTP status $status)"
        fi
        sleep 5
    done
}

# registry_journal_program RESULT_FILE
# Reads the program of the deployment the registry answered RESULT_FILE with, from the canonical record the
# journal materialisation exported under that response's receipt digest. The journal now holds one record
# per deployed reference program, so every caller names the deployment it means instead of taking the first
# record of the directory.
registry_journal_program() {
    local result=$1 receipt
    [ -s "$result" ] || return 1
    receipt=$(jq -r '.receipt_digest // empty' "$result") || return 1
    [[ $receipt =~ ^[0-9a-f]{64}$ ]] || return 1
    python3 - "$WORK_DIR/registry-journal/$receipt.deployment" <<'PYRECORD'
import sys
from pathlib import Path
domain = b'LayerX/programs/registry/deployment/v1\0'
record = Path(sys.argv[1])
if not record.is_file():
    raise SystemExit('registry journal holds no deployment record at ' + str(record))
data = record.read_bytes()
if not data.startswith(domain):
    raise SystemExit('deployment record is not canonically encoded')
print(data[len(domain):len(domain) + 32].hex())
PYRECORD
}

explorer_observation_publish() {
    local probe naming batch checkpoint info="$WORK_DIR/explorer-node-info.json" status
    probe=${LAYERX_BETA_EXPLORER_PROBE_PROGRAM:-}
    [ -n "$probe" ] || probe=$(registry_journal_program "$WORK_DIR/registry-deployment-result.json") || probe=
    naming=${LAYERX_BETA_EXPLORER_NAMING_PROGRAM:-}
    [ -n "$naming" ] || naming=$(registry_journal_program "$WORK_DIR/naming-deployment-result.json") || naming=
    if [[ ! $probe =~ ^[0-9a-f]{64}$ ]]; then
        MISSING_INPUTS+=("LAYERX_BETA_EXPLORER_PROBE_PROGRAM: layerx-explorer-index probes one registered program before it serves, and the reference escrow deployment record under $WORK_DIR/registry-journal supplied none; set it to the thirty-two byte program id the explorer should probe")
        return 0
    fi
    if [[ ! $naming =~ ^[0-9a-f]{64}$ ]]; then
        MISSING_INPUTS+=("LAYERX_BETA_EXPLORER_NAMING_PROGRAM: the Human web application resolves names through one naming program, and the reference naming deployment record under $WORK_DIR/registry-journal supplied none; set it to the thirty-two byte program id of a deployed naming program")
        return 0
    fi
    status=$(curl --silent --show-error --max-time 10 --cacert "$CA_DIR/ca.crt" \
        --output "$info" --write-out '%{http_code}' "$NODE_URL/v1/node-info" 2>/dev/null) || status=unreachable
    if [ "$status" != 200 ]; then
        MISSING_INPUTS+=("LAYERX_EXPLORER_OBSERVED_SEALED_BATCH: the node public read $NODE_URL/v1/node-info answered $status, so the sealed batch and finalised checkpoint the explorer index reports could not be observed")
        return 0
    fi
    batch=$(jq -r '.latest_sealed_batch // empty' "$info")
    checkpoint=$(jq -r '.latest_finalised_checkpoint // empty' "$info")
    [[ $batch =~ ^[0-9]+$ ]] || fail "node-info reported a non-numeric latest sealed batch: $batch"
    [[ $checkpoint =~ ^[0-9a-f]{64}$ ]] || fail "node-info reported an invalid finalised checkpoint: $checkpoint"
    apply_configmap "$TESTNET_NAMESPACE" layerx-explorer-index \
        --from-literal=probe-program="$probe" \
        --from-literal=naming-program="$naming" \
        --from-literal=observed-sealed-batch="$batch" \
        --from-literal=finalised-checkpoint="$checkpoint"
    EXPLORER_OBSERVATION_PUBLISHED=1
    log "explorer program index observation published: probe program $probe naming program $naming sealed batch $batch"
}

readyz() {
    curl --silent --show-error --max-time 10 --cacert "$CA_DIR/ca.crt" "$1/readyz" 2>/dev/null
}

wait_ready() {
    local deadline=$((SECONDS + READY_TIMEOUT)) body developer_ready internal_ready human_ready human_status human_web_status explorer_status
    while :; do
        body=$(readyz "$TESTNET_URL") || body=""
        human_status=$(curl --silent --show-error --max-time 10 --cacert "$CA_DIR/ca.crt" \
            --output "$WORK_DIR/human-readyz.json" --write-out '%{http_code}' "$HUMAN_URL/readyz" 2>/dev/null) || human_status=unreachable
        human_ready=false
        if [ "$human_status" = 200 ] && jq -e '.ready == true and all(.components[]; . == "ready")' "$WORK_DIR/human-readyz.json" >/dev/null 2>&1; then
            human_ready=true
        fi
        human_web_status=served-by-the-production-browser-harness
        if [ -n "$HUMAN_WEB_PORT" ]; then
            human_web_status=$(curl --silent --show-error --max-time 10 --cacert "$CA_DIR/ca.crt" \
                --resolve "$HUMAN_WEB_HOST:$HUMAN_WEB_PORT:127.0.0.1" \
                --output "$WORK_DIR/human-web-root.html" --write-out '%{http_code}' "$HUMAN_WEB_URL/" 2>/dev/null) || human_web_status=unreachable
        fi
        explorer_status=$(curl --silent --show-error --max-time 10 --cacert "$CA_DIR/ca.crt" \
            --header "Authorization: Bearer $(cat "$SECRETS_DIR/explorer-program.token")" \
            --output "$WORK_DIR/explorer-healthz.json" --write-out '%{http_code}' "$EXPLORER_INDEX_URL/healthz" 2>/dev/null) || explorer_status=unreachable
        developer_ready=$(kube -n "$DEVELOPER_NAMESPACE" get deployments -o json 2>/dev/null \
            | jq -r '[.items[] | select((.status.readyReplicas // 0) < .spec.replicas) | .metadata.name] | join(",")') \
            || developer_ready="namespace $DEVELOPER_NAMESPACE unreadable"
        internal_ready=$(kube -n "$INTERNAL_NAMESPACE" get deployments,statefulsets -o json 2>/dev/null \
            | jq -r '[.items[] | select((.status.readyReplicas // 0) < .spec.replicas) | .metadata.name] | join(",")') \
            || internal_ready="namespace $INTERNAL_NAMESPACE unreadable"
        if [ -n "$body" ] && jq -e '.state == "ready" and all(.journeys[]; .ready == true)' <<<"$body" > /dev/null 2>&1 \
            && jq -e 'all(.dependencies[]; .ready == true) and (.journeys | length) == 4' <<<"$body" > /dev/null 2>&1 && [ -z "$developer_ready" ] && [ -z "$internal_ready" ] && [ "$human_ready" = true ] && { [ -z "$HUMAN_WEB_PORT" ] || [ "$human_web_status" = 200 ]; } && [ "$explorer_status" = 200 ]; then
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
                printf 'beta-cluster: Human web application HTTP status at %s: %s\n' "$HUMAN_WEB_URL" "$human_web_status"
                printf 'beta-cluster: explorer program index HTTP status at %s: %s\n' "$EXPLORER_INDEX_URL" "$explorer_status"
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
        printf 'export LAYERX_TEST_DESTINATION_PUBLIC_KEY=%s\n' "$(cat "$SECRETS_DIR/test-destination-signer.pub.hex")"
        printf 'export LAYERX_TEST_DESTINATION_AUTH_TOKEN_FILE=%s\n' "$SECRETS_DIR/test-destination-auth.token"
        printf 'export LAYERX_TEST_SEQUENCER_PUBLIC_KEY=%s\n' "$(cat "$SECRETS_DIR/sequencer-public-key")"
        printf 'export LAYERX_TEST_SEND_ENCODER=%s\n' "$WORK_DIR/smoke-target/debug/examples/hosted-send"
        printf 'export LAYERX_BIN=%s\n' "$WORK_DIR/smoke-target/debug/layerx"
        printf 'export LAYERX_TEST_ASSET=%s\n' "$NODE_ASSET_ID"
        printf 'export LAYERX_TEST_AMOUNT=%s\n' "$TEST_AMOUNT"
        printf 'export LAYERX_TEST_ESCROW_WASM=%s\n' "$WORK_DIR/program-target/wasm32-unknown-unknown/release/layerx_reference_escrow.wasm"
        printf 'export LAYERX_GATEWAY_CA_FILE=%s\n' "$CA_DIR/ca.crt"
        printf 'export WEBHOOKS_URL=%s\n' "$DEVELOPER_URL"
        printf 'export LAYERX_AGENT_BOUNDARY_URL=%s\n' "$AGENT_URL"
        printf 'export LAYERX_AGENTD_URL=%s\n' "$AGENTD_URL"
        printf 'export LAYERX_AGENTD_CLIENT_CERT_FILE=%s\n' "$CA_DIR/agentd-client/cert.pem"
        printf 'export LAYERX_AGENTD_CLIENT_KEY_FILE=%s\n' "$CA_DIR/agentd-client/key.pem"
        printf 'export LAYERX_IDENTITY_URL=%s\n' "$IDENTITY_URL"
        printf 'export LAYERX_PAXEER_BOUNDARY_URL=%s\n' "$PAXEER_URL"
        printf 'export LAYERX_PAXEER_SETTLEMENT_CONTRACT=%s\n' "$GUARANTOR_BOND"
        printf 'export LAYERX_PAXEER_CHECKPOINT_REGISTRY=%s\n' "$CHECKPOINT_REGISTRY"
        printf 'export LAYERX_PAXEER_DEPLOYMENT_RECORD=%s\n' "$WORK_DIR/paxeer/deployment.json"
        printf 'export LAYERX_HUMAN_WEB_URL=%s\n' "$HUMAN_WEB_URL"
        printf 'export LAYERX_EXPLORER_INDEX_URL=%s\n' "$EXPLORER_INDEX_URL"
        printf 'export KUBECONFIG=%s\n' "$KUBECONFIG_FILE"
    } >> "$ENV_FILE"
    [ -s "$WORK_DIR/human-owner.env" ] || fail "human-owner.env missing after native owner production"
    cat "$WORK_DIR/human-owner.env" >> "$ENV_FILE"
    qualification_url LAYERX_QUALIFICATION_NODE_URL LAYERX_BETA_QUALIFICATION_NODE_URL "$NODE_URL" \
        "beta_driver.py --node-url: the core boundary Service layerx-pending-core (node readiness, state and receipts)"
    qualification_url LAYERX_QUALIFICATION_AGENT_URL LAYERX_BETA_QUALIFICATION_AGENT_URL "$AGENTD_URL" \
        "beta_driver.py --agentd-url: the agentd Service layerx-agentd (owner daemon readiness behind a mutually authenticated boundary)"
    qualification_url LAYERX_QUALIFICATION_HUMAN_URL LAYERX_BETA_QUALIFICATION_HUMAN_URL "$HUMAN_URL" \
        "beta_driver.py --human-service-url: layerx-human HTTPS API; /readyz verifies all production components"
    qualification_url LAYERX_QUALIFICATION_PAXEER_URL LAYERX_BETA_QUALIFICATION_PAXEER_URL "$PAXEER_URL" \
        "beta_driver.py --paxeer-testnet-url: the Paxeer boundary Service paxeer-boundary (JSON-RPC relay to the chain $PAXEER_CHAIN_ID node)"
    ramp_env_write
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
        printf 'second_guarantor_id=%s\n' "$NODE_SECOND_GUARANTOR_ID"
        printf 'guarantor_operational_independence=false\n'
        printf 'checkpoint_submitter=%s\n' "$(cat "$SECRETS_DIR/paxeer-checkpoint-submitter.address")"
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

agentd_check() {
    log "agentd: probing the published owner daemon readiness surface at $AGENTD_URL"
    sh "$REPO_ROOT/platform/hosted/agentd/probe.sh" \
        --url "$AGENTD_URL" \
        --ca "$CA_DIR/ca.crt" \
        --client-cert "$CA_DIR/agentd-client/cert.pem" \
        --client-key "$CA_DIR/agentd-client/key.pem" \
        --bearer-file "$SECRETS_DIR/human/agent/program-token" \
        || fail "the hosted agentd Service did not answer as a ready, mutually authenticated owner daemon"
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
            docker exec "$node" sh -c 'test "$(stat -c %u:%g /var/lib/layerx-program-registry-builds/slot-0)" = 4030:4030 && mountpoint -q /var/lib/layerx-program-registry-builds/slot-0'
        done
    else
        for node in $(kube get nodes -l "$BOUNDARY_LABEL=v2" -o name); do
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

builder_environment_key() {
    local vendor recipe
    vendor=$(git -C "$REPO_ROOT" rev-parse --verify "HEAD:programs/vendor") \
        || fail "programs/vendor is not committed at HEAD; $BUILDER_ENVIRONMENT_RECIPE/build-env.sh constructs the builder environment from the committed revision only"
    recipe=$(git -C "$REPO_ROOT" rev-parse --verify "HEAD:$BUILDER_ENVIRONMENT_RECIPE") \
        || fail "$BUILDER_ENVIRONMENT_RECIPE is not committed at HEAD; its build-env.sh constructs the builder environment from the committed revision only"
    printf 'LayerX/beta-cluster/builder-environment/v1\n%s\n%s\n' "$vendor" "$recipe" | sha256sum | cut -d ' ' -f 1
}

builder_environment_stage_discard() {
    case "$1" in
        "$BUILDER_ENVIRONMENT_CACHE_DIR"/.staging-*) ;;
        *) fail "refusing to remove '$1': not a builder environment staging directory under $BUILDER_ENVIRONMENT_CACHE_DIR" ;;
    esac
    chmod -R u+rwX -- "$1" || fail "could not reset the permissions of the builder environment staging directory $1"
    rm -rf -- "$1" || fail "could not remove the builder environment staging directory $1"
}

builder_environment_construct() {
    local key out stage logfile status recorded observed
    require_tool docker git tar python3 sha256sum
    [ -f "$REPO_ROOT/$BUILDER_ENVIRONMENT_RECIPE/build-env.sh" ] \
        || fail "$BUILDER_ENVIRONMENT_RECIPE/build-env.sh is missing; it constructs the hermetic builder root filesystem LAYERX_BETA_BUILDER_ENVIRONMENT_DIR otherwise names"
    git -C "$REPO_ROOT" diff --quiet HEAD -- programs/vendor "$BUILDER_ENVIRONMENT_RECIPE" \
        || fail "programs/vendor or $BUILDER_ENVIRONMENT_RECIPE carries uncommitted changes; build-env.sh binds the builder environment to the committed revision, so commit them or point LAYERX_BETA_BUILDER_ENVIRONMENT_DIR at an already constructed root filesystem"
    key=$(builder_environment_key)
    out="$BUILDER_ENVIRONMENT_CACHE_DIR/$key"
    logfile="$LOG_DIR/builder-environment.log"
    mkdir -p "$BUILDER_ENVIRONMENT_CACHE_DIR" "$LOG_DIR"
    if [ -f "$out/rootfs/bin/layerx-build" ] && [ -s "$out/environment-tree-digest" ] && [ -s "$out/source-revision" ]; then
        recorded=$(cat "$out/environment-tree-digest")
        observed=$(environment_digest "$out/rootfs") || fail "digest of the constructed builder environment $out/rootfs failed"
        [ "$recorded" = "$observed" ] \
            || fail "constructed builder environment $out no longer digests to its recorded environment-tree-digest ($recorded, observed $observed); remove that directory so up constructs it again"
        log "reusing the builder environment constructed at $out from revision $(cat "$out/source-revision") (recipe $key)"
    else
        [ ! -e "$out" ] \
            || fail "$out exists without a complete builder environment (rootfs/bin/layerx-build, environment-tree-digest and source-revision); remove that directory so up constructs it again"
        preflight_disk "$MIN_FREE_GIB" LAYERX_BETA_MIN_FREE_GIB "construct the hermetic builder environment of the committed recipe"
        log "constructing the builder environment of the committed recipe into $out (docker build, log $logfile)"
        stage=$(mktemp -d "$BUILDER_ENVIRONMENT_CACHE_DIR/.staging-XXXXXXXX") \
            || fail "could not create a builder environment staging directory under $BUILDER_ENVIRONMENT_CACHE_DIR"
        status=0
        bash "$REPO_ROOT/$BUILDER_ENVIRONMENT_RECIPE/build-env.sh" "$stage/environment" >> "$logfile" 2>&1 || status=$?
        if [ "$status" -ne 0 ]; then
            builder_environment_stage_discard "$stage"
            fail "bash $BUILDER_ENVIRONMENT_RECIPE/build-env.sh exited $status; it needs a reachable docker daemon and the committed programs/vendor and recipe trees (log $logfile)"
        fi
        mv -T -- "$stage/environment" "$out" \
            || { builder_environment_stage_discard "$stage"; fail "could not move the constructed builder environment into $out"; }
        rmdir -- "$stage"
        log "constructed the builder environment at $out (tree digest $(cat "$out/environment-tree-digest"))"
    fi
    LAYERX_BETA_BUILDER_ENVIRONMENT_DIR="$out/rootfs"
}

require_builder_environment() {
    if [ -z "${LAYERX_BETA_BUILDER_ENVIRONMENT_DIR:-}" ]; then
        builder_environment_construct
    fi
    [ -d "$LAYERX_BETA_BUILDER_ENVIRONMENT_DIR" ] || fail "LAYERX_BETA_BUILDER_ENVIRONMENT_DIR=$LAYERX_BETA_BUILDER_ENVIRONMENT_DIR is not a directory"
    [ -f "$LAYERX_BETA_BUILDER_ENVIRONMENT_DIR/bin/layerx-build" ] || fail "LAYERX_BETA_BUILDER_ENVIRONMENT_DIR=$LAYERX_BETA_BUILDER_ENVIRONMENT_DIR lacks the bin/layerx-build entrypoint"
}

custody_latest_checkpoint() {
    python3 - "$REPO_ROOT" "$PAXEER_URL" "$PAXEER_OBSERVER_URL" "$CA_DIR/ca.pem" "$WORK_DIR/paxeer/rpc-origins.json" "$CHECKPOINT_REGISTRY" <<'PYCHECKPOINT'
import sys
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv[1]) / 'tests/bridge'))
from custody_credit import eth_hash, unhex
from deploy_local_custody import disposable_rpc
rpcs = [disposable_rpc(url, sys.argv[4], sys.argv[5]) for url in sys.argv[2:4]]
data = '0x' + eth_hash(b'latestCanonicalCheckpointHash()')[:4].hex()
observed = {unhex(rpc.call('eth_call', [dict(to=sys.argv[6], data=data), 'latest']), 32) for rpc in rpcs}
if len(observed) != 1:
    raise SystemExit('the Paxeer origins disagree on the latest canonical checkpoint')
print('0x' + observed.pop().hex())
PYCHECKPOINT
}

human_evidence_delivery_script() {
    cat <<'HUMAN_EVIDENCE_DELIVERY'
set -eu
umask 077
root=$1 transaction=$2 encoded=$3 digest=$4
[ -d "$root" ] && [ ! -L "$root" ] || { printf '%s: movement provider evidence root missing\n' "$root" >&2; exit 1; }
credit=$root/credit-$transaction.bin
deposit=$root/deposit-$transaction.bin
if [ ! -e "$credit" ] && [ ! -L "$credit" ]; then
    (set -C; printf '%s' "$encoded" | base64 -d > "$credit") || { rm -f "$credit"; printf '%s: custody credit could not be written\n' "$credit" >&2; exit 1; }
fi
printf '%s  %s\n' "$digest" "$credit" | sha256sum -c --status - || { printf '%s: custody credit bytes differ from the attested credit\n' "$credit" >&2; exit 1; }
[ "$(stat -c %s "$credit")" -eq 427 ] || { printf '%s: custody credit is not 427 bytes\n' "$credit" >&2; exit 1; }
for path in "$deposit" "$credit"; do
    [ -f "$path" ] && [ ! -L "$path" ] || { printf '%s: evidence file missing\n' "$path" >&2; exit 1; }
    [ "$(stat -c '%u %a %h' "$path")" = "$(id -u) 600 1" ] || { printf '%s: evidence file is not private to the movement provider\n' "$path" >&2; exit 1; }
done
HUMAN_EVIDENCE_DELIVERY
}

human_custody_evidence_publish() {
    local input="$WORK_DIR/human-evidence-input" ns="$TESTNET_NAMESPACE" log_file="$LOG_DIR/human-deposit-proof.log"
    local transaction account credit checkpoint attempted="" deadline
    transaction=$(jq -er '.transactionHash' "$input/custody-deposit.json" | tr '[:upper:]' '[:lower:]') \
        || fail "$input/custody-deposit.json: the owner custody deposit receipt is required before evidence delivery"
    [[ $transaction =~ ^0x[0-9a-f]{64}$ ]] || fail "custody-deposit.json carries no 32-byte transaction hash"
    account="agent:$(jq -er '.did' "$input/owner-admission.json"):main" \
        || fail "$input/owner-admission.json: the admitted owner DID is required before evidence delivery"
    credit="$input/credit-${transaction#0x}.bin"
    [ -f "$credit" ] && [ ! -L "$credit" ] && [ "$(stat -c %s "$credit")" -eq 427 ] \
        || fail "$credit: the attested custody credit for the owner deposit is missing; owner_custody.py deposit publishes it"
    [[ $CHECKPOINT_REGISTRY =~ ^0x[0-9a-fA-F]{40}$ ]] || fail "the CheckpointRegistry address is required before evidence delivery"
    log "publishing the owner custody deposit proof for $transaction through the in-cluster movement provider"
    deadline=$((SECONDS + 600))
    while :; do
        checkpoint=$(custody_latest_checkpoint) \
            || fail "the latest canonical checkpoint could not be read from CheckpointRegistry $CHECKPOINT_REGISTRY"
        if [ "$checkpoint" != "$attempted" ] && [[ ! $checkpoint =~ ^0x0{64}$ ]]; then
            attempted=$checkpoint
            printf 'attempting deposit proof publication against checkpoint %s\n' "$checkpoint" >> "$log_file"
            if kube -n "$ns" exec layerx-node-0 -c human-movement -- sh -ec '
                umask 077
                runtime=$(mktemp -d /run/human-private/movement-publish.XXXXXX)
                status=0
                /usr/local/bin/layerx-runtime-clock --runtime-dir "$runtime" -- \
                    /usr/local/bin/layerx-human-movement-provider --publish-deposit-proof "$@" || status=$?
                rm -r "$runtime"
                exit "$status"
            ' sh "$transaction" "$checkpoint" "$account" >> "$log_file" 2>&1; then
                break
            fi
        fi
        [ "$SECONDS" -lt "$deadline" ] \
            || fail "no canonical checkpoint covered the owner custody deposit $transaction within 600s; see $log_file"
        sleep 5
    done
    human_evidence_delivery_script | kube -n "$ns" exec -i layerx-node-0 -c human-movement -- \
        sh -s -- /var/lib/layerx/human/evidence "${transaction#0x}" "$(base64 -w0 "$credit")" "$(sha256sum "$credit" | cut -c1-64)" \
        || fail "the custody credit material for $transaction was not delivered to the movement provider evidence root"
    log "owner custody deposit proof and credit material delivered to the movement provider for $transaction"
}

ramp_inputs_require() {
    local variable path
    local -a missing=() supplied=()
    for variable in "${RAMP_VALUE_INPUTS[@]}" "${RAMP_FILE_INPUTS[@]}" "${RAMP_OPTIONAL_INPUTS[@]}"; do
        if [ -n "${!variable:-}" ]; then supplied+=("$variable"); fi
    done
    if [ "${#supplied[@]}" -eq 0 ]; then
        RAMP_ENABLED=0
        MISSING_INPUTS+=("${RAMP_VALUE_INPUTS[*]} ${RAMP_FILE_INPUTS[*]}: the reference fiat ramp; the repository does not invent provider, compliance or custody-owner coordinates, so the ramp image, workload, port-forward and sandbox journey are left out until the owner supplies all of them")
        log "the reference fiat ramp is unconfigured: no LAYERX_BETA_RAMP_* input is set, so its image, workload and port-forward are left out of this bring-up"
        return 0
    fi
    RAMP_ENABLED=1
    for variable in "${RAMP_VALUE_INPUTS[@]}" "${RAMP_FILE_INPUTS[@]}"; do
        [ -n "${!variable:-}" ] || missing+=("$variable")
    done
    if [ "${#missing[@]}" -ne 0 ]; then
        fail "the reference ramp needs owner coordinates it cannot derive from the cluster; set ${missing[*]}, or unset every LAYERX_BETA_RAMP_* variable to bring the cluster up without the ramp"
    fi
    for variable in "${RAMP_FILE_INPUTS[@]}"; do
        path=${!variable}
        [ -f "$path" ] && [ ! -L "$path" ] && [ -r "$path" ] \
            || fail "$variable=$path must name a readable regular file, not a symlink"
        [ -s "$path" ] || fail "$variable=$path is empty"
    done
}

ramp_config_render() {
    # ramp_config_render OUTPUT
    python3 - "$1" "$TESTNET_NAMESPACE" "$NODE_NETWORK_ID" "$NODE_ASSET_ID" "$PAXEER_CHAIN_ID" \
        "$SECRETS_DIR/sequencer-id" "$SECRETS_DIR/sequencer-public-key" \
        "$SECRETS_DIR/sequencer-first-batch" "$SECRETS_DIR/sequencer-last-batch" \
        "$RAMP_WORKER_ID" "$RAMP_FEE_LIMIT" <<'PYRAMPCONFIG'
import json, os, sys

output, namespace, network_id, asset_hex, chain_id = sys.argv[1:6]
sequencer_id, sequencer_key, sequencer_first, sequencer_last = [
    open(path, encoding='ascii').read().strip() for path in sys.argv[6:10]
]
worker_id, fee_limit = sys.argv[10], int(sys.argv[11])

def refuse(message):
    raise SystemExit('beta-cluster: error: %s' % message)

def value(name):
    present = os.environ.get(name, '')
    if not present:
        refuse('%s is required to render the reference ramp configuration' % name)
    return present

def hex32(name):
    present = value(name)
    if len(present) != 64 or any(digit not in '0123456789abcdef' for digit in present):
        refuse('%s must be 64 lowercase hexadecimal characters' % name)
    return present

def endpoint(name):
    present = value(name)
    if not present.startswith('https://') or present == 'https://':
        refuse('%s must be a canonical https:// endpoint' % name)
    return present

def internal(service, port):
    return 'https://%s.%s.svc.cluster.local:%d' % (service, namespace, port)

asset = list(bytes.fromhex(asset_hex))
with open(value('LAYERX_BETA_RAMP_QUOTES_FILE'), encoding='utf-8') as handle:
    quotes = json.load(handle)
if not isinstance(quotes, list) or not quotes:
    refuse('LAYERX_BETA_RAMP_QUOTES_FILE must hold a non-empty JSON array of operator quotes')
for quote in quotes:
    if not isinstance(quote, dict):
        refuse('every entry of LAYERX_BETA_RAMP_QUOTES_FILE must be a quote object')
    if quote.get('layerx_asset') != asset:
        refuse('quote %r must name the beta node asset %s' % (quote.get('quote_id'), asset_hex))

actor_did = value('LAYERX_BETA_RAMP_OPERATOR_DID')
account = 'agent:%s:main' % actor_did
config = {
    'listen': '0.0.0.0:8443',
    'journal_path': '/var/lib/layerx-ramp/journal.jsonl',
    'worker_id': worker_id,
    'lease_seconds': 60,
    'reconcile_seconds': 5,
    'operator': {
        'principal_id': value('LAYERX_BETA_RAMP_OPERATOR_PRINCIPAL_ID'),
        'account': account,
        'signer_key_handle': value('LAYERX_BETA_RAMP_OPERATOR_SIGNER_KEY_HANDLE'),
    },
    'quotes': quotes,
    'server_identity_pkcs12': '/run/secrets/server-identity.p12',
    'server_identity_password_file': '/run/secrets/server-identity-password',
    'client_tls': {
        'ca_pem': '/run/secrets/outbound-ca.pem',
        'identity_pkcs12': '/run/secrets/outbound-identity.p12',
        'identity_password_file': '/run/secrets/outbound-identity-password',
        'timeout_seconds': 8,
    },
    'identity': {
        'endpoint': internal('layerx-identity', 9443),
        'service_token_file': '/run/secrets/identity-token',
        'audience': 'layerx-ramp',
    },
    'compliance': {
        'endpoint': endpoint('LAYERX_BETA_RAMP_COMPLIANCE_ENDPOINT'),
        'service_token_file': '/run/secrets/compliance-token',
        'public_key': hex32('LAYERX_BETA_RAMP_COMPLIANCE_PUBLIC_KEY'),
    },
    'provider': {
        'endpoint': endpoint('LAYERX_BETA_RAMP_PROVIDER_ENDPOINT'),
        'credential_file': '/run/secrets/provider-token',
        'settlement_path': '/layerx-ramp-v1/settlements',
        'status_path': '/layerx-ramp-v1/settlements',
    },
    'layerx': {
        'gateway_endpoint': internal('layerx-gateway', 443),
        'receipt_authority_endpoint': internal('layerx-receipt-authority', 9443),
        'signer_endpoint': endpoint('LAYERX_BETA_RAMP_SIGNER_ENDPOINT'),
        'gateway_key_file': '/run/secrets/gateway-key',
        'authority_token_file': '/run/secrets/receipt-authority-token',
        'signer_token_file': '/run/secrets/kms-token',
        'actor_did': actor_did,
        'protocol_version': 2,
        'network_id': int(network_id),
        'fee_limit': fee_limit,
        'signer_public_key': hex32('LAYERX_BETA_RAMP_SIGNER_PUBLIC_KEY'),
        'sequencer_id': sequencer_id,
        'sequencer_public_key': sequencer_key,
        'sequencer_first_batch': sequencer_first,
        'sequencer_last_batch': sequencer_last,
    },
    'paxeer': {
        'custody_endpoint': internal('paxeer-boundary', 9443),
        'custody_credential_file': '/run/secrets/paxeer-custody-token',
        'broadcast_path': '/layerx-paxeer-v1/rebalances',
        'status_path': '/layerx-paxeer-v1/rebalances',
        'operator_account': account,
        'wallet_address': value('LAYERX_BETA_RAMP_PAXEER_WALLET_ADDRESS'),
        'vault_id': value('LAYERX_BETA_RAMP_PAXEER_VAULT_ID'),
        'signer_key_handle': value('LAYERX_BETA_RAMP_PAXEER_SIGNER_KEY_HANDLE'),
        'rpc_endpoints': [internal('paxeer-boundary', 9443), internal('paxeer-observer-boundary', 9443)],
        'rpc_trust_anchor_der': '/run/secrets/paxeer-rpc-ca.der',
        'rpc_chain_id': int(chain_id),
        'rpc_minimum_agreement': 2,
        'required_confirmations': 12,
        'poll_cadence_seconds': 5,
        'delayed_after_polls': 12,
    },
    'provider_callback_public_key': hex32('LAYERX_BETA_RAMP_PROVIDER_CALLBACK_PUBLIC_KEY'),
    'operator_control_token_file': '/run/secrets/operator-control-token',
}
descriptor = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(descriptor, 'w', encoding='utf-8') as handle:
    os.fchmod(handle.fileno(), 0o600)
    json.dump(config, handle, indent=2)
    handle.write('\n')
PYRAMPCONFIG
}

ramp_secrets_apply() {
    local c="$CA_DIR" s="$SECRETS_DIR" ns="$TESTNET_NAMESPACE" dir="$WORK_DIR/ramp"
    mkdir -p "$dir"
    chmod 0700 "$dir"
    (umask 077; cat "$c/ca.crt" "$LAYERX_BETA_RAMP_OUTBOUND_CA_PEM_FILE" > "$dir/outbound-ca.pem")
    ramp_config_render "$dir/config.json"
    apply_secret "$ns" layerx-reference-ramp-config --from-file=config.json="$dir/config.json"
    apply_secret "$ns" layerx-reference-ramp-server-tls \
        --from-file=server-identity.p12="$c/ramp/server.p12" --from-file=server-identity-password="$c/ramp/password"
    apply_secret "$ns" layerx-reference-ramp-client-tls \
        --from-file=outbound-ca.pem="$dir/outbound-ca.pem" \
        --from-file=outbound-identity.p12="$LAYERX_BETA_RAMP_OUTBOUND_IDENTITY_PKCS12_FILE" \
        --from-file=outbound-identity-password="$LAYERX_BETA_RAMP_OUTBOUND_IDENTITY_PASSWORD_FILE"
    apply_secret "$ns" layerx-reference-ramp-identity --from-file=identity-token="$s/identity-tokens/ramp"
    apply_secret "$ns" layerx-reference-ramp-compliance --from-file=compliance-token="$LAYERX_BETA_RAMP_COMPLIANCE_TOKEN_FILE"
    apply_secret "$ns" layerx-reference-ramp-provider --from-file=provider-token="$LAYERX_BETA_RAMP_PROVIDER_TOKEN_FILE"
    apply_secret "$ns" layerx-reference-ramp-layerx \
        --from-file=gateway-key="$LAYERX_BETA_RAMP_GATEWAY_KEY_FILE" \
        --from-file=receipt-authority-token="$s/ramp-authority.token" \
        --from-file=kms-token="$LAYERX_BETA_RAMP_SIGNER_TOKEN_FILE"
    apply_secret "$ns" layerx-reference-ramp-paxeer \
        --from-file=paxeer-custody-token="$LAYERX_BETA_RAMP_PAXEER_CUSTODY_TOKEN_FILE" \
        --from-file=paxeer-rpc-ca.der="$c/ca.der"
    apply_secret "$ns" layerx-reference-ramp-operator --from-file=operator-control-token="$s/ramp-operator-control.token"
    apply_tls_secret "$ns" layerx-reference-ramp-ingress-tls ramp
}

ramp_wait_ready() {
    local deadline=$((SECONDS + READY_TIMEOUT)) status
    while :; do
        status=$(curl --silent --show-error --max-time 10 --cacert "$CA_DIR/ca.crt" \
            --output "$WORK_DIR/ramp/readyz.json" --write-out '%{http_code}' "$RAMP_URL/readyz" 2>/dev/null) || status=unreachable
        if [ "$status" = 200 ] && jq -e '.ready == true and .external_custody == true' "$WORK_DIR/ramp/readyz.json" > /dev/null 2>&1; then
            log "reference ramp ready at $RAMP_URL: $(jq -c '{provider_contract, compliance_contract, paxeer_contract}' "$WORK_DIR/ramp/readyz.json")"
            return 0
        fi
        if [ "$SECONDS" -ge "$deadline" ]; then
            kube -n "$TESTNET_NAMESPACE" logs layerx-reference-ramp-0 -c ramp --tail 40 >&2 || true
            fail "the reference ramp did not report ready at $RAMP_URL/readyz within ${READY_TIMEOUT}s (last HTTP status $status)"
        fi
        sleep 5
    done
}

ramp_apply() {
    ramp_inputs_require
    [ "$RAMP_ENABLED" = 1 ] || return 0
    ramp_secrets_apply
    kube apply -f "$MANIFESTS_DIR/ramp.yaml" > /dev/null
    wait_for_pod_ready "$TESTNET_NAMESPACE" app=layerx-reference-ramp 600
    port_forward ramp "$TESTNET_NAMESPACE" layerx-reference-ramp "$RAMP_PORT" 443
    ramp_wait_ready
}

ramp_journey_input() {
    # ramp_journey_input VARIABLE OVERRIDE VALUE SURFACE
    local variable=$1 override=$2 value=$3 surface=$4
    value=${!override:-$value}
    if [ -z "$value" ]; then
        printf '# %s: %s\nunset %s\n' "$variable" "$surface" "$variable" >> "$ENV_FILE"
        MISSING_INPUTS+=("$override: $surface")
        return 0
    fi
    printf '# %s: %s\nexport %s=%s\n' "$variable" "$surface" "$variable" "$value" >> "$ENV_FILE"
}

ramp_env_write() {
    local on_quote off_quote
    [ "$RAMP_ENABLED" = 1 ] || return 0
    on_quote=$(jq -r 'map(select(.direction == "on_ramp")) | .[0].quote_id // empty' "$LAYERX_BETA_RAMP_QUOTES_FILE")
    off_quote=$(jq -r 'map(select(.direction == "off_ramp")) | .[0].quote_id // empty' "$LAYERX_BETA_RAMP_QUOTES_FILE")
    {
        printf 'export LAYERX_RAMP_URL=%s\n' "$RAMP_URL"
        printf 'export LAYERX_RAMP_OPERATOR_URL=%s\n' "$RAMP_URL"
        printf 'export LAYERX_RAMP_CA_PEM=%s\n' "$CA_DIR/ca.crt"
        printf 'export LAYERX_RAMP_OPERATOR_TOKEN=%s\n' "$(cat "$SECRETS_DIR/ramp-operator-control.token")"
    } >> "$ENV_FILE"
    ramp_journey_input LAYERX_RAMP_ON_QUOTE_ID LAYERX_BETA_RAMP_ON_QUOTE_ID "$on_quote" \
        "platform/ramps/sandbox-journey.sh: the on_ramp quote id of the operator quote catalog"
    ramp_journey_input LAYERX_RAMP_OFF_QUOTE_ID LAYERX_BETA_RAMP_OFF_QUOTE_ID "$off_quote" \
        "platform/ramps/sandbox-journey.sh: the off_ramp quote id of the operator quote catalog"
    ramp_journey_input LAYERX_RAMP_CUSTOMER_TOKEN LAYERX_BETA_RAMP_CUSTOMER_TOKEN "" \
        "platform/ramps/sandbox-journey.sh: an identity session token of the ramp customer principal"
    ramp_journey_input LAYERX_RAMP_OFF_GRANT_JSON LAYERX_BETA_RAMP_OFF_GRANT_JSON "" \
        "platform/ramps/sandbox-journey.sh: the 32-byte payer grant of the off-ramp order"
    ramp_journey_input LAYERX_RAMP_ON_ACCOUNT_SEQUENCE LAYERX_BETA_RAMP_ON_ACCOUNT_SEQUENCE "" \
        "platform/ramps/sandbox-journey.sh: the operator debit sequence for the on-ramp direct send"
    ramp_journey_input LAYERX_RAMP_OFF_RECEIVER_SEQUENCE LAYERX_BETA_RAMP_OFF_RECEIVER_SEQUENCE "" \
        "platform/ramps/sandbox-journey.sh: the operator receiver sequence for the off-ramp payer-grant draw"
}

beta_cluster_up() {
    local run_boundary_checks=$1
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" = 1 ]; then
        source "$REPO_ROOT/platform/hosted/human/material.sh"
        retained_material_inventory check || fail "retained material refused: inventory validation failed"
    fi
    require_tool docker curl openssl jq python3 git sha256sum tar base64
    require_foundry
    [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" = 1 ] || require_builder_environment
    custody_profile_validate
    interop_inputs_require
    ramp_inputs_require
    MISSING_INPUTS=()
    REVISION=$(revision)
    mkdir -p "$WORK_DIR" "$LOG_DIR"
    PULL_POLICY=IfNotPresent
    [ "$(cluster_mode)" = owner ] && PULL_POLICY=Always
    preflight_disk "$MIN_FREE_GIB" LAYERX_BETA_MIN_FREE_GIB "build the beta images and run a local cluster"
    tools_install
    prepare_images
    cluster_create
    load_images
    node_boundary_install
    material_prepare
    secrets_apply
    manifests_render
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" = 1 ]; then
        apply_configmap "$TESTNET_NAMESPACE" layerx-program-builder-release \
            --from-file=environment-tree-digest="$SECRETS_DIR/environment-tree-digest" \
            --from-file=bwrap-digest="$SECRETS_DIR/bwrap-digest" \
            --from-file=cgroup-exec-digest="$SECRETS_DIR/cgroup-exec-digest"
    else
        builder_release_publish
    fi
    kube apply -f "$MANIFESTS_DIR/paxeer.yaml" > /dev/null
    PAXEER_URL="https://localhost:19449"
    PAXEER_OBSERVER_URL="https://localhost:19452"
    wait_for_pod_ready "$TESTNET_NAMESPACE" app=paxeer 600
    port_forward paxeer-boundary "$TESTNET_NAMESPACE" paxeer-boundary 19449 9443
    port_forward paxeer-observer-boundary "$TESTNET_NAMESPACE" paxeer-observer-boundary 19452 9443
    paxeer_origins_write
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" != 1 ]; then
        [ -z "$CUSTODY_PROFILE" ] || fail 'fresh owner custody must be generated against this disposable cluster before genesis'
        human_custody_step bootstrap
        CUSTODY_PROFILE="$WORK_DIR/human-evidence-input/custody.profile"
        cp "$CUSTODY_PROFILE" "$SECRETS_DIR/custody.profile"
        apply_configmap "$TESTNET_NAMESPACE" layerx-node-custody-profile --from-file=profile="$CUSTODY_PROFILE"
        manifests_render
    fi
    mirror_publish
    trusted_boundary_apply
    TESTNET_URL="https://localhost:$TESTNET_PORT"
    GATEWAY_URL="https://localhost:$GATEWAY_PORT"
    FAUCET_URL="https://localhost:$FAUCET_PORT"
    DEVELOPER_URL="https://localhost:19450"
    NODE_URL="https://localhost:19446"
    AGENT_URL="https://localhost:19447"
    AGENTD_URL="https://localhost:19456"
    PAXEER_URL="https://localhost:19449"
    PAXEER_OBSERVER_URL="https://localhost:19452"
    IDENTITY_URL="https://localhost:$IDENTITY_PORT"
    INTEROP_URL="https://localhost:$INTEROP_PORT"
    HUMAN_URL="https://localhost:19453"
    HUMAN_WEB_URL="https://$HUMAN_WEB_HOST"
    RAMP_URL="https://localhost:$RAMP_PORT"
    EXPLORER_INDEX_URL="https://localhost:$EXPLORER_INDEX_PORT"
    wait_for_node_genesis
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" = 1 ]; then
        apply_configmap "$TESTNET_NAMESPACE" layerx-node-settlement --from-file=settlement.env="$WORK_DIR/paxeer/settlement.env"
        human_secrets_apply
    else
        paxeer_contracts_deploy
        settlement_publish
    fi
    wait_for_pod_ready "$TESTNET_NAMESPACE" app=layerx-identity 300
    port_forward identity "$TESTNET_NAMESPACE" layerx-identity "$IDENTITY_PORT" 9443
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" != 1 ]; then
        identity_provision
        kube apply -f "$MANIFESTS_DIR/registry.yaml" > /dev/null
        wait_for_pod_ready "$TESTNET_NAMESPACE" app=layerx-program-registry 600
        (umask 077; mkdir -p "$WORK_DIR/human-evidence-input")
        python3 "$REPO_ROOT/platform/hosted/human/provision.py" --prepare-owner-request \
            --work-dir "$WORK_DIR" --secrets-dir "$SECRETS_DIR"
        local guardians="$REPO_ROOT/platform/hosted/human/guardians.py"
        python3 "$guardians" enroll --work-dir "$WORK_DIR" --secrets-dir "$SECRETS_DIR" \
            --role guarantor-1 --identity "$NODE_GUARANTOR_ID"
        python3 "$guardians" enroll --work-dir "$WORK_DIR" --secrets-dir "$SECRETS_DIR" \
            --role guarantor-2 --identity "$NODE_SECOND_GUARANTOR_ID"
        python3 "$guardians" enroll --work-dir "$WORK_DIR" --secrets-dir "$SECRETS_DIR" \
            --role sequencer --identity "$NODE_SEQUENCER_ID"
        python3 "$guardians" assemble --work-dir "$WORK_DIR"
        local guardian
        for guardian in guarantor-1 guarantor-2 sequencer; do
            apply_secret "$TESTNET_NAMESPACE" "layerx-human-guardian-$guardian" \
                --from-file=seed="$SECRETS_DIR/human-guardians/$guardian-e1/seed"
        done
        apply_configmap "$TESTNET_NAMESPACE" layerx-human-guardian-bindings \
            --from-file=bindings.json="$WORK_DIR/human-evidence-input/recovery-guardian-bindings.json"
        human_evidence_provision
        human_policy_publish
    fi
    kube apply -f "$MANIFESTS_DIR/node.yaml" > /dev/null
    manifests_apply
    port_forward testnet "$TESTNET_NAMESPACE" layerx-testnet-public "$TESTNET_PORT" 443
    port_forward gateway "$TESTNET_NAMESPACE" layerx-gateway "$GATEWAY_PORT" 443
    port_forward faucet "$TESTNET_NAMESPACE" layerx-faucet-public "$FAUCET_PORT" 443
    wait_for_pod_ready "$TESTNET_NAMESPACE" app=layerx-gateway 600
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" != 1 ]; then
        internal_principals_provision
    else
        retained_principals_apply
    fi
    port_forward human "$TESTNET_NAMESPACE" layerx-human 19453 9443
    human_browser_provision
    internal_apply
    port_forward developer "$DEVELOPER_NAMESPACE" layerx-webhooks 19450 443
    port_forward pending-core "$TESTNET_NAMESPACE" layerx-pending-core 19446 9443
    port_forward agent-boundary "$TESTNET_NAMESPACE" layerx-agent-boundary 19447 9443
    port_forward agentd "$TESTNET_NAMESPACE" layerx-agentd 19456 9443
    wait_for_pod_ready "$TESTNET_NAMESPACE" app=layerx-node 600
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" != 1 ]; then human_custody_evidence_publish; fi
    agentd_check
    mirror_ready
    relay_archive_apply
    port_forward explorer-index "$TESTNET_NAMESPACE" layerx-explorer-index "$EXPLORER_INDEX_PORT" 9443
    module_registry_verify
    explorer_observation_publish
    human_web_apply
    if [ -n "$HUMAN_WEB_PORT" ]; then
        [ "$HUMAN_WEB_PORT" = 443 ] || fail "LAYERX_BETA_HUMAN_WEB_PORT must be 443 or empty because the browser origin $HUMAN_WEB_URL carries no port"
        port_forward human-web "$TESTNET_NAMESPACE" layerx-human-web "$HUMAN_WEB_PORT" 443
    fi
    interop_gateway_apply
    ramp_apply
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" != 1 ]; then material_save; fi
    env_write
    identity_write
    wait_ready || fail "beta cluster did not reach journey readiness; see the missing owner inputs above"
    log "every journey ready: $(jq -r '[.journeys[] | .journey] | join(",")' "$WORK_DIR/readyz.json")"
    log "environment exported to $ENV_FILE"
    log "human web application: add '127.0.0.1 $HUMAN_WEB_HOST' to /etc/hosts, trust the beta internal CA at $CA_DIR/ca.crt, then open $HUMAN_WEB_URL"
    log "explorer program index: $EXPLORER_INDEX_URL (bearer $SECRETS_DIR/explorer-program.token)"
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
    if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" = 1 ]; then
        source "$REPO_ROOT/platform/hosted/human/material.sh"
        retained_material_inventory check || fail "retained material refused: inventory validation failed"
    fi
    require_tool docker openssl jq python3 git
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
    material_prepare
    manifests_render
    log "rendered manifests under $MANIFESTS_DIR and beta CA under $CA_DIR (nothing applied)"
}

publish_images() {
    local mode=${1:-} flags=()
    shift || true
    case "$mode" in
        check) flags=(--check) ;;
        dry-run) flags=(--dry-run) ;;
        push) flags=(--phase push) ;;
        verify) flags=(--phase verify) ;;
        promote) flags=(--phase promote) ;;
        self-test) flags=(--self-test) ;;
        "") fail "publish-images requires the caller to name its mode: check, dry-run, push, promote or self-test" ;;
        *) fail "unknown publish-images mode '$mode': expected check, dry-run, push, promote or self-test" ;;
    esac
    bash "$SCRIPT_DIR/publish-images.sh" "${flags[@]}" "$@"
}

main() {
    local command=${1:-} boundary=0
    shift || true
    case "$command" in
        images)
            require_tool docker git tar
            REVISION=$(revision)
            preflight_disk "$IMAGE_MIN_FREE_GIB" LAYERX_BETA_IMAGE_MIN_FREE_GIB "build the beta images"
            prepare_images
            ;;
        publish-images)
            publish_images "$@"
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
        test-retained-material) retained_material_test ;;
        test-guarantor-sequences) guarantor_sequence_test ;;
        test-deposit-root-authority) deposit_root_authority_test ;;
        test-genesis-metadata) genesis_metadata_test ;;
        down) beta_cluster_down ;;
        render) beta_cluster_render ;;
        *) awk 'NR > 1 { if ($0 !~ /^#/) exit; sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}" >&2; exit 64 ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
