#!/bin/sh
#
# mix-in-builder.sh
#
# One recipe for every mix command this repository runs against
# explorer/backend. The explorer gates, the lint scripts and every task that
# compiles, formats or tests the backend go through this script, so there is a
# single place to change when the toolchain moves.
#
# The database is disposable on purpose: a second `mix test` run against a
# database an earlier run already migrated trips an upstream migration-cache
# defect, so every invocation gets its own sidecar unless --reuse-db asks for
# the previous one back.
#
# Exit codes: the mix exit code; 1 when the runtime, the image or a mount does
# not resolve; 130, 143 or 129 when interrupted, terminated or hung up.

set -eu

readonly DEFAULT_IMAGE='explorer-elixir-builder:v10.2.6'
readonly DATABASE_IMAGE='postgres:16'
readonly CONTAINER_WORKDIR='/app'
readonly MIX_HOME_IN_IMAGE='/opt/mix'
readonly DATABASE_READY_ATTEMPTS=60

# Runs inside the builder container. The block_scout_web test helper starts
# Wallaby unconditionally, so a run that reaches that application needs the
# headless browser driver before mix does anything.
CONTAINER_SCRIPT=$(
  cat <<'CONTAINER_SCRIPT_END'
set -e
if [ "${MIX_IN_BUILDER_BROWSER_DRIVER:-0}" = "1" ] && ! command -v chromedriver >/dev/null 2>&1; then
  apk add --no-cache chromium-chromedriver >&2
fi
exec mix "$@"
CONTAINER_SCRIPT_END
)
readonly CONTAINER_SCRIPT

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
repo_root=$(CDPATH='' cd -- "$script_dir/../../.." && pwd -P)
backend_dir=$repo_root/explorer/backend
dockerfile=$script_dir/Dockerfile.elixir-builder

image=${MIX_IN_BUILDER_IMAGE:-$DEFAULT_IMAGE}
mix_env=${MIX_ENV:-test}
build_path=${MIX_BUILD_PATH:-/build}
chain_type=${CHAIN_TYPE:-paxeer_x}
jsonrpc_variant=${ETHEREUM_JSONRPC_VARIANT:-paxeer_x}
pguser=${PGUSER:-postgres}
pgpassword=${PGPASSWORD:-postgres}

reuse_db=0
check_only=0
sidecar_started=0
database_container=''

log() {
  printf 'mix-in-builder: %s\n' "$*" >&2
}

die() {
  log "$*"
  exit 1
}

usage() {
  cat >&2 <<'USAGE'
Usage: explorer/deploy/tools/mix-in-builder.sh [options] <mix args>

Runs one mix command against explorer/backend inside the pinned Elixir builder
image, with a disposable PostgreSQL sidecar and a named build volume.

  explorer/deploy/tools/mix-in-builder.sh compile
  explorer/deploy/tools/mix-in-builder.sh format --check-formatted
  explorer/deploy/tools/mix-in-builder.sh test apps/explorer/test/explorer

Options come before the mix arguments; -- ends them.

  --reuse-db      reuse the PostgreSQL sidecar a previous --reuse-db
                  invocation left running, and leave it running at exit, so a
                  suite can be run twice against one database
  --image <ref>   builder image reference to run instead of the default
  --check         resolve the container runtime, the builder image and the
                  mounts, print the command that would run, and exit without
                  starting anything
  -h, --help      print this usage

Environment

  MIX_ENV                        defaults to test
  MIX_BUILD_PATH                 defaults to /build, where the named volume
                                 holding compiled artefacts is mounted, so a
                                 rerun does not recompile dependencies
  MIX_BUILD_VOLUME               name of that volume; the default is derived
                                 from the backend path, so two working trees
                                 never share one build
  CHAIN_TYPE                     defaults to paxeer_x
  ETHEREUM_JSONRPC_VARIANT       defaults to paxeer_x
  PGUSER, PGPASSWORD             default to postgres, the credentials
                                 apps/explorer/config/test.exs expects
  MIX_IN_BUILDER_IMAGE           same as --image
  MIX_IN_BUILDER_BROWSER_DRIVER  1 installs the headless browser driver, 0
                                 never installs it; unset lets the script
                                 decide from the mix arguments
USAGE
}

# Print an argument list the way a shell would accept it back.
print_command() {
  for argument in "$@"; do
    case $argument in
      '' | *[!A-Za-z0-9_@%+=:,./-]*)
        printf " '%s'" "$(printf '%s' "$argument" | sed "s/'/'\\\\''/g")"
        ;;
      *)
        printf ' %s' "$argument"
        ;;
    esac
  done
  printf '\n'
}

tree_digest() {
  printf '%s' "$backend_dir" | cksum | cut -d ' ' -f 1
}

# One build volume per working tree: the basename keeps the name readable, the
# checksum of the absolute path keeps two trees with the same basename apart.
default_build_volume() {
  printf 'explorer-mix-build-%s-%s' \
    "$(printf '%s' "${repo_root##*/}" | tr -c 'A-Za-z0-9_.-' '-')" \
    "$(tree_digest)"
}

database_container_name() {
  if [ "$reuse_db" -eq 1 ]; then
    printf 'explorer-mix-db-%s' "$(tree_digest)"
  else
    printf 'explorer-mix-db-%s-%s' "$(tree_digest)" "$$"
  fi
}

browser_driver_needed() {
  case ${MIX_IN_BUILDER_BROWSER_DRIVER:-} in
    1) return 0 ;;
    0) return 1 ;;
  esac

  [ "$#" -gt 0 ] || return 1
  [ "$1" = 'test' ] || return 1
  shift

  path_arguments=0
  for argument in "$@"; do
    case $argument in
      *block_scout_web*) return 0 ;;
      apps/* | test/* | *_test.exs | *_test.exs:*) path_arguments=$((path_arguments + 1)) ;;
    esac
  done

  # No path narrows the run, so it reaches every application in the umbrella.
  [ "$path_arguments" -eq 0 ]
}

remove_sidecar() {
  if [ "$sidecar_started" -eq 1 ] && [ "$reuse_db" -eq 0 ] && [ -n "$database_container" ]; then
    sidecar_started=0
    docker rm --force --volumes "$database_container" >/dev/null 2>&1 || true
  fi
}

ensure_runtime() {
  command -v docker >/dev/null 2>&1 || die 'docker is not on PATH'
  docker version --format '{{.Server.Version}}' >/dev/null 2>&1 ||
    die 'the container runtime is not answering'
}

ensure_mounts() {
  for mount in apps config rel mix.exs mix.lock .formatter.exs; do
    [ -e "$backend_dir/$mount" ] || die "mount source $backend_dir/$mount does not exist"
  done
}

ensure_image() {
  if docker image inspect "$image" >/dev/null 2>&1; then
    return 0
  fi

  [ -f "$dockerfile" ] || die "builder image $image is absent and $dockerfile does not exist"
  log "builder image $image is absent, building it from ${dockerfile#"$repo_root"/}"
  docker build --file "$dockerfile" --tag "$image" "$backend_dir" >&2
}

start_sidecar() {
  if [ "$reuse_db" -eq 1 ] && docker container inspect "$database_container" >/dev/null 2>&1; then
    if [ "$(docker container inspect --format '{{.State.Running}}' "$database_container")" != 'true' ]; then
      docker start "$database_container" >/dev/null
    fi
    sidecar_started=1
    log "reusing the database sidecar $database_container"
    return 0
  fi

  docker rm --force --volumes "$database_container" >/dev/null 2>&1 || true
  docker run \
    --detach \
    --name "$database_container" \
    --env POSTGRES_USER="$pguser" \
    --env POSTGRES_PASSWORD="$pgpassword" \
    "$DATABASE_IMAGE" \
    postgres \
    -c fsync=off \
    -c synchronous_commit=off \
    -c full_page_writes=off \
    -c max_connections=200 >/dev/null
  sidecar_started=1

  attempt=0
  until docker exec "$database_container" pg_isready --quiet --username "$pguser" >/dev/null 2>&1; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge "$DATABASE_READY_ATTEMPTS" ]; then
      die "the database sidecar $database_container did not become ready"
    fi
    sleep 1
  done
  log "the database sidecar $database_container is ready"
}

# Builds the docker invocation around the mix arguments, then either prints it
# or runs it. The mix arguments stay positional the whole way, so a path with a
# space in it survives.
mix_in_container() {
  mode=$1
  shift

  set -- run \
    --rm \
    --network "container:$database_container" \
    --shm-size 1g \
    --volume "$backend_dir/apps:$CONTAINER_WORKDIR/apps" \
    --volume "$backend_dir/config:$CONTAINER_WORKDIR/config" \
    --volume "$backend_dir/rel:$CONTAINER_WORKDIR/rel" \
    --volume "$backend_dir/mix.exs:$CONTAINER_WORKDIR/mix.exs" \
    --volume "$backend_dir/mix.lock:$CONTAINER_WORKDIR/mix.lock" \
    --volume "$backend_dir/.formatter.exs:$CONTAINER_WORKDIR/.formatter.exs" \
    --volume "$build_volume:$build_path" \
    --env MIX_ENV="$mix_env" \
    --env MIX_BUILD_PATH="$build_path" \
    --env MIX_HOME="$MIX_HOME_IN_IMAGE" \
    --env CHAIN_TYPE="$chain_type" \
    --env ETHEREUM_JSONRPC_VARIANT="$jsonrpc_variant" \
    --env PGUSER="$pguser" \
    --env PGPASSWORD="$pgpassword" \
    --env MIX_IN_BUILDER_BROWSER_DRIVER="$browser_driver" \
    --workdir "$CONTAINER_WORKDIR" \
    "$image" \
    sh -c "$CONTAINER_SCRIPT" mix-in-builder "$@"

  if [ "$mode" = 'print' ]; then
    print_command docker "$@" >&2
    return 0
  fi

  mix_status=0
  docker "$@" || mix_status=$?
  return "$mix_status"
}

while [ "$#" -gt 0 ]; do
  case $1 in
    --reuse-db)
      reuse_db=1
      shift
      ;;
    --image)
      [ "$#" -ge 2 ] || die '--image needs an image reference'
      image=$2
      shift 2
      ;;
    --image=*)
      image=${1#--image=}
      [ -n "$image" ] || die '--image needs an image reference'
      shift
      ;;
    --check)
      check_only=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    *)
      break
      ;;
  esac
done

[ "$#" -gt 0 ] || [ "$check_only" -eq 1 ] || die 'no mix arguments given; try --help'

build_volume=${MIX_BUILD_VOLUME:-$(default_build_volume)}
database_container=$(database_container_name)

if browser_driver_needed "$@"; then
  browser_driver=1
else
  browser_driver=0
fi

ensure_runtime
ensure_mounts

if [ "$check_only" -eq 1 ]; then
  if docker image inspect "$image" >/dev/null 2>&1; then
    log "the builder image $image resolves"
  elif [ -f "$dockerfile" ]; then
    log "the builder image $image is absent, it would be built from ${dockerfile#"$repo_root"/}"
  else
    die "the builder image $image is absent and $dockerfile does not exist"
  fi
  log "every mount source resolves, build volume $build_volume, sidecar $database_container"
  mix_in_container print "$@"
  exit 0
fi

trap remove_sidecar EXIT
trap 'remove_sidecar; exit 130' INT
trap 'remove_sidecar; exit 143' TERM
trap 'remove_sidecar; exit 129' HUP

ensure_image
start_sidecar

mix_in_container print "$@"

status=0
mix_in_container run "$@" || status=$?

remove_sidecar
exit "$status"
