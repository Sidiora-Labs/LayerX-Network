#!/bin/sh
set -eu

inventory=${1:-tools/workspace/platform-dependency-roots.txt}

while IFS='|' read -r kind path classification; do
    case "$kind" in \#*|'') continue ;; esac
    test -s "$path"
    case "$classification" in
        local-source-install|compile-only-manifest|install-resolved|local-replacements) ;;
        locked)
            test "$kind" = rust
            test -s "${path%/Cargo.toml}/Cargo.lock"
            ;;
        revision-locked)
            test "$kind" = swift
            test -s "${path%/Package.swift}/Package.resolved"
            ;;
        *) echo "unknown Platform dependency classification: $classification" >&2; exit 1 ;;
    esac
done < "$inventory"
