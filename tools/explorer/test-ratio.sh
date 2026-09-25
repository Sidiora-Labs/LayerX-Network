#!/bin/sh
#
# test-ratio.sh
#
# The explorer test-ratio gate. For a git range it lists the files the range
# changes under explorer/, classifies each one as a source file, a test file or
# neither, and fails when the range carries fewer test files than source files
# or when a changed source file is left without a changed test that references
# it.
#
# Run from anywhere inside the repository:
#
#   tools/explorer/test-ratio.sh BASE..HEAD
#   tools/explorer/test-ratio.sh BASE...HEAD
#   tools/explorer/test-ratio.sh BASE HEAD
#
# Exit codes: 0 when the ratio holds or the range changes no explorer source
# file; 1 when the ratio does not hold; 2 when the arguments or the repository
# do not resolve.

set -eu

program=test-ratio

log() {
    printf '%s: %s\n' "$program" "$*" >&2
}

die() {
    log "$*"
    exit 2
}

usage() {
    cat <<'USAGE'
Usage: tools/explorer/test-ratio.sh BASE..HEAD
       tools/explorer/test-ratio.sh BASE...HEAD
       tools/explorer/test-ratio.sh BASE HEAD

Counts the source files and the test files the range changes under explorer/
and fails when the test files are fewer than the source files or when a changed
source file has no changed test that references it. BASE...HEAD measures from
the merge base of the two revisions, which is the range the pull request job
passes.

Source files: an Elixir .ex, a TypeScript .ts or .tsx that is not a test, and a
Rust .rs that is not a test module.

Test files: an Elixir *_test.exs, a TypeScript *.test.ts, *.test.tsx, *.spec.ts,
*.spec.tsx or *.pw.tsx, and a Rust file under a tests/ directory or carrying a
#[cfg(test)] module.

Everything else - shell, YAML, JSON, environment presets, SVG and Markdown -
counts as neither and carries no test requirement.

A changed test file counts towards the ratio only when it references one of the
changed source files: by module name, by an exported name, by a path fragment,
or by sitting where that source file's test belongs.

Exit codes: 0 when the ratio holds or the range changes no explorer source file,
1 when it does not, 2 when the arguments or the repository do not resolve.
USAGE
}

while [ $# -gt 0 ]; do
    case $1 in
        -h | --help)
            usage
            exit 0
            ;;
        --)
            shift
            break
            ;;
        -*)
            usage >&2
            die "unknown option $1"
            ;;
        *)
            break
            ;;
    esac
done

if [ $# -eq 1 ]; then
    range=$1
    case $range in
        *...*)
            base_argument=${range%%...*}
            head_argument=${range#*...}
            from_merge_base=1
            ;;
        *..*)
            base_argument=${range%%..*}
            head_argument=${range#*..}
            from_merge_base=0
            ;;
        *)
            usage >&2
            die "$range is not a git range: write BASE..HEAD, BASE...HEAD or two revisions"
            ;;
    esac
elif [ $# -eq 2 ]; then
    base_argument=$1
    head_argument=$2
    from_merge_base=0
else
    usage >&2
    die "a git range is required"
fi

[ -n "$base_argument" ] || base_argument=HEAD
[ -n "$head_argument" ] || head_argument=HEAD

git rev-parse --show-toplevel >/dev/null 2>&1 || die "this is not a git repository"
repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root" || die "the repository root $repo_root is not reachable"

base_rev=$(git rev-parse --verify --quiet "$base_argument^{commit}") ||
    die "the revision $base_argument does not resolve to a commit"
head_rev=$(git rev-parse --verify --quiet "$head_argument^{commit}") ||
    die "the revision $head_argument does not resolve to a commit"

if [ "$from_merge_base" -eq 1 ]; then
    base_rev=$(git merge-base "$base_rev" "$head_rev") ||
        die "$base_argument and $head_argument have no merge base"
fi

work_dir=$(mktemp -d) || die "a temporary directory could not be created"
trap 'rm -rf "$work_dir"' EXIT HUP INT TERM

source_list=$work_dir/sources
test_list=$work_dir/tests
referenced_list=$work_dir/referenced
unreferencing_list=$work_dir/unreferencing
source_body=$work_dir/source-body
test_body=$work_dir/test-body

: >"$source_list"
: >"$test_list"
: >"$referenced_list"
: >"$unreferencing_list"

# The blob as the head revision carries it. A path the head revision does not
# carry yields nothing rather than an error, so a classification never depends
# on the working tree.
file_at_head() {
    git show "$head_rev:$1" 2>/dev/null || true
}

classify() {
    classify_path=$1
    case $classify_path in
        *_test.exs)
            printf 'test\n'
            return 0
            ;;
        *.test.ts | *.test.tsx | *.spec.ts | *.spec.tsx | *.pw.tsx)
            printf 'test\n'
            return 0
            ;;
        *.ex)
            printf 'source\n'
            return 0
            ;;
        *.ts | *.tsx)
            printf 'source\n'
            return 0
            ;;
        *.rs)
            case $classify_path in
                tests/* | */tests/*)
                    printf 'test\n'
                    return 0
                    ;;
            esac
            if file_at_head "$classify_path" | grep -q '#\[cfg(test)\]'; then
                printf 'test\n'
            else
                printf 'source\n'
            fi
            return 0
            ;;
    esac
    printf 'neither\n'
}

# Identifiers reach the token file escaped for grep -E and prefixed with I:.
# Names shorter than three characters are dropped: they match too much to say
# anything about a reference.
emit_identifiers() {
    awk 'NF > 0 && length($0) >= 3' |
        sort -u |
        sed -e 's/[.$]/\\&/g' -e 's/^/I:/' >>"$1"
}

emit_paths() {
    emit_paths_out=$1
    shift
    for emit_paths_value in "$@"; do
        printf 'P:%s\n' "$emit_paths_value" >>"$emit_paths_out"
    done
}

build_tokens() {
    build_path=$1
    build_out=$2
    : >"$build_out"

    emit_paths "$build_out" "$build_path"
    build_stripped=${build_path%.*}
    emit_paths "$build_out" "$build_stripped"
    build_suffix=$build_stripped
    while :; do
        case $build_suffix in
            */*/*)
                build_suffix=${build_suffix#*/}
                emit_paths "$build_out" "$build_suffix"
                ;;
            *)
                break
                ;;
        esac
    done

    file_at_head "$build_path" >"$source_body"

    case $build_path in
        *.ex)
            { grep -oE '^[[:space:]]*(defmodule|defprotocol)[[:space:]]+[A-Za-z0-9_.]+' "$source_body" || true; } |
                awk '{ print $NF }' |
                emit_identifiers "$build_out"
            ;;
        *.ts | *.tsx)
            {
                { grep -oE '^[[:space:]]*export[[:space:]]+(default[[:space:]]+)?(async[[:space:]]+)?(function|const|let|var|class|interface|type|enum)[[:space:]]+[A-Za-z_$][A-Za-z0-9_$]*' "$source_body" || true; } |
                    awk '{ print $NF }'
                { grep -oE '^[[:space:]]*export[[:space:]]+default[[:space:]]+[A-Za-z_$][A-Za-z0-9_$]*' "$source_body" || true; } |
                    awk '{ print $NF }'
                { grep -oE '^[[:space:]]*export[[:space:]]*\{[^}]*\}' "$source_body" || true; } |
                    awk '{ gsub(/[^A-Za-z0-9_$]+/, "\n"); print }'
            } |
                awk '$0 !~ /^(export|default|async|function|const|let|var|class|interface|type|enum|as|from)$/' |
                emit_identifiers "$build_out"
            ;;
        *.rs)
            build_stem=${build_path##*/}
            build_stem=${build_stem%.rs}
            {
                { grep -oE '^[[:space:]]*pub([[:space:]]*\([^)]*\))?[[:space:]]+(async[[:space:]]+)?(unsafe[[:space:]]+)?(fn|struct|enum|trait|type|mod|const|static)[[:space:]]+[A-Za-z_][A-Za-z0-9_]*' "$source_body" || true; } |
                    awk '{ print $NF }'
                case $build_stem in
                    lib | main | mod) ;;
                    *) printf '%s\n' "$build_stem" ;;
                esac
            } |
                emit_identifiers "$build_out"
            ;;
    esac
}

# The path a test file would carry if it were the mirror test of an Elixir
# source module, under the umbrella layout the backend already uses.
elixir_mirror_path() {
    printf '%s' "$1" | sed -e 's![/]lib[/]!/test/!' -e 's!\.ex$!_test.exs!'
}

test_stem_of() {
    stem_value=${1##*/}
    case $stem_value in
        *.test.ts | *.test.tsx | *.spec.ts | *.spec.tsx | *.pw.tsx)
            stem_value=${stem_value%.*}
            stem_value=${stem_value%.*}
            ;;
        *_test.exs)
            stem_value=${stem_value%_test.exs}
            ;;
        *)
            stem_value=${stem_value%.*}
            ;;
    esac
    printf '%s' "$stem_value"
}

references() {
    reference_test=$1
    reference_body=$2
    reference_source=$3
    reference_tokens=$4

    case $reference_source in
        *.ex)
            if [ "$(elixir_mirror_path "$reference_source")" = "$reference_test" ]; then
                return 0
            fi
            ;;
    esac

    reference_source_dir=${reference_source%/*}
    reference_source_stem=${reference_source##*/}
    reference_source_stem=${reference_source_stem%.*}
    reference_test_dir=${reference_test%/*}
    reference_test_stem=$(test_stem_of "$reference_test")
    if [ "$reference_test_dir" = "$reference_source_dir" ] &&
        [ "$reference_test_stem" = "$reference_source_stem" ]; then
        return 0
    fi

    while IFS= read -r reference_token; do
        reference_kind=${reference_token%%:*}
        reference_value=${reference_token#*:}
        [ -n "$reference_value" ] || continue
        case $reference_kind in
            I)
                reference_pattern='(^|[^A-Za-z0-9_.])'$reference_value'([^A-Za-z0-9_]|$)'
                if grep -qE -- "$reference_pattern" "$reference_body"; then
                    return 0
                fi
                ;;
            P)
                if grep -qF -- "$reference_value" "$reference_body"; then
                    return 0
                fi
                ;;
        esac
    done <"$reference_tokens"

    return 1
}

git diff --name-only --diff-filter=ACMR "$base_rev" "$head_rev" -- explorer >"$work_dir/changed"

while IFS= read -r changed_path; do
    [ -n "$changed_path" ] || continue
    case $(classify "$changed_path") in
        source) printf '%s\n' "$changed_path" >>"$source_list" ;;
        test) printf '%s\n' "$changed_path" >>"$test_list" ;;
        *) ;;
    esac
done <"$work_dir/changed"

source_count=$(grep -c '' "$source_list" || true)
test_count=$(grep -c '' "$test_list" || true)

log "range $base_rev..$head_rev"
log "source files changed: $source_count"

if [ "$source_count" -eq 0 ]; then
    log "test files changed: $test_count"
    log "the range changes no explorer source file"
    exit 0
fi

source_index=0
while IFS= read -r source_path; do
    source_index=$((source_index + 1))
    build_tokens "$source_path" "$work_dir/tokens.$source_index"
done <"$source_list"

counted_tests=0
while IFS= read -r test_path; do
    file_at_head "$test_path" >"$test_body"
    test_hits=0
    source_index=0
    while IFS= read -r source_path; do
        source_index=$((source_index + 1))
        if references "$test_path" "$test_body" "$source_path" "$work_dir/tokens.$source_index"; then
            test_hits=$((test_hits + 1))
            printf '%s\n' "$source_path" >>"$referenced_list"
        fi
    done <"$source_list"
    if [ "$test_hits" -gt 0 ]; then
        counted_tests=$((counted_tests + 1))
    else
        printf '%s\n' "$test_path" >>"$unreferencing_list"
    fi
done <"$test_list"

log "test files changed: $test_count ($counted_tests referencing a changed source file)"

failed=0

while IFS= read -r source_path; do
    if ! grep -qxF -- "$source_path" "$referenced_list"; then
        log "no changed test references $source_path"
        failed=1
    fi
done <"$source_list"

while IFS= read -r test_path; do
    log "$test_path references no changed source file"
done <"$unreferencing_list"

if [ "$counted_tests" -lt "$source_count" ]; then
    log "FAILED: $counted_tests referencing test file(s) for $source_count changed source file(s)"
    failed=1
fi

if [ "$failed" -ne 0 ]; then
    log "every changed source file under explorer/ needs a changed test that references it"
    exit 1
fi

log "the test ratio holds"
