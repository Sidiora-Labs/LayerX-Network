#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$root"

publication_files=$(mktemp)
publication_files_nul=$(mktemp)
gitlinks=$(mktemp)
declared_submodules=$(mktemp)
undeclared_gitlinks=$(mktemp)
trap 'rm -f "$publication_files" "$publication_files_nul" "$gitlinks" "$declared_submodules" "$undeclared_gitlinks"' EXIT HUP INT TERM

git ls-files --cached --others --exclude-standard > "$publication_files"
while IFS= read -r publication_file; do
    if [ -f "$publication_file" ] || [ -L "$publication_file" ]; then
        printf '%s\0' "$publication_file" >> "$publication_files_nul"
    fi
done < "$publication_files"

tracked_worktrees=$(git ls-files --cached | awk '/^\.worktrees\// { print }')
if [ -n "$tracked_worktrees" ]; then
    echo "local worktree paths must not be tracked:" >&2
    printf '%s\n' "$tracked_worktrees" >&2
    exit 1
fi

git ls-files --stage | awk '$1 == "160000" { sub(/^[^\t]*\t/, ""); print }' | sort -u > "$gitlinks"
if [ -f .gitmodules ]; then
    git config -f .gitmodules --get-regexp '^submodule\..*\.path$' 2>/dev/null \
        | cut -d ' ' -f 2- \
        | sort -u > "$declared_submodules" || true
fi
comm -23 "$gitlinks" "$declared_submodules" > "$undeclared_gitlinks"
if [ -s "$undeclared_gitlinks" ]; then
    echo "tracked gitlinks must be declared in .gitmodules:" >&2
    cat "$undeclared_gitlinks" >&2
    exit 1
fi

mutable_action_refs=$(find .github/workflows -type f \( -name '*.yml' -o -name '*.yaml' \) -exec awk '
    /^[[:space:]]*(-[[:space:]]*)?uses:[[:space:]]*/ {
        ref = $0
        sub(/^[[:space:]]*(-[[:space:]]*)?uses:[[:space:]]*/, "", ref)
        sub(/[[:space:]#].*$/, "", ref)
        if (ref ~ /^\.\//) {
            next
        }
        if (ref ~ /^docker:\/\//) {
            digest = ref
            sub(/^.*@sha256:/, "", digest)
            if (ref !~ /@sha256:/ || length(digest) != 64 || digest ~ /[^0-9A-Fa-f]/) {
                print FILENAME ":" FNR ": " ref
            }
            next
        }
        revision = ref
        sub(/^.*@/, "", revision)
        if (index(ref, "@") == 0 || length(revision) != 40 || revision ~ /[^0-9A-Fa-f]/) {
            print FILENAME ":" FNR ": " ref
        }
    }
' {} +)
if [ -n "$mutable_action_refs" ]; then
    echo "external GitHub Actions must use immutable commit pins and Docker actions must use sha256 digests:" >&2
    printf '%s\n' "$mutable_action_refs" >&2
    exit 1
fi

required_files=".editorconfig .gitattributes .gitignore CHANGELOG.md \
CONTRIBUTING.md LICENSE LICENSE_NOTICE.md README.md SECURITY.md SUPPORT.md \
docs/QUALIFICATION.md .github/workflows/ci.yml"
for required_file in $required_files; do
    if [ ! -s "$required_file" ]; then
        echo "required publication file is missing or empty: $required_file" >&2
        exit 1
    fi
done

while IFS= read -r required_ignore; do
    if ! rg -Fqx -- "$required_ignore" .gitignore; then
        echo "required ignore rule is missing: $required_ignore" >&2
        exit 1
    fi
done <<'EOF'
/build/
/build-*/
/.codegraph/
/.logs/
/cache/
/.worktrees/
.env
*.pem
*.key
*.vsix
EOF

large_file=''
while IFS= read -r publication_file; do
    if [ -f "$publication_file" ] && [ "$(wc -c < "$publication_file")" -gt 52428800 ]; then
        large_file=$publication_file
        break
    fi
done < "$publication_files"
if [ -n "$large_file" ]; then
    echo "publishable file exceeds 50 MiB: $large_file" >&2
    exit 1
fi

symlinks=''
while IFS= read -r publication_file; do
    if [ -L "$publication_file" ]; then
        symlinks="${symlinks}${symlinks:+
}$publication_file"
    fi
done < "$publication_files"
if [ -n "$symlinks" ]; then
    echo "symbolic links require explicit publication review:" >&2
    printf '%s\n' "$symlinks" >&2
    exit 1
fi

audit_rg() {
    audit_results=$(mktemp)
    xargs -0 -r -n 200 rg --hidden -n "$@" -- < "$publication_files_nul" > "$audit_results" || true
    if [ -s "$audit_results" ]; then
        cat "$audit_results"
        rm -f "$audit_results"
        return 0
    fi
    rm -f "$audit_results"
    return 1
}

private_refs='(/root/(Layerx-protocol|project-Quorum|private-neo-v1|matrix|layerX)(/|$)|147\.93\.139\.18)'
if audit_rg --glob '!tools/ci/public-repo-audit.sh' "$private_refs"; then
    echo "private workspace or infrastructure reference found" >&2
    exit 1
fi

secret_shapes='(-----BEGIN (RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{36,}|xox[baprs]-[A-Za-z0-9-]{20,}|sk_live_[A-Za-z0-9]{20,}|eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,})'
if audit_rg -i \
    --glob '!tools/ci/public-repo-audit.sh' \
    --glob '!paxeer-network/rpc/tests/mock_data/transactions/0x99d895ea71e5ce3a8b949ba7979a27c08080210a4ba9b46b0bb06f8126b6957d.json' \
    --glob '!paxeer-network/rpc/tests/mock_data/transactions/0x1b9ceaabadfc635aa8eb5e6d4a66ee60c826980805fa93af3913872f7b565586.json' \
    "$secret_shapes"; then
    echo "secret-shaped material found in publication set" >&2
    exit 1
fi

for script in tools/*.sh tools/ci/*.sh; do
    sh -n "$script"
done

if command -v go >/dev/null 2>&1; then
    (cd spec/specgen && go run . -root "$root" -check)
else
    echo "go is required to verify generated specification artifacts" >&2
    exit 1
fi

echo "public repository audit passed"
