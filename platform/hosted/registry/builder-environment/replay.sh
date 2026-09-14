#!/bin/sh
# Replays the registry's exact bwrap invocation (platform/hosted/registry/src/builder.rs)
# against the flattened rootfs for one source tree and one plan command, and
# checks the declared artifact exists.
set -eu
rootfs=$1; source=$2; artifact=$3; shift 3
bwrap --unshare-all --die-with-parent --new-session --disable-userns --cap-drop ALL --clearenv \
    --ro-bind "$rootfs" / --dir /build --bind "$source" /build \
    --dir /tmp --tmpfs /tmp --proc /proc --dev /dev \
    --chdir /build --setenv HOME /tmp --setenv TMPDIR /tmp \
    --setenv CARGO_HOME /opt/cache/cargo --setenv RUSTUP_HOME /opt/cache/rustup \
    --setenv CARGO_TERM_COLOR never --setenv CARGO_NET_OFFLINE true \
    --setenv TZ UTC --setenv LC_ALL C --setenv SOURCE_DATE_EPOCH 1 \
    --setenv RUSTFLAGS --remap-path-prefix=/build=/layerx/source \
    -- /bin/layerx-build "$@"
test -s "$source/$artifact"
sha256sum "$source/$artifact"
