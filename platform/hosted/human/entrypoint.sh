#!/bin/sh
set -eu
umask 077
role=${1:?Human role is required}
shift
private=/run/human-private/$role
mkdir -p "$private"
chmod 0700 "$private"
copy_material() {
    name=$1
    test -s "/run/human-material/$name"
    install -m 0600 "/run/human-material/$name" "$private/$name"
}
case "$role" in
    components)
        copy_material kms-client.der
        copy_material kms-client-key.der
        copy_material ca.der
        copy_material purpose-catalog.json
        exec /usr/local/bin/layerx-human-components "$@"
        ;;
    kms)
        for name in kms-server.der kms-server-key.der kms-client.der kms-executor.der ca.der kms-seal registry.json; do
            copy_material "$name"
        done
        exec /usr/local/bin/layerx-human-kms "$@"
        ;;
    agent)
        copy_material ca.der
        copy_material session-operator
        export LAYERX_AGENT_HUMAN_AUTHORITY_BEARER="$(cat /run/human-material/authority-token)"
        export LAYERX_AGENT_PROGRAM_BEARER_TOKEN="$(cat /run/human-material/program-token)"
        printf 'header = "Authorization: Bearer %s"\n' "$LAYERX_AGENT_PROGRAM_BEARER_TOKEN" > "$private/probe.conf"
        exec /usr/local/bin/layerx-agentd "$@"
        ;;
    identity)
        copy_material recovery-policy.json
        exec /usr/local/bin/layerx-human-identity-provider "$@"
        ;;
    security)
        copy_material trust-history
        exec /usr/local/bin/layerx-human-security-provider "$@"
        ;;
    movement)
        for name in ca.der kms-executor.der kms-executor-key.der; do
            copy_material "$name"
        done
        exec /usr/local/bin/layerx-human-movement-provider "$@"
        ;;
    service) exec /usr/local/bin/layerx-human-service "$@" ;;
    *) printf 'unknown Human role\n' >&2; exit 64 ;;
esac
