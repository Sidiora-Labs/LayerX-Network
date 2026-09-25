#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
usage: tools/ci/human-web-image-build.sh [--tag REF] [--work-dir DIR] [--keep-image]

Builds the human web image from the repository's tracked sources, using the
same build context the beta cluster bring-up packs, so a failure here is the
failure the browser-performance job sees when it brings its cluster up.

The context is a tar of every tracked file of the repository, with the
environment files, the qualification logs and the two working notes left out,
exactly as platform/hosted/tests/beta-cluster.sh packs it; the Dockerfile is
human/apps/web/Dockerfile, read from inside that context. The build log is
written to the work directory and its path is printed on failure. When the
image builds, its entry point is checked: the standalone server the image's
command runs has to be present in the image.

  --tag REF       image reference to build (default layerx-human-web:local)
  --work-dir DIR  context and log directory (default build/human-web-image)
  --keep-image    leave the built image in the local image store

Exit status: 0 when the image builds and carries its entry point, 1 when the
build or the entry-point check fails, 2 on usage or environment errors.
EOF
}

REPO_ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
IMAGE_REF=layerx-human-web:local
WORK_DIR=$REPO_ROOT/build/human-web-image
KEEP_IMAGE=0
DOCKERFILE=human/apps/web/Dockerfile

while [ "$#" -gt 0 ]; do
    case "$1" in
        --tag) [ "$#" -ge 2 ] || { usage >&2; exit 2; }; IMAGE_REF=$2; shift 2 ;;
        --work-dir) [ "$#" -ge 2 ] || { usage >&2; exit 2; }; WORK_DIR=$2; shift 2 ;;
        --keep-image) KEEP_IMAGE=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) printf 'human-web-image-build: unknown argument %s\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
done

command -v docker >/dev/null 2>&1 || {
    printf 'human-web-image-build: docker is required to build the human web image\n' >&2
    exit 2
}
[ -f "$REPO_ROOT/$DOCKERFILE" ] || {
    printf 'human-web-image-build: %s is missing\n' "$DOCKERFILE" >&2
    exit 2
}

mkdir -p "$WORK_DIR"
WORK_DIR=$(cd -- "$WORK_DIR" && pwd)
CONTEXT=$WORK_DIR/context.tar
LOG=$WORK_DIR/build-layerx-human-web.log

printf 'human-web-image-build: packing the build context from the tracked source files of %s\n' "$REPO_ROOT"
(cd "$REPO_ROOT" && git ls-files -z --cached \
    | while IFS= read -r -d '' path; do
        case "/$path" in */.env|*/.env.*|/qual-logs/*|/NEEDS.md|/STATUS.md) continue ;; esac
        [ -e "$path" ] && printf '%s\0' "$path"
    done \
    | tar --null --files-from - -cf "$CONTEXT")

printf 'human-web-image-build: building %s from %s\n' "$IMAGE_REF" "$DOCKERFILE"
if ! docker build --file "$DOCKERFILE" --tag "$IMAGE_REF" - < "$CONTEXT" > "$LOG" 2>&1; then
    tail -n 40 "$LOG" >&2
    printf 'human-web-image-build: image build failed for layerx-human-web (log %s)\n' "$LOG" >&2
    exit 1
fi

if ! docker run --rm --entrypoint node "$IMAGE_REF" \
    -e 'require("node:fs").accessSync("/app/human/apps/web/server.js")' >> "$LOG" 2>&1; then
    tail -n 40 "$LOG" >&2
    printf 'human-web-image-build: the built image does not carry its standalone server (log %s)\n' "$LOG" >&2
    exit 1
fi

[ "$KEEP_IMAGE" = 1 ] || docker image rm "$IMAGE_REF" >> "$LOG" 2>&1 || true
rm -f "$CONTEXT"
printf 'human-web-image-build: built layerx-human-web from the tracked sources (log %s)\n' "$LOG"
