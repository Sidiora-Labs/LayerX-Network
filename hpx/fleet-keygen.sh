#!/usr/bin/env bash
# =============================================================================
# fleet-keygen.sh — one-shot: push updated hpx to every server, generate the
#                   per-node operator account, and collect all pax1 addresses.
#
# Why push first: the fleet still runs the OLD hpx (no `validator keygen`), so
# we scp the local updated ./hpx to /usr/local/bin/hpx on each host, THEN run
# `hpx validator keygen` there. The operator key lands where `hpx validator
# stake` expects it (/root/.paxeer/keyring, key name "operator").
#
# Output: .fleet/operators.tsv  (moniker \t ip \t pax1 \t valoper)
#         -> consumed by stake-fleet.sh `fund` and `stake-cmds`.
#
# Usage:
#   ./fleet-keygen.sh                      # servers from registry /api/nodes
#   SERVERS='203.0.113.10 203.0.113.11' ./fleet-keygen.sh
#   SERVERS_FILE=ips.txt ./fleet-keygen.sh
#   HPX_PUSH=0 ./fleet-keygen.sh           # skip push (servers already updated)
# =============================================================================
set -uo pipefail

readonly SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly HPX_LOCAL="${HPX_LOCAL:-${SELF_DIR}/hpx}"
readonly WORKDIR="${HPX_WORKDIR:-${SELF_DIR}/.fleet}"
readonly OPERATORS_FILE="${WORKDIR}/operators.tsv"
readonly REMOTE_HPX="/usr/local/bin/hpx"
readonly MIRROR="${HPX_MIRROR:-https://node.hyperpaxeer.com}"
readonly SSH_USER="${HPX_SSH_USER:-root}"
readonly PUSH="${HPX_PUSH:-1}"
readonly SSH="ssh -o StrictHostKeyChecking=accept-new -o ConnectTimeout=12 ${HPX_SSH_OPTS:-}"
readonly SCP="scp -o StrictHostKeyChecking=accept-new -o ConnectTimeout=12 ${HPX_SCP_OPTS:-}"

C_RST=$'\033[0m'; C_B=$'\033[1m'; C_D=$'\033[2m'
C_R=$'\033[0;31m'; C_G=$'\033[0;32m'; C_Y=$'\033[0;33m'; C_C=$'\033[0;36m'
info(){ printf "%b\n" "${C_C}[*]${C_RST} $*"; }
ok(){   printf "%b\n" "${C_G}[+]${C_RST} $*"; }
warn(){ printf "%b\n" "${C_Y}[!]${C_RST} $*"; }
err(){  printf "%b\n" "${C_R}[-]${C_RST} $*" >&2; }
die(){  err "$@"; exit 1; }
hr(){   printf "%b\n" "${C_D}----------------------------------------------------------------------${C_RST}"; }

command -v jq   >/dev/null 2>&1 || die "missing jq"
command -v curl >/dev/null 2>&1 || die "missing curl"
command -v ssh  >/dev/null 2>&1 || die "missing ssh"
[ -f "$HPX_LOCAL" ] || die "local hpx not found at $HPX_LOCAL (set HPX_LOCAL=)"
bash -n "$HPX_LOCAL" || die "local hpx has a syntax error — fix before pushing to the fleet"
mkdir -p "$WORKDIR"; chmod 700 "$WORKDIR"

discover_servers(){
    if [ -n "${SERVERS:-}" ]; then printf '%s\n' $SERVERS; return; fi
    if [ -n "${SERVERS_FILE:-}" ] && [ -f "$SERVERS_FILE" ]; then
        grep -oE '([0-9]{1,3}\.){3}[0-9]{1,3}' "$SERVERS_FILE"; return; fi
    curl -fsS --max-time 12 "${MIRROR}/api/nodes" 2>/dev/null | jq -r '.nodes[].ip' 2>/dev/null
}

mapfile -t SERVERS_ARR < <(discover_servers | sort -u)
[ "${#SERVERS_ARR[@]}" -gt 0 ] || die "no servers (set SERVERS='ip ip ...' | SERVERS_FILE=<file> | ensure registry reachable)"

hr; info "${C_B}Fleet keygen${C_RST}  targets=${#SERVERS_ARR[@]}  push=${PUSH}"
info "${SERVERS_ARR[*]}"; hr

: > "${OPERATORS_FILE}.tmp"
declare -i okc=0 failc=0
for ip in "${SERVERS_ARR[@]}"; do
    printf "  %-16s " "$ip"
    # 1) push updated hpx (unless disabled)
    if [ "$PUSH" = 1 ]; then
        if ! $SCP "$HPX_LOCAL" "${SSH_USER}@${ip}:${REMOTE_HPX}.new" >/dev/null 2>&1; then
            printf "%b\n" "${C_R}scp failed (unreachable?)${C_RST}"; failc+=1; continue
        fi
        $SSH "${SSH_USER}@${ip}" "chmod +x ${REMOTE_HPX}.new && mv -f ${REMOTE_HPX}.new ${REMOTE_HPX}" >/dev/null 2>&1 \
            || { printf "%b\n" "${C_R}install failed${C_RST}"; failc+=1; continue; }
    fi
    # 2) run keygen, capture the machine-readable OPERATOR line
    line=$($SSH "${SSH_USER}@${ip}" 'hpx validator keygen 2>/dev/null | grep "^OPERATOR "' 2>/dev/null)
    if [ -z "$line" ]; then printf "%b\n" "${C_R}no OPERATOR line (keygen failed)${C_RST}"; failc+=1; continue; fi
    mon=$(awk '{print $2}' <<<"$line"); acc=$(awk '{print $3}' <<<"$line"); val=$(awk '{print $4}' <<<"$line")
    [ -n "$acc" ] || { printf "%b\n" "${C_R}parse failed${C_RST}"; failc+=1; continue; }
    printf '%s\t%s\t%s\t%s\n' "$mon" "$ip" "$acc" "$val" >> "${OPERATORS_FILE}.tmp"
    printf "%b\n" "${C_G}${acc}${C_RST}"; okc+=1
done

sort -u "${OPERATORS_FILE}.tmp" > "$OPERATORS_FILE"; rm -f "${OPERATORS_FILE}.tmp"
hr
ok "keygen OK on ${okc} host(s); ${failc} failed"
ok "operators -> ${OPERATORS_FILE}  (n=$(wc -l < "$OPERATORS_FILE"))"
[ "$failc" -eq 0 ] || warn "re-run to retry failed hosts (idempotent), or fix connectivity"
echo
echo "Next:  ./stake-fleet.sh fund --yes   then   ./stake-fleet.sh stake-cmds"
