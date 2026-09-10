#!/bin/sh
set -eu
umask 077

usage() {
    cat <<'USAGE'
usage: probe.sh --url URL --ca FILE --client-cert FILE --client-key FILE --bearer-file FILE

Probes the hosted agentd Service. The Service publishes the agentd loopback
health surface through a mutually authenticated TLS boundary, so a ready
deployment answers only when the caller presents both a client certificate the
internal CA issued and the agent program bearer. The probe asserts all three
properties: the authenticated request reports ready, the same request without
the bearer is refused, and a request without a client certificate never reaches
HTTP at all.
USAGE
}

url=
ca=
client_cert=
client_key=
bearer_file=
while [ $# -gt 0 ]; do
    case "$1" in
        --url) url=${2:?probe.sh: --url needs a value}; shift 2 ;;
        --ca) ca=${2:?probe.sh: --ca needs a value}; shift 2 ;;
        --client-cert) client_cert=${2:?probe.sh: --client-cert needs a value}; shift 2 ;;
        --client-key) client_key=${2:?probe.sh: --client-key needs a value}; shift 2 ;;
        --bearer-file) bearer_file=${2:?probe.sh: --bearer-file needs a value}; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) printf 'probe.sh: unknown argument %s\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
done

for required in url ca client_cert client_key bearer_file; do
    eval "value=\$$required"
    if [ -z "$value" ]; then
        printf 'probe.sh: --%s is required\n' "$(printf '%s' "$required" | tr '_' '-')" >&2
        exit 2
    fi
done
for file in "$ca" "$client_cert" "$client_key" "$bearer_file"; do
    [ -r "$file" ] || { printf 'probe.sh: %s is not readable\n' "$file" >&2; exit 2; }
done
[ -s "$bearer_file" ] || { printf 'probe.sh: the agent program bearer is empty\n' >&2; exit 2; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM
printf 'header = "Authorization: Bearer %s"\n' "$(cat "$bearer_file")" > "$work/bearer.conf"

status=0
curl --silent --show-error --max-time 10 --output "$work/ready.json" --write-out '%{http_code}' \
    --cacert "$ca" --cert "$client_cert" --key "$client_key" --config "$work/bearer.conf" \
    "$url/healthz" > "$work/ready.code" 2> "$work/ready.err" || status=$?
if [ "$status" != 0 ]; then
    printf 'agentd-probe: authenticated health request failed (curl %s)\n' "$status" >&2
    sed -n '1,10p' "$work/ready.err" >&2
    exit 1
fi
if [ "$(cat "$work/ready.code")" != 200 ]; then
    printf 'agentd-probe: health returned HTTP %s\n' "$(cat "$work/ready.code")" >&2
    sed -n '1,10p' "$work/ready.json" >&2
    exit 1
fi
if ! grep -q '"ready":true' "$work/ready.json"; then
    printf 'agentd-probe: health did not report a ready owner\n' >&2
    sed -n '1,10p' "$work/ready.json" >&2
    exit 1
fi

status=0
curl --silent --show-error --max-time 10 --output "$work/anonymous.json" --write-out '%{http_code}' \
    --cacert "$ca" --cert "$client_cert" --key "$client_key" \
    "$url/healthz" > "$work/anonymous.code" 2> "$work/anonymous.err" || status=$?
if [ "$status" != 0 ]; then
    printf 'agentd-probe: unauthenticated health request failed before HTTP (curl %s)\n' "$status" >&2
    sed -n '1,10p' "$work/anonymous.err" >&2
    exit 1
fi
if [ "$(cat "$work/anonymous.code")" != 401 ]; then
    printf 'agentd-probe: health without the agent program bearer returned HTTP %s, not 401\n' \
        "$(cat "$work/anonymous.code")" >&2
    exit 1
fi

status=0
curl --silent --show-error --max-time 10 --output /dev/null \
    --cacert "$ca" --config "$work/bearer.conf" \
    "$url/healthz" > /dev/null 2> "$work/unverified.err" || status=$?
if [ "$status" = 0 ]; then
    printf 'agentd-probe: the boundary accepted a client without a certificate\n' >&2
    exit 1
fi

printf 'agentd-probe: %s ready; bearer and client certificate both enforced\n' "$url"
