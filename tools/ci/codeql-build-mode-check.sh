#!/bin/sh
set -eu

# Reads the analysis matrix of the analyze job in the CodeQL workflow and holds
# it to the build modes each language accepts, so that a build mode the
# extractor rejects is caught here instead of in a CodeQL run. The Go extractor
# refuses the none build mode and names the modes it takes; every other analysed
# language stays buildless. The parse is scoped to jobs.analyze: an earlier job,
# a comment or an unrelated step cannot decide the result, and a construct this
# check cannot read is reported rather than passed over.

root=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
workflow=${1:-$root/.github/workflows/codeql.yml}

expected_languages="c-cpp go javascript-typescript python rust actions"
go_build_modes="autobuild manual"
buildless_mode="none"
init_action="github/codeql-action/init@"
# The two values below are GitHub Actions expressions compared literally.
# shellcheck disable=SC2016
language_expression='${{ matrix.language }}'
# shellcheck disable=SC2016
build_mode_expression='${{ matrix.build-mode }}'

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

records=$(mktemp)
trap 'rm -f "$records"' EXIT HUP INT TERM

awk -v init_action="$init_action" '
function trim(value) {
    sub(/^[ ]+/, "", value)
    sub(/[ ]+$/, "", value)
    return value
}
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
function is_mapping(text) {
    return text ~ /^[^:]+:([ ].*)?$/
}
function key_of(text,   key) {
    key = text
    sub(/:.*$/, "", key)
    return trim(key)
}
function value_of(text,   value) {
    value = text
    sub(/^[^:]*:[ ]*/, "", value)
    sub(/[ ]+#.*$/, "", value)
    return unquote(trim(value))
}
function step_key(text,   key, value) {
    if (!is_mapping(text)) {
        in_with = 0
        return
    }
    key = key_of(text)
    value = value_of(text)
    if (key == "uses") {
        uses[steps] = value
        in_with = 0
    } else if (key == "with") {
        if (value != "") {
            report("step " steps " writes its inputs in flow style, which this check does not parse")
            in_with = 0
        } else {
            in_with = 1
        }
    } else {
        in_with = 0
    }
}
function with_key(text,   key, value) {
    if (!is_mapping(text)) {
        return
    }
    key = key_of(text)
    value = value_of(text)
    if (key == "languages") {
        step_languages[steps] = value
    } else if (key == "build-mode") {
        step_mode[steps] = value
    }
}
BEGIN {
    jobs_indentation = -1
    job_indentation = -1
    analyze_indentation = -1
    strategy_indentation = -1
    matrix_indentation = -1
    include_indentation = -1
    entry_indentation = -1
    steps_indentation = -1
    step_indentation = -1
}
{
    line = $0
    sub(/\r$/, "", line)
    if (line ~ /^[ ]*(#.*)?$/) {
        next
    }
    if (line ~ /\t/) {
        report("the workflow indents with a tab, which YAML forbids")
        next
    }
    match(line, /^[ ]*/)
    indentation = RLENGTH
    text = substr(line, indentation + 1)
    sub(/[ ]+$/, "", text)
    item = 0
    content_indentation = indentation
    if (text == "-" || substr(text, 1, 2) == "- ") {
        item = 1
        match(text, /^-[ ]*/)
        content_indentation = indentation + RLENGTH
        text = substr(text, RLENGTH + 1)
    }

    if (jobs_indentation < 0) {
        if (indentation == 0 && !item && is_mapping(text) && key_of(text) == "jobs") {
            if (value_of(text) != "") {
                report("the workflow declares its jobs in flow style, which this check does not parse")
            } else {
                jobs_indentation = indentation
            }
        }
        next
    }

    if (analyze_indentation < 0) {
        if (indentation <= jobs_indentation) {
            next
        }
        if (job_indentation < 0) {
            if (item || !is_mapping(text)) {
                next
            }
            job_indentation = indentation
        }
        if (indentation != job_indentation || item || !is_mapping(text)) {
            next
        }
        if (key_of(text) == "analyze" && value_of(text) == "") {
            analyze_indentation = indentation
            analyze_seen = 1
        } else if (key_of(text) == "analyze") {
            report("the analyze job is written in flow style, which this check does not parse")
        }
        next
    }

    if (indentation <= analyze_indentation) {
        analyze_indentation = -1
        strategy_indentation = -1
        matrix_indentation = -1
        include_indentation = -1
        steps_indentation = -1
        if (!item && indentation == job_indentation && is_mapping(text) && key_of(text) == "analyze") {
            report("the workflow declares the analyze job more than once")
        }
        next
    }

    if (include_indentation >= 0 && indentation <= include_indentation) {
        include_indentation = -1
        entry_indentation = -1
    }
    if (matrix_indentation >= 0 && indentation <= matrix_indentation) {
        matrix_indentation = -1
    }
    if (strategy_indentation >= 0 && indentation <= strategy_indentation) {
        strategy_indentation = -1
    }
    if (steps_indentation >= 0 && indentation <= steps_indentation) {
        steps_indentation = -1
        step_indentation = -1
        in_with = 0
    }

    if (include_indentation >= 0) {
        if (item) {
            entries++
            entry_indentation = content_indentation
            if (text == "") {
                next
            }
            if (text ~ /^[{[]/) {
                report("matrix entry " entries " is written in flow style, which this check does not parse")
                next
            }
        } else {
            if (entries == 0) {
                report("a line precedes the first matrix entry: " text)
                next
            }
            if (indentation != entry_indentation) {
                report("matrix entry " entries " carries a line at an unexpected indentation: " text)
                next
            }
        }
        if (!is_mapping(text)) {
            report("matrix entry " entries " carries a line that is not a mapping: " text)
            next
        }
        key = key_of(text)
        value = value_of(text)
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

    if (matrix_indentation >= 0) {
        if (item || !is_mapping(text)) {
            report("the analysis matrix carries a line this check does not parse: " text)
            next
        }
        if (key_of(text) == "include") {
            if (value_of(text) != "") {
                report("the matrix include list is written in flow style, which this check does not parse")
            } else {
                include_indentation = indentation
            }
        } else {
            report("the analysis matrix declares " key_of(text) " outside its include list, which this check does not parse")
        }
        next
    }

    if (strategy_indentation >= 0) {
        if (!item && is_mapping(text) && key_of(text) == "matrix") {
            if (value_of(text) != "") {
                report("the analysis matrix is written in flow style, which this check does not parse")
            } else {
                matrix_indentation = indentation
                matrix_seen = 1
            }
        }
        next
    }

    if (steps_indentation >= 0) {
        if (item) {
            steps++
            step_indentation = content_indentation
            in_with = 0
            if (text == "") {
                next
            }
            if (text ~ /^[{[]/) {
                report("step " steps " is written in flow style, which this check does not parse")
                next
            }
            step_key(text)
            next
        }
        if (steps == 0) {
            report("a line precedes the first step of the analyze job: " text)
            next
        }
        if (indentation == step_indentation) {
            step_key(text)
        } else if (in_with && indentation > step_indentation) {
            with_key(text)
        }
        next
    }

    if (!item && is_mapping(text)) {
        key = key_of(text)
        if (key == "strategy") {
            if (value_of(text) != "") {
                report("the analyze job declares its strategy in flow style, which this check does not parse")
            } else {
                strategy_indentation = indentation
            }
        } else if (key == "steps") {
            if (value_of(text) != "") {
                report("the analyze job declares its steps in flow style, which this check does not parse")
            } else {
                steps_indentation = indentation
            }
        }
    }
}
END {
    if (jobs_indentation < 0) {
        report("the workflow declares no jobs")
    } else if (!analyze_seen) {
        report("the workflow declares no analyze job")
    } else if (!matrix_seen) {
        report("the analyze job declares no strategy matrix")
    }
    for (entry = 1; entry <= entries; entry++) {
        printf("matrix\t%d\t%s\t%s\n", entry, language[entry], mode[entry])
    }
    for (step = 1; step <= steps; step++) {
        if (index(uses[step], init_action) == 1) {
            printf("init\t%d\t%s\t%s\n", step, step_languages[step], step_mode[step])
        }
    }
}
' "$workflow" > "$records"

count=0
init_steps=0
seen_languages=" "
while IFS='	' read -r record first second third; do
    case "$record" in
        '!')
            note "$first"
            ;;
        matrix)
            count=$((count + 1))
            if [ -z "$second" ]; then
                note "matrix entry $first declares no language"
                continue
            fi
            case "$seen_languages" in
                *" $second "*)
                    note "$second is declared more than once in the analysis matrix"
                    continue
                    ;;
            esac
            seen_languages="$seen_languages$second "
            case " $expected_languages " in
                *" $second "*) ;;
                *)
                    note "$second is analysed but is outside the set this check knows; extend expected_languages in this script with its supported build modes"
                    ;;
            esac
            if [ -z "$third" ]; then
                note "$second declares no build mode"
                continue
            fi
            if [ "$second" = go ]; then
                case " $go_build_modes " in
                    *" $third "*) ;;
                    *)
                        note "go is on the $third build mode, which Go does not support; it supports $go_build_modes"
                        ;;
                esac
            elif [ "$third" != "$buildless_mode" ]; then
                note "$second is on the $third build mode where it declares $buildless_mode"
            fi
            ;;
        init)
            init_steps=$((init_steps + 1))
            if [ "$second" != "$language_expression" ]; then
                note "the CodeQL initialisation step of the analyze job reads languages as '$second' instead of $language_expression"
            fi
            if [ "$third" != "$build_mode_expression" ]; then
                note "the CodeQL initialisation step of the analyze job reads build-mode as '$third' instead of $build_mode_expression"
            fi
            ;;
        *)
            note "the matrix parser emitted a record this check does not know: $record"
            ;;
    esac
done < "$records"

[ "$count" -gt 0 ] || note "the analysis matrix declares no language"
for expected_language in $expected_languages; do
    case "$seen_languages" in
        *" $expected_language "*) ;;
        *)
            note "$expected_language is no longer analysed; the matrix declares $expected_languages"
            ;;
    esac
done
[ "$init_steps" -gt 0 ] || note "the analyze job runs no ${init_action%@} step"

if [ -n "$problems" ]; then
    printf 'CodeQL build mode check: %s\n' "$workflow" >&2
    printf '%s\n' "$problems" | sed 's/^/  /' >&2
    exit 1
fi

printf 'CodeQL build mode check: %s languages analysed once each in the analyze job of %s, go on a build mode Go supports, %s initialisation step bound to the matrix\n' \
    "$count" "$workflow" "$init_steps"
