#!/usr/bin/env bash
# =============================================================================
# hpx installer — copies the CLI to /usr/local/bin and sets up the data dir
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_PATH="/usr/local/bin/hpx"
HPX_HOME="${HPX_HOME:-/root/.paxeer}"

C_RST='\033[0m'; C_GRN='\033[0;32m'; C_RED='\033[0;31m'; C_CYN='\033[0;36m'; C_BOLD='\033[1m'

ok()  { printf "${C_GRN}[+]${C_RST} %s\n" "$*"; }
err() { printf "${C_RED}[-]${C_RST} %s\n" "$*" >&2; }
die() { err "$@"; exit 1; }

echo ""
printf "${C_BOLD}  hpx installer${C_RST}\n"
printf "${C_CYN}  HyperPax Node Manager CLI${C_RST}\n"
echo ""

# Check root
[ "$(id -u)" -eq 0 ] || die "Run as root: sudo bash install.sh"

# Check deps
missing=()
for dep in curl jq; do
    command -v "$dep" >/dev/null 2>&1 || missing+=("$dep")
done

if [ ${#missing[@]} -gt 0 ]; then
    err "Missing dependencies: ${missing[*]}"
    echo "  Install them first:"
    echo "    apt-get update && apt-get install -y ${missing[*]}"
    exit 1
fi

# Install
cp "$SCRIPT_DIR/hpx" "$INSTALL_PATH"
chmod +x "$INSTALL_PATH"
ok "Installed to ${INSTALL_PATH}"


# Shell completion hint
if [ -d /etc/bash_completion.d ]; then
    cat > /etc/bash_completion.d/hpx << 'COMP'
_hpx() {
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local cmds="setup status info logs start stop restart update peers register remove version help"
    if [ "$COMP_CWORD" -eq 1 ]; then
        COMPREPLY=($(compgen -W "$cmds" -- "$cur"))
    elif [ "$COMP_CWORD" -eq 2 ] && [ "${COMP_WORDS[1]}" = "peers" ]; then
        COMPREPLY=($(compgen -W "show refresh" -- "$cur"))
    fi
}
complete -F _hpx hpx
COMP
    ok "Bash completion installed"
fi

echo ""
ok "Installation complete!"
echo ""
echo "  Run interactively:   hpx"
echo "  Install a node:       hpx setup"
echo "  Full help:           hpx help"
echo ""
