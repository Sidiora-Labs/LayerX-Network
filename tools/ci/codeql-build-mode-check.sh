#!/bin/sh
set -eu

# Reads the CodeQL analysis matrix and holds it to the build modes each language
# accepts, so that a build mode the extractor rejects is caught here instead of
# in a CodeQL run. The Go extractor refuses the none build mode and names the
# modes it takes; every other analysed language stays buildless.

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
workflow=${1:-$root/.github/workflows/codeql.yml}

go_build_modes="autobuild manual"
buildless_mode="none"

fail() {
    printf 'CodeQL build mode check: %s\n' "$1" >&2
    exit 1
}

problems=
note() {
    if [ -z "$problems" ]; then
        problems=$1
    else
        problems="$problems
$1"
    fi
}

[ -f "$workflow" ] || fail "$workflow does not exist"

command -v actionlint > /dev/null 2>&1 || fail \
"actionlint is not on PATH, so $workflow cannot be checked for syntax errors"
actionlint "$workflow" || fail "actionlint reported findings in $workflow"

# The two patterns below are GitHub Actions expressions matched literally.
# shellcheck disable=SC2016
grep -qF 'languages: ${{ matrix.language }}' "$workflow" || fail \
"the CodeQL initialisation step in $workflow does not read the language from the matrix"
# shellcheck disable=SC2016
grep -qF 'build-mode: ${{ matrix.build-mode }}' "$workflow" || fail \
"the CodeQL initialisation step in $workflow does not read the build mode from the matrix"

entries=$(mktemp)
trap 'rm -f "$entries"' EXIT HUP INT TERM

awk '
function report(message) {
    printf("!\t%s\n", message)
}
function unquote(value,   first, last, quote) {
    quote = sprintf("%c", 39)
    first = substr(value, 1, 1)
    last = substr(value, length(value), 1)
    if (length(value) >= 2 && first == last && (first == "\"" || first == quote)) {
        return substr(value, 2, length(value) - 2)
    }
    return value
}
{
    line = $0
    sub(/\r$/, "", line)
    if (line ~ /^[ ]*(#.*)?$/) {
        next
    }
    match(line, /^[ ]*/)
    indentation = RLENGTH
    text = substr(line, indentation + 1)
    sub(/[ ]+$/, "", text)
    if (state == "") {
        if (text == "matrix:") {
            state = "matrix"
            matrix_indentation = indentation
        }
        next
    }
    if (state == "matrix") {
        if (indentation <= matrix_indentation) {
            state = "past"
        } else {
            if (text == "include:") {
                state = "include"
                include_indentation = indentation
            }
            next
        }
    }
    if (state == "include") {
        if (indentation <= include_indentation) {
            state = "past"
        } else {
            if (text == "-" || substr(text, 1, 2) == "- ") {
                entries++
                sub(/^-[ ]*/, "", text)
                if (text == "") {
                    next
                }
            } else if (entries == 0) {
                report("a matrix include line precedes the first entry: " text)
                next
            }
            if (text !~ /:/) {
                report("matrix entry " entries " carries a line that is not a mapping: " text)
                next
            }
            key = text
            sub(/:.*$/, "", key)
            value = text
            sub(/^[^:]*:[ ]*/, "", value)
            sub(/[ ]+#.*$/, "", value)
            value = unquote(value)
            if (key == "language") {
                if (language[entries] != "") {
                    report("matrix entry " entries " declares a language twice")
                }
                language[entries] = value
            } else if (key == "build-mode") {
                if (mode[entries] != "") {
                    report("matrix entry " entries " declares a build mode twice")
                }
                mode[entries] = value
            }
            next
        }
    }
    if (state == "past" && (text == "matrix:" || text == "include:")) {
        report("the workflow declares more than one analysis matrix")
    }
}
END {
    for (index_of_entry = 1; index_of_entry <= entries; index_of_entry++) {
        printf("%d\t%s\t%s\n", index_of_entry, language[index_of_entry], mode[index_of_entry])
    }
}
' "$workflow" > "$entries"

count=0
seen_languages=" "
go_seen=no
while IFS='	' read -r field_one field_two field_three; do
    if [ "$field_one" = "!" ]; then
        note "$field_two"
        continue
    fi
    count=$((count + 1))
    language=$field_two
    mode=$field_three
    if [ -z "$language" ]; then
        note "matrix entry $field_one declares no language"
        continue
    fi
    case "$seen_languages" in
        *" $language "*)
            note "$language is declared more than once in the analysis matrix"
            continue
            ;;
    esac
    seen_languages="$seen_languages$language "
    if [ -z "$mode" ]; then
        note "$language declares no build mode"
        continue
    fi
    if [ "$language" = "go" ]; then
        go_seen=yes
        case " $go_build_modes " in
            *" $mode "*) ;;
            *)
                note "go is on the $mode build mode, which Go does not support; it supports $go_build_modes"
                ;;
        esac
    elif [ "$mode" != "$buildless_mode" ]; then
        note "$language is on the $mode build mode where it declares $buildless_mode"
    fi
done < "$entries"

[ "$count" -gt 0 ] || note "the analysis matrix declares no language"
[ "$go_seen" = yes ] || note "the analysis matrix no longer analyses go"

if [ -n "$problems" ]; then
    printf 'CodeQL build mode check: %s\n' "$workflow" >&2
    printf '%s\n' "$problems" | sed 's/^/  /' >&2
    exit 1
fi

printf 'CodeQL build mode check: %s languages analysed in %s, each declared once, go on a build mode Go supports\n' \
    "$count" "$workflow"
