#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
usage: tools/ci/beta-ledger-check.sh [--ledger PATH] [--spec PATH] [--revisions]

Validates every record of the LayerX beta executed-evidence ledger
(spec/layerx-beta/qualification.kvx by default) and prints the set of distinct
gate revisions. Every violation is listed on stderr; the exit status is 1 when
at least one violation exists, 2 on usage or environment errors, 0 otherwise.

  --ledger PATH   ledger to check (default spec/layerx-beta/qualification.kvx)
  --spec PATH     feature spec used to resolve tasks and acceptance criteria
                  (default spec/layerx-beta/spec.kvx)
  --revisions     print only the distinct gate revisions, one per line

Parsing rules (mirroring spec/specgen/kvx.go):
  * a line whose trimmed text is [name] opens a section; every later key = value
    line belongs to it; a section name or a key repeated inside one section is
    a violation
  * comments start at a # outside double quotes and run to end of line
  * a value is a double-quoted string ("..." with \" and \\ escapes), a list
    ([...] split on commas outside quotes, each item a quoted string) or a bare
    scalar; a value containing ${...} is a violation because ledger values must
    be literal
  * a section is either gate.<task>.<n> or observation.<task>.<n>; any other
    section name is a violation

Gate records ([gate.<task>.<n>]) must satisfy:
  * keys task, reqs, revision, command, environment, started_at, outcome,
    evidence and note are all present and no other key exists
  * task equals the <task> part of the section name and [task.<task>] exists
    in the feature spec
  * reqs is a non-empty list of <req>.<ac> pairs, each resolving to key ac_<ac>
    under [req.<req>] in the feature spec
  * revision is a 40-hex commit that exists in this repository
    (git cat-file -e <revision>^{commit})
  * command, after any leading VAR=value words, is either
      - make <targets...>: every word that is not an option or VAR=value must
        be a target defined as "<target>:" (multi-target lines are split on
        whitespace) in Makefile or in any file it includes through a literal
        include/-include/sinclude line, transitively; -C, -f, -I, -o and -W are
        not supported
      - sh|bash|python3|python|node <script> ...: the first non-option word
        after the interpreter must be an existing file in the tree
      - <path> ...: a relative path to an executable regular file in the tree
  * environment is a non-empty string
  * started_at matches YYYY-MM-DDTHH:MM:SSZ
  * outcome is pass, fail or blocked; a blocked gate carries a non-empty note
  * evidence is a relative path without .. segments that exists in the tree
  * every file named status.json or report.json that is the evidence path or
    lies below an evidence directory is a qualification runner document: a
    JSON object whose schema is layerx-qualification-status-v1 or
    layerx-qualification-report-v1 respectively, whose source_revision equals
    the record revision and whose source_identity equals the clean-tree
    identity sha256(<revision> || 0x00) that
    tools/qualification/release_runner.py computes for a tree with no tracked
    changes and no untracked files

Observation records ([observation.<task>.<n>]) must satisfy:
  * keys task, file, symbol, observed, assumption and severity are present;
    the <task> part of the section name is the task that recorded the
    observation and task (<n> or <n>.<m>) names the task it concerns;
    severity is blocker, suspect, assumption or note
  * no gate key (reqs, revision, command, environment, started_at, outcome,
    evidence, note) and no other unknown key is present
EOF
}

KVX_PARSER='
function trim(s) { sub(/^[ \t]+/, "", s); sub(/[ \t]+$/, "", s); return s }
function strip_comment(s,   i, n, c, inq, out) {
    n = length(s); inq = 0; out = ""
    for (i = 1; i <= n; i++) {
        c = substr(s, i, 1)
        if (inq && c == "\\") { out = out c substr(s, i + 1, 1); i++; continue }
        if (c == "\"") inq = !inq
        else if (c == "#" && !inq) break
        out = out c
    }
    if (inq) return "\001"
    return out
}
function unquote(s,   i, n, c, out) {
    n = length(s); out = ""
    for (i = 2; i < n; i++) {
        c = substr(s, i, 1)
        if (c == "\\") { i++; out = out substr(s, i, 1); continue }
        out = out c
    }
    return out
}
function emit(kind, key, vtype, val) {
    printf "%s\036%s\036%s\036%s\036%s\036%d\n", kind, section, key, vtype, val, NR
}
function split_list(s,   i, n, c, inq, item, out, count) {
    n = length(s); inq = 0; item = ""; out = ""; count = 0
    for (i = 2; i < n; i++) {
        c = substr(s, i, 1)
        if (inq && c == "\\") { item = item c substr(s, i + 1, 1); i++; continue }
        if (c == "\"") { inq = !inq; item = item c; continue }
        if (c == "," && !inq) {
            item = trim(item)
            if (item !~ /^".*"$/) return "\001"
            out = out (count ? "\037" : "") unquote(item); count++; item = ""
            continue
        }
        item = item c
    }
    item = trim(item)
    if (item != "") {
        if (item !~ /^".*"$/) return "\001"
        out = out (count ? "\037" : "") unquote(item); count++
    }
    return "\002" out
}
BEGIN { section = "" }
{
    line = $0
    sub(/\r$/, "", line)
    stripped = strip_comment(line)
    if (stripped == "\001") { emit("error", "", "", "unterminated double-quoted string"); next }
    stripped = trim(stripped)
    if (stripped == "") next
    if (stripped ~ /^\[.*\]$/) {
        section = substr(stripped, 2, length(stripped) - 2)
        emit("section", "", "", "")
        next
    }
    eq = index(stripped, "=")
    if (eq == 0) { emit("error", "", "", "line is neither a section header nor key = value"); next }
    key = trim(substr(stripped, 1, eq - 1))
    val = trim(substr(stripped, eq + 1))
    if (key !~ /^[A-Za-z_][A-Za-z0-9_.-]*$/) { emit("error", key, "", "invalid key"); next }
    if (index(val, "${") > 0) { emit("error", key, "", "value contains ${...} interpolation; ledger values must be literal"); next }
    if (val ~ /^".*"$/) { emit("pair", key, "string", unquote(val)); next }
    if (val ~ /^\[.*\]$/) {
        items = split_list(val)
        if (items == "\001") { emit("error", key, "", "list items must be double-quoted strings"); next }
        emit("pair", key, "list", substr(items, 2))
        next
    }
    emit("pair", key, "scalar", val)
}
'

beta_ledger_check() {
    local root ledger="" spec="" revisions_only=0
    root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
    while [ "$#" -gt 0 ]; do
        case $1 in
        --ledger)
            [ "$#" -ge 2 ] || { usage >&2; return 2; }
            ledger=$2
            shift 2
            ;;
        --spec)
            [ "$#" -ge 2 ] || { usage >&2; return 2; }
            spec=$2
            shift 2
            ;;
        --revisions)
            revisions_only=1
            shift
            ;;
        -h | --help)
            usage
            return 0
            ;;
        *)
            usage >&2
            return 2
            ;;
        esac
    done
    ledger=${ledger:-$root/spec/layerx-beta/qualification.kvx}
    spec=${spec:-$root/spec/layerx-beta/spec.kvx}
    [ -f "$ledger" ] || { echo "beta-ledger-check: ledger not found: $ledger" >&2; return 2; }
    [ -f "$spec" ] || { echo "beta-ledger-check: feature spec not found: $spec" >&2; return 2; }
    command -v python3 >/dev/null 2>&1 || { echo "beta-ledger-check: python3 is required" >&2; return 2; }
    git -C "$root" rev-parse --git-dir >/dev/null 2>&1 || { echo "beta-ledger-check: $root is not a git repository" >&2; return 2; }

    local -a violations=() order=()
    local -A value=() vtype=() lineno=() section_line=() section_keys=()
    local -A spec_sections=() spec_keys=()
    local -A make_targets=() makefiles_seen=()
    local -A revisions=()
    local kind section key type val ln
    local gates=0 observations=0

    while IFS=$'\036' read -r kind section key type val ln; do
        case $kind in
        error)
            violations+=("$ledger:$ln: $val")
            ;;
        section)
            if [ -n "${section_line[$section]+x}" ]; then
                violations+=("$ledger:$ln: duplicate section [$section] (first at line ${section_line[$section]})")
            else
                section_line[$section]=$ln
                section_keys[$section]=""
                order+=("$section")
            fi
            ;;
        pair)
            if [ -z "$section" ]; then
                violations+=("$ledger:$ln: key '$key' appears before any section")
                continue
            fi
            if [ -n "${value[$section$'\037'$key]+x}" ]; then
                violations+=("$ledger:$ln: duplicate key '$key' in [$section]")
                continue
            fi
            value[$section$'\037'$key]=$val
            vtype[$section$'\037'$key]=$type
            lineno[$section$'\037'$key]=$ln
            section_keys[$section]="${section_keys[$section]} $key"
            ;;
        esac
    done < <(awk "$KVX_PARSER" "$ledger")

    while IFS=$'\036' read -r kind section key type val ln; do
        case $kind in
        section) spec_sections[$section]=1 ;;
        pair) spec_keys[$section$'\037'$key]=1 ;;
        esac
    done < <(awk "$KVX_PARSER" "$spec")

    collect_makefile() {
        local file=$1 include
        [ -n "${makefiles_seen[$file]+x}" ] && return 0
        makefiles_seen[$file]=1
        [ -f "$root/$file" ] || return 0
        local target line
        while IFS= read -r line; do
            for target in ${line%%:*}; do
                make_targets[$target]=1
            done
        done < <(grep -E '^[^[:space:]#=:$][^=:#]*:([^=]|$)' "$root/$file" || true)
        while IFS= read -r include; do
            collect_makefile "$include"
        done < <(sed -n -E 's/^-?s?include[[:space:]]+([^[:space:]$]+)[[:space:]]*$/\1/p' "$root/$file")
    }
    collect_makefile Makefile

    in_tree_path() {
        case $1 in
        /* | "" | ../* | */../* | */.. | ..) return 1 ;;
        esac
        return 0
    }

    command_violation() {
        local command=$1 word w script=""
        local -a words=()
        read -r -a words <<<"$command"
        while [ "${#words[@]}" -gt 0 ] && [[ ${words[0]} =~ ^[A-Za-z_][A-Za-z0-9_]*= ]]; do
            words=("${words[@]:1}")
        done
        [ "${#words[@]}" -gt 0 ] || { echo "command names no executable"; return 0; }
        word=${words[0]}
        case $word in
        make)
            local found=0
            for w in "${words[@]:1}"; do
                case $w in
                -C | -f | -I | -o | -W) echo "make option $w is not supported; only the top-level Makefile and its includes are recognised"; return 0 ;;
                -*) continue ;;
                *=*) continue ;;
                esac
                found=1
                [ -n "${make_targets[$w]+x}" ] || echo "make target '$w' is not defined in Makefile or an included file"
            done
            [ "$found" -eq 1 ] || echo "make command names no target"
            ;;
        sh | bash | python3 | python | node)
            for w in "${words[@]:1}"; do
                case $w in -*) continue ;; esac
                script=$w
                break
            done
            [ -n "$script" ] || { echo "interpreter command names no script"; return 0; }
            in_tree_path "$script" && [ -f "$root/$script" ] || echo "script '$script' is not a file in the tree"
            ;;
        *)
            in_tree_path "$word" && [ -f "$root/$word" ] && [ -x "$root/$word" ] || echo "'$word' is not an executable file in the tree"
            ;;
        esac
    }

    runner_document_violations() {
        local file=$1 revision=$2
        python3 - "$file" "$revision" "$root" <<'PY'
import hashlib
import json
import os
import sys

path, revision, root = sys.argv[1:4]
shown = os.path.relpath(path, root)
expected = {
    "status.json": "layerx-qualification-status-v1",
    "report.json": "layerx-qualification-report-v1",
}[os.path.basename(path)]
try:
    with open(path, "rb") as handle:
        document = json.load(handle)
except (OSError, ValueError) as error:
    print(f"{shown}: not a JSON document ({error})")
    sys.exit(0)
if not isinstance(document, dict):
    print(f"{shown}: not a JSON object")
    sys.exit(0)
schema = document.get("schema")
if schema != expected:
    print(f"{shown}: schema {schema!r} is not {expected!r}")
source_revision = document.get("source_revision")
if source_revision != revision:
    print(f"{shown}: source_revision {source_revision!r} differs from the record revision {revision}")
identity = hashlib.sha256(revision.encode("ascii") + b"\0").hexdigest()
source_identity = document.get("source_identity")
if source_identity != identity:
    print(f"{shown}: source_identity {source_identity!r} is not the clean-tree identity {identity} for revision {revision}")
PY
    }

    local gate_keys="task reqs revision command environment started_at outcome evidence note"
    local observation_keys="task file symbol observed assumption severity"
    local record_task task_part req present line_ref
    for section in "${order[@]}"; do
        line_ref="$ledger:${section_line[$section]}"
        if [[ $section =~ ^gate\.([0-9]+(\.[0-9]+)?)\.([0-9]+)$ ]]; then
            gates=$((gates + 1))
            task_part=${BASH_REMATCH[1]}
            for key in $gate_keys; do
                [ -n "${value[$section$'\037'$key]+x}" ] || violations+=("$line_ref: [$section] lacks key '$key'")
            done
            for key in ${section_keys[$section]}; do
                case " $gate_keys " in
                *" $key "*) ;;
                *) violations+=("$line_ref: [$section] carries unknown key '$key'") ;;
                esac
            done
            record_task=${value[$section$'\037'task]-}
            if [ -n "${value[$section$'\037'task]+x}" ]; then
                [ "$record_task" = "$task_part" ] || violations+=("$line_ref: [$section] task '$record_task' differs from the section task '$task_part'")
                [ -n "${spec_sections[task.$record_task]+x}" ] || violations+=("$line_ref: [$section] task '$record_task' is not a [task.*] section of $spec")
            fi
            if [ -n "${value[$section$'\037'reqs]+x}" ]; then
                if [ "${vtype[$section$'\037'reqs]}" != list ] || [ -z "${value[$section$'\037'reqs]}" ]; then
                    violations+=("$line_ref: [$section] reqs must be a non-empty list")
                else
                    while IFS= read -r -d $'\037' req || [ -n "$req" ]; do
                        if [[ $req =~ ^([0-9]+)\.([0-9]+)$ ]]; then
                            [ -n "${spec_keys[req.${BASH_REMATCH[1]}$'\037'ac_${BASH_REMATCH[2]}]+x}" ] || violations+=("$line_ref: [$section] req '$req' does not resolve to ac_${BASH_REMATCH[2]} under [req.${BASH_REMATCH[1]}] in $spec")
                        else
                            violations+=("$line_ref: [$section] req '$req' is not of the form <req>.<ac>")
                        fi
                    done < <(printf '%s\037' "${value[$section$'\037'reqs]}")
                fi
            fi
            if [ -n "${value[$section$'\037'revision]+x}" ]; then
                val=${value[$section$'\037'revision]}
                if [[ $val =~ ^[0-9a-f]{40}$ ]] && git -C "$root" cat-file -e "$val^{commit}" 2>/dev/null; then
                    revisions[$val]=1
                else
                    violations+=("$line_ref: [$section] revision '$val' is not a commit in this repository")
                fi
            fi
            if [ -n "${value[$section$'\037'command]+x}" ]; then
                while IFS= read -r val; do
                    [ -z "$val" ] || violations+=("$line_ref: [$section] command '${value[$section$'\037'command]}': $val")
                done < <(command_violation "${value[$section$'\037'command]}")
            fi
            if [ -n "${value[$section$'\037'environment]+x}" ] && [ -z "${value[$section$'\037'environment]}" ]; then
                violations+=("$line_ref: [$section] environment is empty")
            fi
            if [ -n "${value[$section$'\037'started_at]+x}" ] && ! [[ ${value[$section$'\037'started_at]} =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]]; then
                violations+=("$line_ref: [$section] started_at '${value[$section$'\037'started_at]}' is not YYYY-MM-DDTHH:MM:SSZ")
            fi
            if [ -n "${value[$section$'\037'outcome]+x}" ]; then
                case ${value[$section$'\037'outcome]} in
                pass | fail) ;;
                blocked)
                    [ -n "${value[$section$'\037'note]-}" ] || violations+=("$line_ref: [$section] is blocked without a note naming the owner action")
                    ;;
                *) violations+=("$line_ref: [$section] outcome '${value[$section$'\037'outcome]}' is not pass, fail or blocked") ;;
                esac
            fi
            if [ -n "${value[$section$'\037'evidence]+x}" ]; then
                val=${value[$section$'\037'evidence]}
                if ! in_tree_path "$val" || [ ! -e "$root/$val" ]; then
                    violations+=("$line_ref: [$section] evidence '$val' does not exist in the tree")
                elif [ -n "${value[$section$'\037'revision]+x}" ] && [[ ${value[$section$'\037'revision]} =~ ^[0-9a-f]{40}$ ]]; then
                    local document
                    while IFS= read -r document; do
                        while IFS= read -r line; do
                            [ -z "$line" ] || violations+=("$line_ref: [$section] evidence $line")
                        done < <(runner_document_violations "$document" "${value[$section$'\037'revision]}")
                    done < <(
                        if [ -d "$root/$val" ]; then
                            find "$root/$val" -type f \( -name status.json -o -name report.json \) | sort
                        else
                            case ${val##*/} in status.json | report.json) printf '%s\n' "$root/$val" ;; esac
                        fi
                    )
                fi
            fi
        elif [[ $section =~ ^observation\.([0-9]+(\.[0-9]+)?)\.([0-9]+)$ ]]; then
            observations=$((observations + 1))
            task_part=${BASH_REMATCH[1]}
            for key in $observation_keys; do
                [ -n "${value[$section$'\037'$key]+x}" ] || violations+=("$line_ref: [$section] lacks key '$key'")
            done
            for key in ${section_keys[$section]}; do
                case " $observation_keys " in
                *" $key "*) continue ;;
                esac
                case " reqs revision command environment started_at outcome evidence note " in
                *" $key "*) violations+=("$line_ref: [$section] carries gate key '$key'") ;;
                *) violations+=("$line_ref: [$section] carries unknown key '$key'") ;;
                esac
            done
            if [ -n "${value[$section$'\037'task]+x}" ] && ! [[ ${value[$section$'\037'task]} =~ ^[0-9]+(\.[0-9]+)?$ ]]; then
                violations+=("$line_ref: [$section] task '${value[$section$'\037'task]}' is not a task identifier")
            fi
            if [ -n "${value[$section$'\037'severity]+x}" ]; then
                case ${value[$section$'\037'severity]} in
                blocker | suspect | assumption | note) ;;
                *) violations+=("$line_ref: [$section] severity '${value[$section$'\037'severity]}' is not blocker, suspect, assumption or note") ;;
                esac
            fi
        else
            violations+=("$line_ref: [$section] is neither gate.<task>.<n> nor observation.<task>.<n>")
        fi
    done

    local -a distinct=()
    if [ "${#revisions[@]}" -gt 0 ]; then
        while IFS= read -r val; do distinct+=("$val"); done < <(printf '%s\n' "${!revisions[@]}" | sort)
    fi

    if [ "${#violations[@]}" -gt 0 ]; then
        printf 'beta-ledger-check: %d violation(s)\n' "${#violations[@]}" >&2
        printf '  %s\n' "${violations[@]}" >&2
        return 1
    fi
    if [ "$revisions_only" -eq 1 ]; then
        [ "${#distinct[@]}" -eq 0 ] || printf '%s\n' "${distinct[@]}"
        return 0
    fi
    printf 'beta-ledger-check: %d gate record(s), %d observation record(s) in %s\n' "$gates" "$observations" "${ledger#"$root"/}"
    printf 'beta-ledger-check: distinct gate revisions (%d):\n' "${#distinct[@]}"
    [ "${#distinct[@]}" -eq 0 ] || printf '  %s\n' "${distinct[@]}"
    return 0
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    beta_ledger_check "$@"
fi
