#!/usr/bin/env bash
# =============================================================================
# HyperPax Node Manager — uninstaller
#
#   curl -sSL https://get.cloud.hyperpaxeer.com/uninstall.sh | sudo bash
#
# Stops + removes the paxd node service and the hpx CLI. Chain data/keys under
# /root/.paxeer are only deleted if you explicitly confirm.
# =============================================================================
set -euo pipefail

HOME_DIR="${HPX_HOME:-/root/.paxeer}"
RST='\033[0m'; RED='\033[0;31m'; GRN='\033[0;32m'; YLW='\033[0;33m'; BOLD='\033[1m'
ok()   { printf "${GRN}[+]${RST} %s\n" "$*"; }
warn() { printf "${YLW}[!]${RST} %s\n" "$*"; }
die()  { printf "${RED}[-]${RST} %s\n" "$*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run as root: sudo bash uninstall.sh"
printf "\n${BOLD}HyperPax Node Manager — uninstaller${RST}\n\n"

# Stop + remove the systemd services/units.
for unit in paxd.service hpx-peers-refresh.timer hpx-peers-refresh.service; do
    systemctl disable --now "$unit" 2>/dev/null || true
    rm -f "/etc/systemd/system/${unit}" "/etc/systemd/system/multi-user.target.wants/${unit}" \
          "/etc/systemd/system/timers.target.wants/${unit}"
done
systemctl daemon-reload 2>/dev/null || true
systemctl reset-failed 2>/dev/null || true
ok "stopped + removed paxd service + peer-refresh timer"

# Remove CLI + completion + binary.
rm -f /usr/local/bin/hpx;        ok "removed /usr/local/bin/hpx"
rm -f /etc/bash_completion.d/hpx; ok "removed bash completion"
rm -f /usr/local/bin/paxd;       ok "removed /usr/local/bin/paxd"

# Optional: wipe chain data + keys.
if [ -d "$HOME_DIR" ]; then
    printf "${YLW}Delete chain data + keys at ${HOME_DIR}? (irreversible) [y/N]${RST}: "
    read -r yn < /dev/tty 2>/dev/null || yn="n"
    if [[ "$yn" =~ ^[Yy] ]]; then rm -rf "$HOME_DIR"; ok "wiped ${HOME_DIR}"; else warn "kept ${HOME_DIR}"; fi
fi

printf "\n${GRN}uninstall complete.${RST}\n\n"
