#!/usr/bin/env bash
# Sourced by the callers that start `layerxd --serve` from a
# bootstrap-generated sequencer.env outside the supervisor (the daemon test
# scripts). sequencer.env no longer carries the seed; it names the key file
# as LAYERX_NODE_SEQUENCER_KEY_FILE and the supervisor reads it. This file
# gives every other caller the same delivery:
#
#   source platform/hosted/node/sequencer-env.sh
#   layerx_sequencer_environment "$data/sequencer.env"
#   exec layerxd --serve "$data/sequencer.conf"
#
# layerx_sequencer_environment exports every LAYERX_* KEY=VALUE line of the
# file, refuses a LAYERX_NODE_SEQUENCER_PRIVATE_KEY line, reads the seed
# (32 raw bytes or 64 hex characters) from LAYERX_NODE_SEQUENCER_KEY_FILE
# through a read-only descriptor and exports it as
# LAYERX_NODE_SEQUENCER_PRIVATE_KEY, the only form cmd/layerxd reads. It
# returns 1 with a message on stderr instead of exporting anything partial.

layerx_sequencer_environment() {
    local env_file=$1 line key key_file size descriptor seed
    [ -r "$env_file" ] || { printf 'sequencer-env: environment file missing: %s\n' "$env_file" >&2; return 1; }
    while IFS= read -r line || [ -n "$line" ]; do
        [ -n "$line" ] || continue
        [[ $line =~ ^(LAYERX_[A-Z0-9_]+)=([^[:cntrl:]]*)$ ]] || {
            printf 'sequencer-env: %s carries a line that is not a LAYERX_* KEY=VALUE pair: %s\n' "$env_file" "${line%%=*}" >&2
            return 1
        }
        key=${BASH_REMATCH[1]}
        [ "$key" != LAYERX_NODE_SEQUENCER_PRIVATE_KEY ] || {
            printf 'sequencer-env: %s must not carry LAYERX_NODE_SEQUENCER_PRIVATE_KEY; the seed is read from LAYERX_NODE_SEQUENCER_KEY_FILE\n' "$env_file" >&2
            return 1
        }
        export "$line"
    done < "$env_file"
    key_file=${LAYERX_NODE_SEQUENCER_KEY_FILE:-}
    [ -n "$key_file" ] || { printf 'sequencer-env: LAYERX_NODE_SEQUENCER_KEY_FILE missing from %s\n' "$env_file" >&2; return 1; }
    [[ $key_file = /* ]] || { printf 'sequencer-env: LAYERX_NODE_SEQUENCER_KEY_FILE must be an absolute path: %s\n' "$key_file" >&2; return 1; }
    [ -f "$key_file" ] && [ -r "$key_file" ] || { printf 'sequencer-env: sequencer key file is not a readable regular file: %s\n' "$key_file" >&2; return 1; }
    size=$(stat -L -c %s "$key_file")
    [ "$size" -le 128 ] || { printf 'sequencer-env: sequencer key file must hold 32 raw bytes or 64 hex characters: %s\n' "$key_file" >&2; return 1; }
    exec {descriptor}<"$key_file" || { printf 'sequencer-env: sequencer key file could not be opened: %s\n' "$key_file" >&2; return 1; }
    if [ "$size" -eq 32 ]; then
        seed=$(od -An -v -tx1 <&"$descriptor" | tr -d ' \n')
    else
        seed=$(tr -d ' \t\r\n' <&"$descriptor" | tr 'A-F' 'a-f')
    fi
    exec {descriptor}<&-
    [[ $seed =~ ^[0-9a-f]{64}$ ]] || {
        printf 'sequencer-env: sequencer key file must hold 32 raw bytes or 64 hex characters: %s\n' "$key_file" >&2
        return 1
    }
    export LAYERX_NODE_SEQUENCER_PRIVATE_KEY="$seed"
}
