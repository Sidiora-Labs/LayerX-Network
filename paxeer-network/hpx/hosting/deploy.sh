#!/usr/bin/env bash
# Install the revision-bound HPX registry runtime and HTTPS Nginx vhost.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HPX_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
PAXEER_ROOT="$(cd "${HPX_DIR}/.." && pwd)"
MONOREPO_ROOT="$(cd "${PAXEER_ROOT}/.." && pwd)"

DOMAIN="node.hyperpaxeer.com"
SOURCE_REVISION="${HPX_SOURCE_REVISION:-$(git -C "$MONOREPO_ROOT" log -1 --format=%H -- paxeer-network/hpx/registry .github/workflows/paxeer-hpx-registry.yml)}"
ARTIFACTS_ROOT="${HPX_ARTIFACTS_ROOT:-/srv/hpx/artifacts}"
DATA_DIR="${HPX_DATA_DIR:-/srv/hpx/data}"
WEB_ROOT="${HPX_WEB_ROOT:-/var/www/hpx}"
REGISTRY_BIN="${HPX_REGISTRY_BIN:-/usr/local/libexec/hpx-registry}"
ENV_FILE="${HPX_REGISTRY_ENV:-/etc/hpx-registry.env}"
UNIT_FILE="/etc/systemd/system/hpx-registry.service"
NGINX_SITE="/etc/nginx/sites-available/hpx-registry"
RELEASE_BASE="https://github.com/Sidiora-Labs/LayerX-Protocol/releases/download/hpx-registry-${SOURCE_REVISION}"

CHAIN_ID="${HPX_CHAIN_ID:-hyperpax_125-1}"
SEED_PEERS="${HPX_SEED_PEERS:-e9c56cbadc4a96b67f69dcaaa7b4691851e945ca@31.220.74.140:26656}"
STATESYNC_RPC="${HPX_STATESYNC_RPC:-http://31.220.74.140:26657}"
STATESYNC_SERVERS="${HPX_STATESYNC_RPC_SERVERS:-31.220.74.140:26657,31.220.74.140:26657}"
FONT_ORIGIN="https://cdn.usercontent.paxeercode.com/fonts"
FONT_FILES=(
  LTWave-Light.otf
  LTWave-Regular.otf
  LTWave-Medium.otf
  LTWave-Bold.otf
  LTWaveMono-Regular.otf
  LTWaveMono-Medium.otf
)

say() { printf '\033[0;36m[deploy]\033[0m %s\n' "$*"; }
die() { printf '\033[0;31m[deploy] ERROR:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run as root"
[[ "$SOURCE_REVISION" =~ ^[0-9a-f]{40}$ ]] || die "invalid source revision: $SOURCE_REVISION"
[ -f "$SCRIPT_DIR/index.html" ] || die "missing landing page: $SCRIPT_DIR/index.html"
[ -f "$SCRIPT_DIR/fonts.sha256" ] || die "missing font manifest: $SCRIPT_DIR/fonts.sha256"
[ -f "$PAXEER_ROOT/assets/PaxeerLogo.png" ] || die "missing Paxeer logo"
if [ -n "${HPX_REGISTER_TOKEN:-}" ] && [[ ! "$HPX_REGISTER_TOKEN" =~ ^[A-Za-z0-9._~-]+$ ]]; then
  die "HPX_REGISTER_TOKEN may contain only letters, digits, dot, underscore, tilde and hyphen"
fi
[ -L "$ARTIFACTS_ROOT/current" ] || die "publish an HPX release before deployment: $ARTIFACTS_ROOT/current"
for command in curl sha256sum systemctl nginx certbot; do
  command -v "$command" >/dev/null 2>&1 || die "missing dependency: $command"
done

case "$(uname -m)" in
  x86_64|amd64) registry_arch=amd64 ;;
  aarch64|arm64) registry_arch=arm64 ;;
  *) die "unsupported registry architecture: $(uname -m)" ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
asset="hpx-registry-linux-${registry_arch}"
say "downloading registry runtime for ${SOURCE_REVISION}"
curl -fL --retry 5 --max-time 180 -o "$tmp/$asset" "$RELEASE_BASE/$asset"
curl -fL --retry 5 --max-time 60 -o "$tmp/$asset.sha256" "$RELEASE_BASE/$asset.sha256"
(
  cd "$tmp"
  sha256sum -c "$asset.sha256"
)

say "downloading checksum-bound LTWave brand fonts"
mkdir -p "$tmp/fonts"
for font in "${FONT_FILES[@]}"; do
  curl -fL --retry 5 --max-time 60 -o "$tmp/fonts/$font" "$FONT_ORIGIN/$font"
done
(
  cd "$tmp/fonts"
  sha256sum -c "$SCRIPT_DIR/fonts.sha256"
)

if ! id hpx-registry >/dev/null 2>&1; then
  useradd --system --home-dir /nonexistent --shell /usr/sbin/nologin hpx-registry
fi
install -d -m 0755 /usr/local/libexec "$ARTIFACTS_ROOT" /var/www/certbot
install -d -m 0755 "$WEB_ROOT" "$WEB_ROOT/fonts"
install -m 0644 "$SCRIPT_DIR/index.html" "$WEB_ROOT/index.html"
install -m 0644 "$PAXEER_ROOT/assets/PaxeerLogo.png" "$WEB_ROOT/paxeer-logo.png"
for font in "${FONT_FILES[@]}"; do
  install -m 0644 "$tmp/fonts/$font" "$WEB_ROOT/fonts/$font"
done
install -d -o hpx-registry -g hpx-registry -m 0750 "$DATA_DIR"
install -m 0755 "$tmp/$asset" "${REGISTRY_BIN}.new"
mv -f "${REGISTRY_BIN}.new" "$REGISTRY_BIN"

umask 077
cat > "$ENV_FILE" <<ENV
HPX_ADDR=127.0.0.1:8099
HPX_ARTIFACTS_DIR=${ARTIFACTS_ROOT}/current
HPX_DATA_DIR=${DATA_DIR}
HPX_CHAIN_ID=${CHAIN_ID}
HPX_SEED_PEERS=${SEED_PEERS}
HPX_STATESYNC_RPC=${STATESYNC_RPC}
HPX_STATESYNC_RPC_SERVERS=${STATESYNC_SERVERS}
HPX_REGISTER_TOKEN=${HPX_REGISTER_TOKEN:-}
ENV
chmod 0600 "$ENV_FILE"

cat > "$UNIT_FILE" <<UNIT
[Unit]
Description=HyperPax HPX distribution and peer registry
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=hpx-registry
Group=hpx-registry
EnvironmentFile=${ENV_FILE}
ExecStart=${REGISTRY_BIN}
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadOnlyPaths=${ARTIFACTS_ROOT}
ReadWritePaths=${DATA_DIR}
RestrictSUIDSGID=true
LockPersonality=true
MemoryDenyWriteExecute=true

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable hpx-registry.service
systemctl restart hpx-registry.service

say "bootstrapping HTTP vhost for certificate issuance"
install -m 0644 "$SCRIPT_DIR/nginx-http.conf" "$NGINX_SITE"
ln -sfn "$NGINX_SITE" /etc/nginx/sites-enabled/hpx-registry
nginx -t
systemctl reload nginx

certbot_args=(certonly --webroot -w /var/www/certbot -d "$DOMAIN" --non-interactive --agree-tos --keep-until-expiring)
if [ -n "${LETSENCRYPT_EMAIL:-}" ]; then
  certbot_args+=(--email "$LETSENCRYPT_EMAIL")
else
  certbot_args+=(--register-unsafely-without-email)
fi
certbot "${certbot_args[@]}"

say "enabling HTTPS registry vhost"
install -m 0644 "$SCRIPT_DIR/nginx.conf" "$NGINX_SITE"
nginx -t
systemctl reload nginx

say "HPX registry deployed at https://${DOMAIN}"
say "registry source revision ${SOURCE_REVISION}"
