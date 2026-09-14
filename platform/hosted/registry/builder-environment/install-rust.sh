#!/bin/sh
set -eu
mkdir -p /tmp/rust-dist/2025-11-10
while read -r digest url; do
    case "$url" in
        https://static.rust-lang.org/dist/*) file=/tmp/rust-dist/${url#https://static.rust-lang.org/dist/} ;;
        https://static.rust-lang.org/rustup/archive/1.28.2/x86_64-unknown-linux-gnu/rustup-init) file=/tmp/rustup-init ;;
        *) echo 'Unrecognized pinned Rust input' >&2; exit 1 ;;
    esac
    curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 "$url" -o "$file"
    printf '%s  %s\n' "$digest" "$file" | sha256sum --check --strict
done < /tmp/rust-downloads.lock
sed -i 's|https://static.rust-lang.org/dist/|file:///tmp/rust-dist/|g' /tmp/rust-dist/channel-rust-1.91.1.toml
sha256sum /tmp/rust-dist/channel-rust-1.91.1.toml > /tmp/rust-dist/channel-rust-1.91.1.toml.sha256
chmod 0755 /tmp/rustup-init
RUSTUP_DIST_SERVER=file:///tmp/rust-dist /tmp/rustup-init -y --no-modify-path \
    --profile minimal --default-toolchain 1.91.1 --component clippy --component rustfmt \
    --target wasm32-unknown-unknown
rm -rf /tmp/rust-dist /tmp/rustup-init
