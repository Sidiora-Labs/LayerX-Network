#!/bin/sh
set -eu

inventory=${1:-tools/workspace/platform-dependency-roots.txt}
actual=$(mktemp)
declared=$(mktemp)
trap 'rm -f "$actual" "$declared"' EXIT HUP INT TERM

find platform -type d \( -name target -o -name node_modules -o -name build -o -name .build \) -prune -o \
    -type f \( -name Cargo.toml -o -name go.mod -o -name package.json -o -name pom.xml \
    -o -name Package.swift -o -name Package.resolved -o -name '*.csproj' -o -name pyproject.toml \
    -o -name requirements.txt -o -name settings.gradle.kts -o -name build.gradle.kts \) \
    -print | LC_ALL=C sort > "$actual"
awk -F '|' '!/^#/ && NF == 3 { print $2 }' "$inventory" | LC_ALL=C sort > "$declared"
cmp "$actual" "$declared"

while IFS='|' read -r kind path classification; do
    case "$kind" in \#*|'') continue ;; esac
    test -s "$path"
    case "$classification" in
        locked-workspace-root|locked-independent)
            test "$kind" = rust
            test -s "${path%/Cargo.toml}/Cargo.lock"
            ;;
        revision-locked)
            test "$kind" = swift-lock
            grep -q '"revision"' "$path"
            ;;
        covered-by-root-npm)
            test "$kind" = npm
            directory=${path%/package.json}
            grep -Fq '"'"$directory"'"' package.json
            ;;
        covered-by-platform-cargo|covered-by-human-dashboard-lock|covered-by-agents-parent|covered-by-express-parent|covered-by-swift-lock|local-source-install|compile-only-manifest|install-resolved|local-replacements|environment-blocked-unverified-gradle) ;;
        *) echo "unknown Platform dependency classification: $classification" >&2; exit 1 ;;
    esac
done < "$inventory"
