#!/usr/bin/env bash
# =============================================================================
# Deploy hpx files to the web server
# Run this on the server hosting get.hyperpaxeer.com
#
# Usage:
#   sudo bash hosting/deploy.sh
#
# After first run, set up SSL:
#   sudo apt install certbot python3-certbot-nginx
#   sudo certbot --nginx -d node.hyperpaxeer.com
#   Then uncomment the HTTPS block in the nginx config.
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
WEB_ROOT="/var/www/hpx"
NGINX_CONF="/etc/nginx/sites-available/hpx"

GRN='\033[0;32m'; CYN='\033[0;36m'; RST='\033[0m'
ok()   { printf "${GRN}[+]${RST} %s\n" "$*"; }
info() { printf "${CYN}[*]${RST} %s\n" "$*"; }

[ "$(id -u)" -eq 0 ] || { echo "Run as root"; exit 1; }

# Install nginx if missing
if ! command -v nginx >/dev/null 2>&1; then
    info "Installing nginx..."
    apt-get update -qq && apt-get install -y nginx >/dev/null 2>&1
    ok "nginx installed"
fi

# Create web root and copy files
info "Deploying files to ${WEB_ROOT}..."
mkdir -p "$WEB_ROOT"
cp "$PROJECT_DIR/get-hpx.sh"  "$WEB_ROOT/get-hpx.sh"
cp "$PROJECT_DIR/hpx"         "$WEB_ROOT/hpx"
cp "$PROJECT_DIR/uninstall.sh" "$WEB_ROOT/uninstall.sh"

# Write version file
VERSION=$(grep '^readonly VERSION=' "$PROJECT_DIR/hpx" | sed 's/.*"\(.*\)"/\1/')
echo "$VERSION" > "$WEB_ROOT/version.txt"

chmod 644 "$WEB_ROOT"/*
ok "Files deployed"

# Install nginx config
info "Installing nginx config..."
cp "$SCRIPT_DIR/nginx.conf" "$NGINX_CONF"

# Enable site
if [ ! -L /etc/nginx/sites-enabled/hpx ]; then
    ln -sf "$NGINX_CONF" /etc/nginx/sites-enabled/hpx
fi

# Test and reload
nginx -t 2>&1
systemctl reload nginx
ok "nginx reloaded"

echo ""
ok "Deployment complete!"
echo ""
echo "  Endpoints:"
echo "    http://node.hyperpaxeer.com/install    Installer script"
echo "    http://node.hyperpaxeer.com/hpx        CLI binary"
echo "    http://node.hyperpaxeer.com/uninstall  Uninstaller"
echo "    http://node.hyperpaxeer.com/version    Version check"
echo ""
echo "  Users install with:"
echo "    curl -sSL https://node.hyperpaxeer.com/install | sudo bash"
echo ""
echo "  Next: set up HTTPS with certbot:"
echo "    sudo apt install certbot python3-certbot-nginx"
echo "    sudo certbot --nginx -d get.hyperpaxeer.com"
echo ""
