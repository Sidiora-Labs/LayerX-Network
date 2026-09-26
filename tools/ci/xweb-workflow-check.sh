#!/usr/bin/env bash
# Offline check of .github/workflows/xweb-test.yml: the workflow triggers on
# pull requests and pushes to main; it carries one concurrency group per ref with
# cancel-in-progress; its path filters are exactly the trees the paxeer-x-web
# feature touches and the trees its legs exercise, each naming a path this
# repository holds or a path a task of spec/paxeer-x-web/spec.kvx adds; its five jobs are the five legs, each named
# for the tools it runs, and together they run exactly the commands of the
# feature's gate task verify_cmd, each from the repository root with a tool the
# same job installs first at a version this repository already pins; every
# action is pinned by commit to a pin another workflow in this repository uses;
# and no continue-on-error or conditional step is present. The check then
# mutates the workflow and requires each mutation to be refused, so the
# assertions are known to bite. No network and no runner are involved.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../.." && pwd)
WORKFLOW="$REPO_ROOT/.github/workflows/xweb-test.yml"

fail() { printf 'xweb-workflow-check: error: %s\n' "$*" >&2; exit 1; }

command -v python3 >/dev/null 2>&1 \
    || fail "python3 is required to parse the workflow"
python3 -c 'import sys, tomllib, yaml; sys.exit(0)' >/dev/null 2>&1 \
    || fail "python3 3.11 or later with the yaml module is required to parse the workflow and the spec"
[ -f "$WORKFLOW" ] \
    || fail "$WORKFLOW is missing"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
chmod 0700 "$WORK"

cat > "$WORK/check.py" <<'PYCHECK'
"""Assert that .github/workflows/xweb-test.yml says what it runs.

Exit 0 when every assertion holds, 2 when any is refused. Every refusal names
the offending value.
"""

import glob
import json
import os
import re
import shlex
import sys
import tomllib

import yaml

WORKFLOW_PATH = ".github/workflows/xweb-test.yml"
SPEC_PATH = "spec/paxeer-x-web/spec.kvx"
GATE_TASK = ("3", "1")

FILTERS = [
    "interop/crates/x-websearch/**",
    "interop/deploy/x-websearch/**",
    "interop/crates/layerx-x402/src/**",
    "interop/Cargo.toml",
    "interop/Cargo.lock",
    "modules/xweb/**",
    "modules/layerxbridge/**",
    "precompiles/xweb/**",
    "contracts/src/precompiles/IXWeb.sol",
    "contracts/src/xweb/**",
    "contracts/test/XWebConsumer.t.sol",
    "contracts/test/CW1155ERC1155PointerTest.t.sol",
    "contracts/test/CW721ERC721PointerTest.t.sol",
    "Makefile",
    "include/layerx/lx_web.h",
    "include/layerx/lx_batch.h",
    "include/layerx/programs.h",
    "src/modules/web/**",
    "src/modules/programs/call.c",
    "src/network/lx_web_adapter.c",
    "src/sequencer/lx_web_root.c",
    "tests/modules/**",
    "tests/sequencer/**",
    "tests/network/**",
    "tests/fixtures/web/**",
    "tests/test_programs_web_read.c",
    "tests/test_web_program_path.c",
    "programs/crates/layerx-programs-runtime/**",
    "programs/sdk/rust/**",
    "agent/sdk/typescript/src/web-search.ts",
    "agent/sdk/typescript/test/**",
    "agent/sdk/python/layerx_sdk/web_search.py",
    "agent/sdk/python/tests/**",
    "agent/crates/**",
    "tools/ci/xweb-forge-libs.sh",
    WORKFLOW_PATH,
]
TRIGGERS = ["pull_request", "push"]
PUSH_BRANCHES = ["main"]
CONCURRENCY_PREFIX = "xweb-test-"
REF_EXPRESSION = "${{ github.ref }}"

# The five legs: job id -> the job name, the tool phrases the name carries and
# the positions, in the gate task's verify_cmd, of the commands the job runs.
LEGS = {
    "sidecar-crate": {
        "name": "cargo test for the x-websearch crate",
        "tools": ["cargo test"],
        "gate": [0],
    },
    "module-precompile": {
        "name": "go test for modules/xweb and precompiles/xweb",
        "tools": ["go test"],
        "gate": [1],
    },
    "consumer-contract": {
        "name": "forge test for contracts/test/XWebConsumer.t.sol",
        "tools": ["forge test"],
        "gate": [2, 3],
    },
    "kernel-web": {
        "name": (
            "make for the kernel web targets and cargo test for the runtime "
            "web_read test"
        ),
        "tools": ["make", "cargo test"],
        "gate": [4, 5],
    },
    "agent-clients": {
        "name": (
            "cargo test, npm and pytest for the MCP, TypeScript and Python web "
            "clients"
        ),
        "tools": ["cargo test", "npm", "pytest"],
        "gate": [6, 7, 8],
    },
}

# "go test" is a substring of "cargo test", so a phrase matches only where it
# begins a word: a job named for one tool never reads as a claim about another.
TOOL_PHRASES = {
    phrase: re.compile(r"(?<![\w./-])%s\b" % re.escape(phrase))
    for phrase in ("cargo test", "go test", "forge test", "make", "npm", "pytest")
}

KERNEL_PACKAGES = {"build-essential", "clang", "libssl-dev", "libsqlite3-dev", "ripgrep"}
SIDECAR_PACKAGES = {"pkg-config", "libssl-dev"}
WASM_TARGET = "wasm32-unknown-unknown"

ALLOWED_PROGRAMS = {"bash", "cargo", "forge", "go", "make", "npm", "node", "python3"}

PINNED_USES = re.compile(r"[\w.-]+/[\w./-]+@[0-9a-f]{40}\Z")
USES_VALUE = re.compile(r"\buses:\s*(\S+)")
EXACT_VERSION = re.compile(r"v?[0-9]+\.[0-9]+\.[0-9]+\Z")
IPV4 = re.compile(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b")
DATE = re.compile(r"\b20\d{2}-\d{2}-\d{2}\b")
GATE_FORM = re.compile(r"\Atimeout 20m sh -c '(.*)'\Z", re.S)
SUBSHELL = re.compile(r"\A\(cd ([A-Za-z0-9._/-]+) && (.*)\)\Z", re.S)

REFUSALS = []
CHECKED = []


def refuse(message):
    REFUSALS.append(message)


def note(message):
    CHECKED.append(message)


def read(path):
    with open(path, "r", encoding="utf-8") as handle:
        return handle.read()


def command_lines(run):
    lines = []
    for line in str(run).splitlines():
        stripped = line.strip()
        if stripped and not stripped.startswith("#"):
            lines.append(stripped)
    return lines


def split_top_level(text):
    """Split a shell list on && outside parentheses."""
    parts, depth, current, index = [], 0, [], 0
    while index < len(text):
        char = text[index]
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
        if depth == 0 and text.startswith(" && ", index):
            parts.append("".join(current).strip())
            current = []
            index += 4
            continue
        current.append(char)
        index += 1
    parts.append("".join(current).strip())
    return parts


def load_spec(root):
    try:
        return tomllib.loads(read(os.path.join(root, SPEC_PATH)))
    except (OSError, tomllib.TOMLDecodeError) as error:
        refuse("%s cannot be read as the feature spec: %s" % (SPEC_PATH, error))
        return {}


def feature_tasks(spec):
    tasks = []
    for wave in (spec.get("task") or {}).values():
        if not isinstance(wave, dict):
            continue
        for task in wave.values():
            if isinstance(task, dict) and "verify_cmd" in task:
                tasks.append(task)
    return tasks


def declared_paths(spec):
    paths = set()
    for task in feature_tasks(spec):
        for key in ("touches", "tests"):
            for path in task.get(key) or []:
                paths.add(str(path).rstrip("/"))
    return paths


def is_declared(declared, path):
    for entry in declared:
        if entry == path or entry.startswith(path + "/") or path.startswith(entry + "/"):
            return True
    return False


def present_or_declared(root, declared, path, what):
    if os.path.exists(os.path.join(root, path)):
        note("the %s %s is present in this repository" % (what, path))
    elif is_declared(declared, path):
        note("the %s %s is added by a task of %s" % (what, path, SPEC_PATH))
    else:
        refuse(
            "the %s %s is neither present nor added by a task of %s"
            % (what, path, SPEC_PATH)
        )


def gate_commands(spec):
    wave, task = GATE_TASK
    block = ((spec.get("task") or {}).get(wave) or {}).get(task) or {}
    verify = block.get("verify_cmd")
    if not isinstance(verify, str):
        refuse("the gate task %s.%s carries no verify_cmd" % GATE_TASK)
        return []
    match = GATE_FORM.match(verify)
    if not match:
        refuse(
            "the gate task verify_cmd %r is not the one timeout 20m sh -c form"
            % verify
        )
        return []
    commands = split_top_level(match.group(1))
    note("the gate task %s.%s runs %d commands" % (GATE_TASK + (len(commands),)))
    return commands


def run_steps(job):
    steps = []
    for index, step in enumerate(job.get("steps") or []):
        if isinstance(step, dict) and "run" in step and str(step.get("name", "")).startswith("Run "):
            steps.append((index, step))
    return steps


def leg_commands(job):
    commands = []
    for _, step in run_steps(job):
        commands.extend(command_lines(step["run"]))
    return commands


def install_lines(job):
    lines = []
    for step in job.get("steps") or []:
        if isinstance(step, dict) and "run" in step and not str(step.get("name", "")).startswith("Run "):
            lines.extend(command_lines(step["run"]))
    return lines


def check_triggers(document):
    triggers = document.get("on", document.get(True))
    if not isinstance(triggers, dict):
        refuse("the workflow carries no trigger mapping")
        return None
    if sorted(triggers) != sorted(TRIGGERS):
        refuse(
            "the triggers are %s, not the pull request and push triggers %s"
            % (sorted(triggers), sorted(TRIGGERS))
        )
    pull_request = triggers.get("pull_request")
    if not isinstance(pull_request, dict) or sorted(pull_request) != ["paths"]:
        refuse("the pull_request trigger must carry paths and nothing else")
    push = triggers.get("push")
    if not isinstance(push, dict) or sorted(push) != ["branches", "paths"]:
        refuse("the push trigger must carry branches and paths and nothing else")
    elif push["branches"] != PUSH_BRANCHES:
        refuse("the push branches are %s, not %s" % (push["branches"], PUSH_BRANCHES))
    else:
        note("the workflow runs on pull requests and on pushes to main")
    return triggers


def check_concurrency(document):
    concurrency = document.get("concurrency")
    if not isinstance(concurrency, dict):
        refuse("the workflow carries no concurrency group")
        return
    group = str(concurrency.get("group", ""))
    if group != CONCURRENCY_PREFIX + REF_EXPRESSION:
        refuse(
            "the concurrency group is %r, not one group per ref %r"
            % (group, CONCURRENCY_PREFIX + REF_EXPRESSION)
        )
    else:
        note("the concurrency group %s is one group per ref" % group)
    if concurrency.get("cancel-in-progress") is not True:
        refuse(
            "the concurrency group carries cancel-in-progress %r, not true"
            % (concurrency.get("cancel-in-progress"),)
        )
    else:
        note("the concurrency group cancels the run in progress")
    for job_id, job in (document.get("jobs") or {}).items():
        if isinstance(job, dict) and "concurrency" in job:
            refuse("the job %s overrides the workflow concurrency group" % job_id)


def check_filters(root, declared, triggers):
    if not isinstance(triggers, dict):
        return
    filter_sets = {}
    for trigger in TRIGGERS:
        block = triggers.get(trigger)
        if isinstance(block, dict) and "paths" in block:
            filter_sets[trigger] = block["paths"]
        if isinstance(block, dict) and "paths-ignore" in block:
            refuse("the %s trigger carries a paths-ignore filter" % trigger)
    for trigger in TRIGGERS:
        if trigger not in filter_sets:
            refuse("the %s trigger carries no path filter" % trigger)
    for trigger, paths in filter_sets.items():
        if paths != FILTERS:
            refuse(
                "the %s path filters are %s, not exactly %s"
                % (trigger, paths, FILTERS)
            )
    if len(filter_sets) == 2 and len({tuple(paths) for paths in filter_sets.values()}) != 1:
        refuse("the pull request and push path filters differ")
    seen = set()
    for paths in filter_sets.values():
        for entry in paths:
            if entry in seen:
                continue
            seen.add(entry)
            if entry.startswith("!"):
                refuse("the path filter %s is a negation" % entry)
                continue
            if "*" in entry and not entry.endswith("/**"):
                refuse("the path filter %s is neither a directory glob nor a file" % entry)
                continue
            if entry.endswith("/**"):
                directory = entry[: -len("/**")]
                if "*" in directory:
                    refuse("the path filter %s globs more than one directory" % entry)
                elif os.path.isdir(os.path.join(root, directory)):
                    note("the path filter %s names the directory %s" % (entry, directory))
                elif is_declared(declared, directory):
                    note(
                        "the path filter %s names the directory %s a task of %s adds"
                        % (entry, directory, SPEC_PATH)
                    )
                else:
                    refuse(
                        "the path filter %s names no directory: %s is absent and "
                        "no task of %s adds it" % (entry, directory, SPEC_PATH)
                    )
            elif os.path.isfile(os.path.join(root, entry)):
                note("the path filter %s names a file this repository holds" % entry)
            elif is_declared(declared, entry):
                note(
                    "the path filter %s names a file a task of %s adds"
                    % (entry, SPEC_PATH)
                )
            else:
                refuse(
                    "the path filter %s names no file: it is absent and no task "
                    "of %s adds it" % (entry, SPEC_PATH)
                )


def check_jobs_are_the_legs(jobs, gate):
    if sorted(jobs) != sorted(LEGS):
        missing = sorted(set(LEGS) - set(jobs))
        extra = sorted(set(jobs) - set(LEGS))
        refuse(
            "the jobs are %s, not the five legs %s (missing %s, unexpected %s)"
            % (sorted(jobs), sorted(LEGS), missing, extra)
        )
    positions = sorted(index for leg in LEGS.values() for index in leg["gate"])
    if gate and positions != list(range(len(gate))):
        refuse(
            "the legs cover the gate commands %s, not each of the %d commands "
            "of the gate task exactly once" % (positions, len(gate))
        )
    for job_id, leg in LEGS.items():
        job = jobs.get(job_id)
        if not isinstance(job, dict):
            continue
        if job.get("name") != leg["name"]:
            refuse(
                "the job %s is named %r, not %r" % (job_id, job.get("name"), leg["name"])
            )
        else:
            note("the job %s is named %r" % (job_id, leg["name"]))
        if not gate:
            continue
        expected = [gate[index] for index in leg["gate"] if index < len(gate)]
        actual = leg_commands(job)
        if actual != expected:
            refuse(
                "the job %s runs %s, not the gate commands %s"
                % (job_id, actual, expected)
            )
        else:
            note("the job %s runs exactly the gate commands %s" % (job_id, expected))
    union = [command for job in jobs.values() if isinstance(job, dict) for command in leg_commands(job)]
    if gate and sorted(union) != sorted(gate):
        refuse(
            "the legs together run %s, not the gate task's commands %s"
            % (sorted(union), sorted(gate))
        )


def check_names_claim_only_what_runs(jobs):
    for job_id, job in jobs.items():
        if not isinstance(job, dict):
            continue
        name = str(job.get("name", ""))
        commands = " ".join(leg_commands(job))
        expected = set(LEGS.get(job_id, {}).get("tools", []))
        for phrase, pattern in TOOL_PHRASES.items():
            claimed = bool(pattern.search(name))
            run = bool(pattern.search(commands))
            if claimed and not run:
                refuse("the job %s is named for %s but runs no such command" % (job_id, phrase))
            if run and not claimed:
                refuse("the job %s runs %s but its name does not say so" % (job_id, phrase))
            if claimed and run:
                note("the job %s names and runs %s" % (job_id, phrase))
            if claimed != (phrase in expected):
                refuse(
                    "the job %s %s %s, which its leg %s"
                    % (
                        job_id,
                        "claims" if claimed else "does not claim",
                        phrase,
                        "does not run" if claimed else "runs",
                    )
                )
        for _, step in run_steps(job):
            step_name = str(step.get("name", ""))
            text = " ".join(command_lines(step["run"]))
            for phrase, pattern in TOOL_PHRASES.items():
                if pattern.search(text) and not pattern.search(step_name):
                    refuse(
                        "the step %r of the job %s runs %s but its name does not say so"
                        % (step_name, job_id, phrase)
                    )


def makefile_targets(root):
    targets = set()
    for line in read(os.path.join(root, "Makefile")).splitlines():
        match = re.match(r"^([A-Za-z0-9_.-]+(?:\s+[A-Za-z0-9_.-]+)*)\s*:(?!=)", line)
        if match:
            targets.update(match.group(1).split())
    return targets


def feature_make_targets(spec):
    targets = set()
    for task in feature_tasks(spec):
        if "Makefile" not in (task.get("touches") or []):
            continue
        for match in re.finditer(r"\bmake ((?:[A-Za-z0-9_.-]+ ?)+)", str(task["verify_cmd"])):
            targets.update(match.group(1).split())
    return targets


def crate_manifests(root):
    names = {}
    for manifest in glob.glob(os.path.join(root, "*", "crates", "*", "Cargo.toml")):
        found = re.search(r'^name\s*=\s*"([^"]+)"', read(manifest), re.M)
        if found:
            names[found.group(1)] = os.path.relpath(manifest, root)
    return names


def check_command_paths(root, spec, declared, job_id, directory, tokens):
    """Every path a command names resolves from the repository root."""
    base = directory or ""
    for index, token in enumerate(tokens):
        if token.startswith("/"):
            refuse(
                "the job %s names the absolute path %s, so the command is not "
                "relative to the repository root" % (job_id, token)
            )
        if token in ("cd", "pushd", "popd"):
            refuse("the job %s changes directory outside a subshell in %s" % (job_id, tokens))
        if ".." in token.split("/") and not token.endswith("/..."):
            refuse("the job %s climbs out of its directory with %s" % (job_id, token))
    program = tokens[0]
    if "=" in program and program.split("=", 1)[0].isupper():
        variable, value = program.split("=", 1)
        if variable == "FOUNDRY_CONFIG":
            present_or_declared(root, declared, os.path.join(base, value), "Foundry configuration")
        tokens = tokens[1:]
        program = tokens[0] if tokens else ""
    if program not in ALLOWED_PROGRAMS:
        refuse(
            "the job %s runs %r, whose program is not one this workflow's legs use"
            % (job_id, program)
        )
        return
    arguments = tokens[1:]
    for index, token in enumerate(arguments):
        following = arguments[index + 1] if index + 1 < len(arguments) else None
        if token in ("--manifest-path", "--match-path") and following:
            present_or_declared(root, declared, os.path.join(base, following), "command path")
        if token == "-p" and following:
            manifests = crate_manifests(root)
            if following in manifests:
                note("the package %s is the crate at %s" % (following, manifests[following]))
            elif any(
                path.endswith("/crates/%s/Cargo.toml" % following) for path in declared
            ):
                note("the package %s is a crate a task of %s adds" % (following, SPEC_PATH))
            else:
                refuse(
                    "the job %s names the package %s, which no Cargo.toml in this "
                    "repository or task of %s declares" % (job_id, following, SPEC_PATH)
                )
    if program == "go":
        for token in arguments:
            if token.startswith("./"):
                package = token[2:]
                if package.endswith("/..."):
                    package = package[: -len("/...")]
                present_or_declared(root, declared, os.path.join(base, package), "Go package")
    if program == "bash" and arguments:
        present_or_declared(root, declared, os.path.join(base, arguments[0]), "script")
    if program == "make":
        known = makefile_targets(root) | feature_make_targets(spec)
        for target in arguments:
            if target.startswith("-") or "=" in target:
                refuse("the job %s passes make the option %s" % (job_id, target))
            elif target in makefile_targets(root):
                note("the make target %s is in the Makefile" % target)
            elif target in known:
                note("the make target %s is added by a task of %s" % (target, SPEC_PATH))
            else:
                refuse(
                    "the job %s runs the make target %s, which neither the Makefile "
                    "nor a task of %s adds" % (job_id, target, SPEC_PATH)
                )
    if program == "node" and arguments:
        script = arguments[-1]
        match = re.match(r"\Adist/(test/[A-Za-z0-9._-]+)\.js\Z", script)
        if match is None:
            refuse("the job %s runs node on %s, which is not a built test" % (job_id, script))
        else:
            present_or_declared(
                root, declared, os.path.join(base, match.group(1) + ".ts"), "TypeScript test"
            )
    if program == "python3" and arguments[:2] == ["-m", "pytest"]:
        for token in arguments[2:]:
            if not token.startswith("-"):
                present_or_declared(root, declared, os.path.join(base, token), "Python test")


def check_commands_run_from_the_root(root, spec, declared, document, jobs):
    defaults = document.get("defaults") or {}
    if ((defaults.get("run") or {}).get("working-directory")) != ".":
        refuse("the workflow does not set defaults.run.working-directory to the repository root")
    else:
        note("every command starts with the repository root as its directory")
    for job_id, job in jobs.items():
        if not isinstance(job, dict):
            continue
        if "defaults" in job:
            refuse("the job %s overrides the working directory" % job_id)
        for step in job.get("steps") or []:
            if isinstance(step, dict) and "working-directory" in step:
                refuse(
                    "the step %r of the job %s sets working-directory %r"
                    % (step.get("name"), job_id, step["working-directory"])
                )
        for command in leg_commands(job):
            directory = None
            parts = [command]
            if command.startswith("("):
                match = SUBSHELL.match(command)
                if match is None:
                    refuse(
                        "the job %s runs the subshell %r, which is not one cd into a "
                        "repository directory followed by its commands" % (job_id, command)
                    )
                    continue
                directory = match.group(1)
                if os.path.isdir(os.path.join(root, directory)):
                    note(
                        "the job %s enters %s inside a subshell, so the next command "
                        "runs from the repository root again" % (job_id, directory)
                    )
                else:
                    refuse("the job %s enters %s, which is not a directory" % (job_id, directory))
                parts = split_top_level(match.group(2))
            for part in parts:
                try:
                    tokens = shlex.split(part)
                except ValueError as error:
                    refuse("the job %s runs %r, which does not parse: %s" % (job_id, part, error))
                    continue
                if not tokens:
                    refuse("the job %s runs an empty command" % job_id)
                    continue
                check_command_paths(root, spec, declared, job_id, directory, tokens)


def check_installers(root, jobs):
    rust = re.search(r'channel\s*=\s*"([^"]+)"', read(os.path.join(root, "rust-toolchain.toml")))
    go_directive = re.search(r"^go (\S+)", read(os.path.join(root, "go.mod")), re.M)
    engines = json.loads(read(os.path.join(root, "agent/sdk/typescript/package.json"))).get("engines") or {}
    node_floor = re.match(r">=\s*(\d+)", str(engines.get("node", "")))
    foundry_pins = action_versions(root, "foundry-rs/foundry-toolchain@")
    for job_id, job in jobs.items():
        if not isinstance(job, dict):
            continue
        steps = [step for step in (job.get("steps") or []) if isinstance(step, dict)]
        commands = leg_commands(job)
        joined = " ".join(commands)
        first_run = min((index for index, _ in run_steps(job)), default=len(steps))
        before = steps[:first_run]
        installs = [
            line
            for step in before
            if "run" in step
            for line in command_lines(step["run"])
        ]
        uses = {str(step["uses"]).split("@")[0]: step for step in before if "uses" in step}
        after_installs = [
            line
            for step in steps[first_run:]
            if "run" in step and not str(step.get("name", "")).startswith("Run ")
            for line in command_lines(step["run"])
        ]
        if after_installs:
            refuse("the job %s installs %s after its first command" % (job_id, after_installs))
        if not steps or not str(steps[0].get("uses", "")).startswith("actions/checkout@"):
            refuse("the job %s does not check out the source first" % job_id)
        if re.search(r"(?<![\w-])cargo\b", joined):
            wanted = "rustup toolchain install %s " % (rust.group(1) if rust else "?")
            if any(line.startswith(wanted) for line in installs):
                note("the job %s installs the pinned Rust %s first" % (job_id, rust.group(1)))
            else:
                refuse(
                    "the job %s runs cargo without first installing the Rust %s "
                    "rust-toolchain.toml pins" % (job_id, rust.group(1) if rust else "?")
                )
        if re.search(r"(?<![\w-])go test\b", joined):
            step = uses.get("actions/setup-go")
            version = str((step or {}).get("with", {}).get("go-version"))
            if step is None:
                refuse("the job %s runs go without a step that installs it (actions/setup-go)" % job_id)
            elif go_directive is None or version != go_directive.group(1):
                refuse(
                    "the job %s installs Go %s, not the %s go.mod names"
                    % (job_id, version, go_directive.group(1) if go_directive else "?")
                )
            else:
                note("the job %s installs the Go %s go.mod names" % (job_id, version))
        if re.search(r"\bforge\b", joined):
            step = uses.get("foundry-rs/foundry-toolchain")
            version = str((step or {}).get("with", {}).get("version"))
            if step is None:
                refuse("the job %s runs forge without a step that installs it (foundry-rs/foundry-toolchain)" % job_id)
            elif not EXACT_VERSION.match(version):
                refuse(
                    "the job %s installs Foundry %s, a moving channel rather than a version"
                    % (job_id, version)
                )
            elif version not in foundry_pins:
                refuse(
                    "the job %s installs Foundry %s, none of the %s this repository's "
                    "workflows pin" % (job_id, version, sorted(foundry_pins))
                )
            else:
                note("the job %s installs the pinned Foundry %s" % (job_id, version))
        if re.search(r"(?<![\w-])make\b", joined):
            packages = apt_packages(installs)
            if KERNEL_PACKAGES <= packages:
                note("the job %s installs the kernel's native dependencies" % job_id)
            else:
                refuse(
                    "the job %s runs make without the kernel's native dependencies %s"
                    % (job_id, sorted(KERNEL_PACKAGES - packages))
                )
            if not any(WASM_TARGET in line for line in installs if line.startswith("rustup ")):
                refuse("the job %s runs the programs targets without the %s target" % (job_id, WASM_TARGET))
        if "interop/Cargo.toml" in joined:
            packages = apt_packages(installs)
            if SIDECAR_PACKAGES <= packages:
                note("the job %s installs the native-tls build dependencies" % job_id)
            else:
                refuse(
                    "the job %s builds the interop workspace without %s"
                    % (job_id, sorted(SIDECAR_PACKAGES - packages))
                )
        if re.search(r"\bnpm\b|\bnode\b", joined):
            step = uses.get("actions/setup-node")
            version = str((step or {}).get("with", {}).get("node-version"))
            if step is None:
                refuse("the job %s runs npm without a step that installs it (actions/setup-node)" % job_id)
            elif node_floor is None or not version.isdigit() or int(version) < int(node_floor.group(1)):
                refuse(
                    "the job %s installs Node.js %s, which the TypeScript SDK's engines "
                    "%r do not admit" % (job_id, version, engines.get("node"))
                )
            else:
                note("the job %s installs Node.js %s" % (job_id, version))
        if re.search(r"\bpytest\b", joined):
            if "python3-pytest" in apt_packages(installs):
                note("the job %s installs pytest first" % job_id)
            else:
                refuse("the job %s runs pytest without a step that installs it" % job_id)


def apt_packages(lines):
    packages = set()
    for line in lines:
        for segment in line.split("&&"):
            tokens = segment.split()
            if "install" in tokens and ("apt-get" in tokens or "apt" in tokens):
                after = tokens[tokens.index("install") + 1:]
                packages.update(token for token in after if not token.startswith("-"))
    return packages


def action_versions(root, prefix):
    versions = set()
    for path in sorted(glob.glob(os.path.join(root, ".github", "workflows", "*.yml"))):
        if os.path.basename(path) == os.path.basename(WORKFLOW_PATH):
            continue
        try:
            other = yaml.safe_load(read(path))
        except yaml.YAMLError:
            continue
        if not isinstance(other, dict):
            continue
        for job in (other.get("jobs") or {}).values():
            if not isinstance(job, dict):
                continue
            for step in job.get("steps") or []:
                if isinstance(step, dict) and str(step.get("uses", "")).startswith(prefix):
                    version = str((step.get("with") or {}).get("version"))
                    if EXACT_VERSION.match(version):
                        versions.add(version)
    return versions


def check_actions_are_pinned(root, text):
    others = []
    for path in sorted(glob.glob(os.path.join(root, ".github", "workflows", "*.yml"))):
        if os.path.basename(path) != os.path.basename(WORKFLOW_PATH):
            others.append(read(path))
    joined = "\n".join(others)
    for line in text.splitlines():
        match = USES_VALUE.search(line)
        if not match:
            continue
        value = match.group(1)
        if not PINNED_USES.match(value):
            refuse("the action %s is not pinned to a 40-character commit" % value)
            continue
        if "#" not in line:
            refuse("the action %s carries no version comment" % value)
        if value in joined:
            note("the action %s is the pin another workflow uses" % value)
        else:
            refuse(
                "the action %s is pinned to a commit no other workflow in this "
                "repository uses" % value
            )


def check_nothing_is_excused(document, jobs, text):
    if "continue-on-error" in text:
        refuse("the workflow carries continue-on-error")
    else:
        note("no job or step carries continue-on-error")
    for job_id, job in jobs.items():
        if not isinstance(job, dict):
            continue
        if "if" in job:
            refuse("the job %s runs only under the condition %r" % (job_id, job["if"]))
        if "timeout-minutes" not in job:
            refuse("the job %s carries no timeout-minutes" % job_id)
        strategy = job.get("strategy") or {}
        if isinstance(strategy, dict) and strategy.get("fail-fast") is False:
            refuse("the job %s lets a failing leg run on beside others" % job_id)
        for step in job.get("steps") or []:
            if isinstance(step, dict) and "if" in step:
                refuse(
                    "the step %r of the job %s runs only under the condition %r"
                    % (step.get("name"), job_id, step["if"])
                )
    if document.get("permissions") != {"contents": "read"}:
        refuse("the workflow permissions are %r, not contents: read" % (document.get("permissions"),))
    else:
        note("the workflow reads the repository contents and nothing else")
    if "secrets." in text:
        refuse("the workflow reads a secret, which no xweb leg needs")
    for match in IPV4.finditer(text):
        refuse("the workflow carries the address %s" % match.group(0))
    for match in DATE.finditer(text):
        refuse("the workflow carries the date %s" % match.group(0))


def main():
    workflow, root = sys.argv[1], sys.argv[2]
    text = read(workflow)
    try:
        document = yaml.safe_load(text)
    except yaml.YAMLError as error:
        sys.stderr.write("xweb-workflow-check: refused: the workflow does not parse: %s\n" % error)
        return 2
    if not isinstance(document, dict):
        sys.stderr.write("xweb-workflow-check: refused: the workflow is not a mapping\n")
        return 2
    spec = load_spec(root)
    declared = declared_paths(spec)
    gate = gate_commands(spec)
    jobs = document.get("jobs") or {}
    if not isinstance(jobs, dict):
        refuse("the workflow carries no job mapping")
        jobs = {}
    triggers = check_triggers(document)
    check_concurrency(document)
    check_filters(root, declared, triggers)
    check_jobs_are_the_legs(jobs, gate)
    check_names_claim_only_what_runs(jobs)
    check_commands_run_from_the_root(root, spec, declared, document, jobs)
    check_installers(root, jobs)
    check_actions_are_pinned(root, text)
    check_nothing_is_excused(document, jobs, text)
    for line in CHECKED:
        sys.stderr.write("xweb-workflow-check: %s\n" % line)
    for line in REFUSALS:
        sys.stderr.write("xweb-workflow-check: refused: %s\n" % line)
    return 2 if REFUSALS else 0


if __name__ == "__main__":
    sys.exit(main())
PYCHECK

cat > "$WORK/mutate.py" <<'PYMUTATE'
"""Write a single-change mutant of the xweb workflow, for the negative half."""

import sys

MUTATIONS = {
    "continue-on-error": (
        "    runs-on: ubuntu-24.04\n    timeout-minutes: 30\n",
        "    runs-on: ubuntu-24.04\n    continue-on-error: true\n"
        "    timeout-minutes: 30\n",
        1,
    ),
    "no-cancel": (
        "  cancel-in-progress: true\n",
        "  cancel-in-progress: false\n",
        1,
    ),
    "shared-group": (
        "  group: xweb-test-${{ github.ref }}\n",
        "  group: xweb-test\n",
        1,
    ),
    "extra-filter": (
        "      - 'modules/xweb/**'\n",
        "      - 'modules/xweb/**'\n      - 'docs/**'\n",
        2,
    ),
    "missing-directory": (
        "      - 'modules/xweb/**'\n",
        "      - 'modules/xweb-absent/**'\n",
        2,
    ),
    "narrowed-crates": (
        "      - 'agent/crates/**'\n",
        "      - 'agent/crates/layerx-mcp/**'\n",
        2,
    ),
    "dropped-kernel-tree": (
        "      - 'tests/sequencer/**'\n",
        "",
        2,
    ),
    "undeclared-file": (
        "      - 'tests/test_web_program_path.c'\n",
        "      - 'tests/test_web_program_absent.c'\n",
        2,
    ),
    "dropped-command": (
        "\n      - name: Run go test for modules/xweb and precompiles/xweb\n"
        "        run: go test -count=1 ./modules/xweb/... ./precompiles/xweb/...\n",
        "\n",
        1,
    ),
    "unpinned-action": (
        "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1",
        "actions/checkout@v4",
        1,
    ),
    "renamed-job": ("\n  module-precompile:\n", "\n  everything:\n", 1),
    "misnamed-leg": (
        "    name: cargo test for the x-websearch crate\n",
        "    name: go test for the x-websearch crate\n",
        1,
    ),
    "working-directory": (
        "        run: cargo test --locked --manifest-path interop/Cargo.toml -p x-websearch\n",
        "        run: cargo test --locked --manifest-path interop/Cargo.toml -p x-websearch\n"
        "        working-directory: interop\n",
        1,
    ),
    "bare-cd": (
        "        run: (cd agent/sdk/python && python3 -m pytest -q tests/test_web_search.py)\n",
        "        run: cd agent/sdk/python && python3 -m pytest -q tests/test_web_search.py\n",
        1,
    ),
    "missing-installer": (
        "      - name: Install the pinned Go toolchain\n"
        "        uses: actions/setup-go@b7ad1dad31e06c5925ef5d2fc7ad053ef454303e # v7.0.0\n"
        "        with:\n"
        "          go-version: '1.25.6'\n\n",
        "",
        1,
    ),
    "floating-foundry": (
        "          version: v1.8.1\n",
        "          version: stable\n",
        1,
    ),
    "conditional-step": (
        "      - name: Run forge test for contracts/test/XWebConsumer.t.sol\n",
        "      - name: Run forge test for contracts/test/XWebConsumer.t.sol\n"
        "        if: github.event_name == 'push'\n",
        1,
    ),
}


def main():
    name, source, destination = sys.argv[1], sys.argv[2], sys.argv[3]
    if name not in MUTATIONS:
        sys.stderr.write("mutate: unknown mutation %s\n" % name)
        return 1
    old, new, count = MUTATIONS[name]
    with open(source, "r", encoding="utf-8") as handle:
        text = handle.read()
    if text.count(old) < count:
        sys.stderr.write(
            "mutate: the %s mutation does not apply: %r appears %d times, not %d\n"
            % (name, old, text.count(old), count)
        )
        return 1
    mutant = text.replace(old, new, count)
    if mutant == text:
        sys.stderr.write("mutate: the %s mutation changed nothing\n" % name)
        return 1
    with open(destination, "w", encoding="utf-8") as handle:
        handle.write(mutant)
    return 0


if __name__ == "__main__":
    sys.exit(main())
PYMUTATE

python3 "$WORK/check.py" "$WORKFLOW" "$REPO_ROOT" \
    || fail "the xweb workflow does not say what it runs"

for entry in \
    "continue-on-error|continue-on-error" \
    "no-cancel|cancel-in-progress" \
    "shared-group|one group per ref" \
    "extra-filter|docs/**" \
    "missing-directory|modules/xweb-absent/**" \
    "narrowed-crates|agent/crates/layerx-mcp/**" \
    "dropped-kernel-tree|path filters are" \
    "undeclared-file|tests/test_web_program_absent.c names no file" \
    "dropped-command|go test" \
    "unpinned-action|actions/checkout@v4" \
    "renamed-job|module-precompile" \
    "misnamed-leg|named for go test" \
    "working-directory|working-directory" \
    "bare-cd|changes directory outside a subshell" \
    "missing-installer|actions/setup-go" \
    "floating-foundry|moving channel" \
    "conditional-step|condition"; do
    mutation=${entry%%|*}
    keyword=${entry#*|}
    python3 "$WORK/mutate.py" "$mutation" "$WORKFLOW" "$WORK/$mutation.yml" \
        || fail "the $mutation mutation could not be applied to the workflow"
    status=0
    output=$(python3 "$WORK/check.py" "$WORK/$mutation.yml" "$REPO_ROOT" 2>&1) || status=$?
    [ "$status" -eq 2 ] \
        || fail "the check did not refuse the $mutation mutation (exit $status): $output"
    printf '%s\n' "$output" | grep -F 'refused:' | grep -qF -- "$keyword" \
        || fail "the check refused the $mutation mutation without naming $keyword: $output"
done

printf 'xweb-workflow-check: the xweb workflow carries the feature path filters, one concurrency group per ref, the five legs running the gate commands from the repository root and no excused step, and refuses every mutation of them\n' >&2
