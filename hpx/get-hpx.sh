#!/usr/bin/env bash
# =============================================================================
# HyperPax Node Manager — web installer
#
#   curl -sSL https://node.hyperpaxeer.com/get-hpx.sh | sudo bash
#
# Installs the lightweight `hpx` CLI, then launches the interactive setup
# wizard. The node itself runs as a native paxd binary under systemd — no
# Docker. The binary, libwasmvm, genesis and config variants are all pulled
# from the mirror by `hpx setup`.
# =============================================================================
set -euo pipefail

MIRROR="${HPX_MIRROR:-https://node.hyperpaxeer.com}"
INSTALL_DIR="/usr/local/bin"

RST='\033[0m'; BOLD='\033[1m'; DIM='\033[2m'
RED='\033[0;31m'; GRN='\033[0;32m'; YLW='\033[0;33m'; CYN='\033[0;36m'; WHT='\033[1;37m'
ok()   { printf "${GRN}[+]${RST} %s\n" "$*"; }
info() { printf "${CYN}[*]${RST} %s\n" "$*"; }
warn() { printf "${YLW}[!]${RST} %s\n" "$*"; }
die()  { printf "${RED}[-]${RST} %s\n" "$*" >&2; exit 1; }
hr()   { printf "${DIM}%70s${RST}\n" | tr ' ' '-'; }

printf "\n${BOLD}${CYN}"
prn() { printf '%s\n' "$1"; }
prn '  ██╗  ██╗██████╗ ██╗  ██╗'
prn '  ██║  ██║██╔══██╗╚██╗██╔╝   Paxeer Network Node Manager'
prn '  ███████║██████╔╝ ╚███╔╝    native paxd + systemd'
prn '  ██╔══██║██╔═══╝  ██╔██╗'
prn '  ██║  ██║██║     ██╔╝ ██╗'
prn '  ╚═╝  ╚═╝╚═╝     ╚═╝  ╚═╝'
printf "${RST}\n"; hr

# ── Preflight ─────────────────────────────────────────────────────────────────
[ "$(id -u)" -eq 0 ] || die "run as root:  curl -sSL ${MIRROR}/get-hpx.sh | sudo bash"
[ "$(uname -s)" = "Linux" ] || die "Linux only (got $(uname -s))"
case "$(uname -m)" in
    x86_64|amd64|aarch64|arm64) ok "arch $(uname -m)" ;;
    *) die "unsupported arch $(uname -m)" ;;
esac

# ── Dependencies (curl + jq only; node runs native, no docker) ────────────────
info "checking dependencies"
missing=()
for dep in curl jq; do command -v "$dep" >/dev/null 2>&1 && ok "$dep" || { warn "$dep missing"; missing+=("$dep"); }; done
if [ ${#missing[@]} -gt 0 ]; then
    info "installing: ${missing[*]}"
    if command -v apt-get >/dev/null 2>&1; then
        DEBIAN_FRONTEND=noninteractive apt-get update -qq
        DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "${missing[@]}"
    elif command -v yum >/dev/null 2>&1; then
        yum install -y -q "${missing[@]}"
    else
        die "install ${missing[*]} manually and re-run"
    fi
fi

# ── Download the hpx CLI ──────────────────────────────────────────────────────
info "downloading hpx CLI from ${MIRROR}"
manifest=$(mktemp)
trap 'rm -f "$manifest" "${INSTALL_DIR}/hpx.new"' EXIT
curl -fsSL --retry 5 --max-time 60 "${MIRROR}/checksums.txt" -o "$manifest" \
    || die "could not download ${MIRROR}/checksums.txt"
curl -fsSL --retry 5 --max-time 60 "${MIRROR}/hpx" -o "${INSTALL_DIR}/hpx.new" \
    || die "could not download ${MIRROR}/hpx"
want=$(awk '$2 == "hpx" { print $1; exit }' "$manifest")
[ -n "$want" ] || die "checksum manifest has no hpx entry"
got=$(sha256sum "${INSTALL_DIR}/hpx.new" | awk '{print $1}')
[ "$got" = "$want" ] || die "hpx sha256 mismatch (got $got want $want)"
ok "hpx sha256 verified"
chmod +x "${INSTALL_DIR}/hpx.new"
mv -f "${INSTALL_DIR}/hpx.new" "${INSTALL_DIR}/hpx"
trap - EXIT
rm -f "$manifest"
ok "installed ${INSTALL_DIR}/hpx"

# ── Bash completion ───────────────────────────────────────────────────────────
if [ -d /etc/bash_completion.d ]; then
    cat > /etc/bash_completion.d/hpx <<'COMP'
_hpx() {
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local cmds="setup status info logs start stop restart update peers register statesync validator stake keygen remove version help"
    if [ "$COMP_CWORD" -eq 1 ]; then
        COMPREPLY=($(compgen -W "$cmds" -- "$cur"))
    elif [ "$COMP_CWORD" -eq 2 ] && [ "${COMP_WORDS[1]}" = "peers" ]; then
        COMPREPLY=($(compgen -W "show refresh" -- "$cur"))
    elif [ "$COMP_CWORD" -eq 2 ] && [ "${COMP_WORDS[1]}" = "validator" ]; then
        COMPREPLY=($(compgen -W "keygen stake status" -- "$cur"))
    fi
}
complete -F _hpx hpx
COMP
    ok "bash completion installed"
fi

echo ""; hr
printf "${BOLD}${GRN}hpx installed.${RST}  (export HPX_MIRROR to override the mirror)\n"
echo "    hpx setup     install + start a node (fullnode or validator)"
echo "    hpx           interactive menu"
echo ""

# ── Launch setup wizard ───────────────────────────────────────────────────────
printf "${YLW}Run the setup wizard now? [Y/n]${RST}: "
read -r yn < /dev/tty 2>/dev/null || yn="y"
case "$yn" in
    [Nn]*) printf "${DIM}skipped — run 'hpx setup' when ready.${RST}\n" ;;
    *)     exec hpx setup ;;
esac
