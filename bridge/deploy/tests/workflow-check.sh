#!/usr/bin/env bash
# Offline check of .github/workflows/bridge-test.yml: the workflow triggers on
# pull requests, the merge group and pushes to main and the release branches; it
# carries exactly the three path filters bridge/**,
# interop/crates/layerx-bridge-relayer/** and the workflow file itself, each
# naming a path this repository holds or this feature's spec declares; it runs
# four jobs whose names are the four legs and whose names claim no tool the job
# does not invoke; every command runs from the repository root with a tool the
# same job installs, at a version this repository already pins; every action is
# pinned by commit to a pin another workflow in this repository already uses; and
# no continue-on-error is present. The check then mutates the workflow seven ways
# and requires each mutation to be refused, so the assertions are known to bite.
# No network and no runner are involved.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/../../.." && pwd)
WORKFLOW="$REPO_ROOT/.github/workflows/bridge-test.yml"

fail() { printf 'workflow-check: error: %s\n' "$*" >&2; exit 1; }

command -v python3 >/dev/null 2>&1 \
    || fail "python3 is required to parse the workflow"
python3 -c 'import yaml' >/dev/null 2>&1 \
    || fail "the python3 yaml module is required to parse the workflow"
[ -f "$WORKFLOW" ] \
    || fail "$WORKFLOW is missing"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
chmod 0700 "$WORK"

cat > "$WORK/check.py" <<'PYCHECK'
"""Assert that .github/workflows/bridge-test.yml says what it runs.

Exit 0 when every assertion holds, 2 when any is refused. Every refusal names
the offending value.
"""

import glob
import json
import os
import re
import shlex
import sys

import yaml

WORKFLOW_PATH = ".github/workflows/bridge-test.yml"
FILTERS = [
    "bridge/**",
    "interop/crates/layerx-bridge-relayer/**",
    WORKFLOW_PATH,
]
PUSH_BRANCHES = ["main", "release/**"]
TRIGGERS = ["pull_request", "merge_group", "push"]

# The four legs: job id -> the job name, the tool phrases the name carries and
# the repository paths the job's commands must name.
LEGS = {
    "evm-vault": {
        "name": "forge test for the bridge/evm vault",
        "tools": ["forge test"],
        "paths": ["bridge/evm"],
    },
    "solana-program": {
        "name": "cargo test and cargo build-sbf for the bridge/solana program",
        "tools": ["cargo test", "cargo build-sbf"],
        "paths": ["bridge/solana/Cargo.toml"],
    },
    "relayer-crate": {
        "name": "cargo test for the layerx-bridge-relayer crate",
        "tools": ["cargo test"],
        "paths": ["interop/Cargo.toml"],
    },
    "deploy-tooling": {
        "name": (
            "go test and the offline check scripts for bridge/deploy "
            "and bridge/vectors"
        ),
        "tools": ["go test"],
        "paths": ["bridge/vectors", "bridge/deploy"],
    },
}

# "go test" is a substring of "cargo test", so a phrase matches only where it
# begins a word: a job named for one tool can never read as a claim about
# another.
TOOL_PHRASES = {
    phrase: re.compile(r"(?<![\w-])%s\b" % re.escape(phrase))
    for phrase in ("forge test", "cargo test", "cargo build-sbf", "go test")
}

# The tool a command invokes and the step that must install it in the same job.
TOOL_INSTALLERS = {
    "forge": ("uses", "foundry-rs/foundry-toolchain@"),
    "cargo": ("run", "rustup toolchain install"),
    "go": ("uses", "actions/setup-go@"),
    "cargo build-sbf": ("run", "release.anza.xyz"),
}

ALLOWED_PROGRAMS = {
    "bash",
    "cargo",
    "curl",
    "forge",
    "git",
    "go",
    "printf",
    "rustup",
    "sh",
}

# Paths the commands read: each must exist in the repository or be declared by
# this feature's spec, which is what a sibling wave task creates.
INPUT_PATHS = {
    "bridge/evm",
    "bridge/solana/Cargo.toml",
    "interop/Cargo.toml",
    "bridge/vectors",
    "bridge/deploy",
    "bridge/deploy/tests/workflow-check.sh",
    "bridge/deploy/tests/deploy-scripts-check.sh",
    "bridge/deploy/tests/checklist-check.sh",
    "bridge/deploy/tests/docs-check.sh",
}
# Paths the commands create rather than read.
CLONE_DESTINATIONS = {
    "bridge/evm/lib/forge-std",
    "bridge/evm/lib/openzeppelin-contracts",
}
OFFLINE_CHECKS = {
    "bridge/deploy/tests/workflow-check.sh",
    "bridge/deploy/tests/deploy-scripts-check.sh",
    "bridge/deploy/tests/checklist-check.sh",
    "bridge/deploy/tests/docs-check.sh",
}

SPEC_PATH = "spec/paxeer-x-bridge/spec.kvx"
REFERENCE_WORKFLOW = ".github/workflows/paxeer-forge-test.yml"

PATH_TOKEN = re.compile(
    r"(?<![A-Za-z0-9._/-])\.?/?((?:bridge|interop)/[A-Za-z0-9._/*-]+)"
)
CLONE_LINE = re.compile(
    r"git clone --depth 1 --branch (\S+) (https://\S+) (\S+)"
)
USES_VALUE = re.compile(r"\buses:\s*(\S+)")
PINNED_USES = re.compile(r"[\w.-]+/[\w./-]+@[0-9a-f]{40}\Z")
ANZA_VERSION = re.compile(r"release\.anza\.xyz/v([0-9][0-9.]*)/install")
FOUNDRY_ACTION = "foundry-rs/foundry-toolchain@"
# A version, not a moving channel: stable and nightly change what forge does
# without a commit, so the bytecode a deploy reproduces would move with them.
EXACT_VERSION = re.compile(r"v?[0-9]+\.[0-9]+\.[0-9]+\Z")
IPV4 = re.compile(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b")
DATE = re.compile(r"\b20\d{2}-\d{2}-\d{2}\b")

REFUSALS = []
CHECKED = []


def refuse(message):
    REFUSALS.append(message)


def note(message):
    CHECKED.append(message)


def read(root, relative):
    with open(os.path.join(root, relative), "r", encoding="utf-8") as handle:
        return handle.read()


def command_lines(run):
    lines = []
    for line in str(run).splitlines():
        stripped = line.strip()
        if stripped and not stripped.startswith("#"):
            lines.append(stripped)
    return lines


def job_commands(job):
    commands = []
    for step in job.get("steps") or []:
        if "run" in step:
            commands.extend(command_lines(step["run"]))
    return commands


def declared_by_spec(spec_text, path):
    return '"%s"' % path in spec_text or '"%s/' % path in spec_text


def crate_names(root):
    names = set()
    manifests = glob.glob(
        os.path.join(root, "interop", "crates", "*", "Cargo.toml")
    ) + glob.glob(os.path.join(root, "bridge", "*", "Cargo.toml"))
    for manifest in manifests:
        with open(manifest, "r", encoding="utf-8") as handle:
            found = re.search(r'^name\s*=\s*"([^"]+)"', handle.read(), re.M)
        if found:
            names.add(found.group(1))
    return names


def action_versions(root, prefix):
    """The exact versions the repository's other workflows pin for an action."""
    versions = set()
    for path in sorted(glob.glob(os.path.join(root, ".github", "workflows", "*.yml"))):
        if os.path.basename(path) == os.path.basename(WORKFLOW_PATH):
            continue
        with open(path, "r", encoding="utf-8") as handle:
            try:
                other = yaml.safe_load(handle.read())
            except yaml.YAMLError:
                continue
        if not isinstance(other, dict):
            continue
        for job in (other.get("jobs") or {}).values():
            if not isinstance(job, dict):
                continue
            for step in job.get("steps") or []:
                if not isinstance(step, dict):
                    continue
                if str(step.get("uses", "")).startswith(prefix):
                    version = str((step.get("with") or {}).get("version"))
                    if EXACT_VERSION.match(version):
                        versions.add(version)
    return versions


def check_triggers(document):
    triggers = document.get("on", document.get(True))
    if not isinstance(triggers, dict):
        refuse("the workflow carries no trigger mapping")
        return
    if sorted(triggers) != sorted(TRIGGERS):
        refuse(
            "the triggers are %s, not the pull request, merge group and push "
            "triggers %s" % (sorted(triggers), sorted(TRIGGERS))
        )
    pull_request = triggers.get("pull_request")
    if not isinstance(pull_request, dict) or sorted(pull_request) != ["paths"]:
        refuse("the pull_request trigger must carry paths and nothing else")
    push = triggers.get("push")
    if not isinstance(push, dict) or sorted(push) != ["branches", "paths"]:
        refuse("the push trigger must carry branches and paths and nothing else")
    elif push["branches"] != PUSH_BRANCHES:
        refuse(
            "the push branches are %s, not %s" % (push["branches"], PUSH_BRANCHES)
        )
    if triggers.get("merge_group") is not None:
        refuse(
            "the merge_group trigger carries %s; GitHub applies no path filter "
            "to a merge group, so carrying one would claim a filter that does "
            "not act" % (triggers["merge_group"],)
        )
    note("the workflow runs on %s" % ", ".join(sorted(TRIGGERS)))
    return triggers


def check_filters(root, triggers):
    if not isinstance(triggers, dict):
        return
    filter_sets = {}
    for trigger in ("pull_request", "push"):
        block = triggers.get(trigger)
        if isinstance(block, dict) and "paths" in block:
            filter_sets[trigger] = block["paths"]
        if isinstance(block, dict) and "paths-ignore" in block:
            refuse("the %s trigger carries a paths-ignore filter" % trigger)
    for trigger, paths in filter_sets.items():
        if paths != FILTERS:
            refuse(
                "the %s path filters are %s, not exactly %s"
                % (trigger, paths, FILTERS)
            )
    if len(filter_sets) == 2 and len(set(map(tuple, filter_sets.values()))) != 1:
        refuse("the pull request and push path filters differ")
    for paths in filter_sets.values():
        for entry in paths:
            if entry.startswith("!"):
                refuse("the path filter %s is a negation" % entry)
                continue
            if entry.endswith("/**"):
                directory = entry[: -len("/**")]
                if os.path.isdir(os.path.join(root, directory)):
                    note("the path filter %s names the directory %s" % (entry, directory))
                else:
                    refuse(
                        "the path filter %s names no directory: %s is absent"
                        % (entry, directory)
                    )
            elif entry == WORKFLOW_PATH:
                if os.path.isfile(os.path.join(root, entry)):
                    note("the path filter %s names the workflow file itself" % entry)
                else:
                    refuse("the path filter %s names no file" % entry)
            else:
                refuse(
                    "the path filter %s is neither a directory glob nor the "
                    "workflow file" % entry
                )


def check_jobs_are_the_legs(jobs):
    if sorted(jobs) != sorted(LEGS):
        missing = sorted(set(LEGS) - set(jobs))
        extra = sorted(set(jobs) - set(LEGS))
        refuse(
            "the jobs are %s, not the four legs %s (missing %s, unexpected %s)"
            % (sorted(jobs), sorted(LEGS), missing, extra)
        )
    for job_id, leg in LEGS.items():
        job = jobs.get(job_id)
        if job is None:
            continue
        if job.get("name") != leg["name"]:
            refuse(
                "the job %s is named %r, not %r"
                % (job_id, job.get("name"), leg["name"])
            )
        else:
            note("the job %s is named %r" % (job_id, leg["name"]))


def check_names_claim_only_what_runs(jobs):
    for job_id, job in jobs.items():
        name = str(job.get("name", ""))
        commands = " ".join(job_commands(job))
        for phrase, pattern in TOOL_PHRASES.items():
            claimed = bool(pattern.search(name))
            run = bool(pattern.search(commands))
            if claimed and not run:
                refuse(
                    "the job %s is named for %s but runs no such command"
                    % (job_id, phrase)
                )
            if run and not claimed:
                refuse(
                    "the job %s runs %s but its name does not say so"
                    % (job_id, phrase)
                )
            if claimed and run:
                note("the job %s names and runs %s" % (job_id, phrase))
        scripts = sorted(
            {
                token
                for line in job_commands(job)
                for token in shlex.split(line)
                if token.endswith("-check.sh")
            }
        )
        if ("check scripts" in name) != bool(scripts):
            refuse(
                "the job %s names %r and runs the check scripts %s, which do "
                "not agree" % (job_id, name, scripts)
            )
        if scripts:
            if set(scripts) != OFFLINE_CHECKS:
                refuse(
                    "the job %s runs the check scripts %s, not the offline set "
                    "%s" % (job_id, scripts, sorted(OFFLINE_CHECKS))
                )
            for script in scripts:
                if script.endswith("-dry-run-check.sh"):
                    refuse(
                        "the job %s runs %s, which needs a node and is not an "
                        "offline check" % (job_id, script)
                    )
            note(
                "the job %s runs the four offline check scripts" % job_id
            )


def check_commands_run_from_the_root(root, document, jobs, spec_text):
    defaults = document.get("defaults") or {}
    if ((defaults.get("run") or {}).get("working-directory")) != ".":
        refuse(
            "the workflow does not set defaults.run.working-directory to the "
            "repository root"
        )
    else:
        note("every command runs with the repository root as its directory")
    named = set()
    for job_id, job in jobs.items():
        if "defaults" in job:
            refuse("the job %s overrides the working directory" % job_id)
        uses = [
            str(step["uses"]) for step in (job.get("steps") or []) if "uses" in step
        ]
        runs = []
        for step in job.get("steps") or []:
            if "working-directory" in step:
                refuse(
                    "the step %r of the job %s sets working-directory %r"
                    % (step.get("name"), job_id, step["working-directory"])
                )
            if "run" in step:
                runs.extend(command_lines(step["run"]))
        joined = " ".join(runs)
        for line in runs:
            tokens = shlex.split(line)
            if not tokens:
                continue
            if tokens[0] not in ALLOWED_PROGRAMS:
                refuse(
                    "the job %s runs %r, whose program is not one this "
                    "repository's runners carry" % (job_id, tokens[0])
                )
            for token in tokens:
                if token.startswith("/"):
                    refuse(
                        "the job %s names the absolute path %s, so the command "
                        "is not relative to the repository root"
                        % (job_id, token)
                    )
                if token in ("cd", "pushd"):
                    refuse(
                        "the job %s changes directory in %r" % (job_id, line)
                    )
            for index, token in enumerate(tokens[:-1]):
                if token == "-p":
                    crate = tokens[index + 1]
                    if crate in crate_names(root):
                        note("the package %s is a crate in this repository" % crate)
                    else:
                        refuse(
                            "the job %s names the package %s, which no "
                            "Cargo.toml in this repository declares"
                            % (job_id, crate)
                        )
        for tool, (kind, installer) in TOOL_INSTALLERS.items():
            if not re.search(r"\b%s\b" % re.escape(tool), joined):
                continue
            if kind == "uses":
                present = any(installer in value for value in uses)
            else:
                present = any(installer in line for line in runs)
            if present:
                note("the job %s installs %s before running it" % (job_id, tool))
            else:
                refuse(
                    "the job %s runs %s without a step that installs it (%s %s)"
                    % (job_id, tool, kind, installer)
                )
        for token in PATH_TOKEN.findall(joined):
            path = token
            for suffix in ("/...", "/**"):
                if path.endswith(suffix):
                    path = path[: -len(suffix)]
            path = path.rstrip("/")
            named.add(path)
        for path in LEGS.get(job_id, {}).get("paths", []):
            if path not in PATH_TOKEN.findall(joined):
                stripped = {
                    re.sub(r"(/\.\.\.|/\*\*)$", "", found)
                    for found in PATH_TOKEN.findall(joined)
                }
                if path not in stripped:
                    refuse(
                        "the job %s names none of the paths its leg exercises: "
                        "%s is absent" % (job_id, path)
                    )
    unknown = named - INPUT_PATHS - CLONE_DESTINATIONS
    if unknown:
        refuse(
            "the workflow names the unchecked paths %s" % sorted(unknown)
        )
    for path in sorted(INPUT_PATHS):
        if path not in named:
            refuse("no job names the path %s this check accounts for" % path)
            continue
        if os.path.exists(os.path.join(root, path)):
            note("the command path %s is present in this repository" % path)
        elif declared_by_spec(spec_text, path):
            note(
                "the command path %s is declared by %s and created by a sibling "
                "wave task" % (path, SPEC_PATH)
            )
        else:
            refuse(
                "the command path %s is neither present nor declared by %s"
                % (path, SPEC_PATH)
            )
    for path in sorted(CLONE_DESTINATIONS):
        if path not in named:
            refuse("no job clones the pinned library into %s" % path)
        elif not path.startswith("bridge/evm/lib/"):
            refuse(
                "the clone destination %s is outside the uncommitted "
                "bridge/evm/lib directory" % path
            )
    if os.path.isdir(os.path.join(root, "bridge", "evm")) and not os.path.isfile(
        os.path.join(root, "bridge", "evm", "foundry.toml")
    ):
        refuse("bridge/evm carries no foundry.toml, so forge cannot root there")


def check_versions_are_the_repository_pins(root, document, jobs):
    runs = [
        line
        for job in jobs.values()
        for line in job_commands(job)
    ]
    rust = [
        line for line in runs if line.startswith("rustup toolchain install ")
    ]
    if not rust:
        refuse("no job installs a pinned Rust toolchain")
    channel = re.search(
        r'channel\s*=\s*"([^"]+)"', read(root, "rust-toolchain.toml")
    )
    for line in rust:
        version = shlex.split(line)[3]
        if channel is None or version != channel.group(1):
            refuse(
                "the workflow installs Rust %s, not the %s this repository's "
                "rust-toolchain.toml pins"
                % (version, channel.group(1) if channel else "unstated version")
            )
        else:
            note("the workflow installs the pinned Rust %s" % version)

    foundry = [
        str((step.get("with") or {}).get("version"))
        for job in jobs.values()
        for step in (job.get("steps") or [])
        if str(step.get("uses", "")).startswith(FOUNDRY_ACTION)
    ]
    if not foundry:
        refuse("no job installs Foundry for the bridge/evm leg")
    pinned = action_versions(root, FOUNDRY_ACTION)
    for version in foundry:
        if not EXACT_VERSION.match(version):
            refuse(
                "the workflow installs Foundry %s, a moving channel rather than "
                "a version, so forge could change what it reports without a "
                "commit" % version
            )
        elif not pinned:
            refuse(
                "no other workflow in this repository pins a Foundry version to "
                "follow"
            )
        elif version not in pinned:
            refuse(
                "the workflow installs Foundry %s, which is none of the %s this "
                "repository's workflows pin" % (version, sorted(pinned))
            )
        else:
            note("the workflow installs the pinned Foundry %s" % version)

    go_directive = re.search(r"^go (\S+)", read(root, "go.mod"), re.M)
    go_versions = [
        str((step.get("with") or {}).get("go-version"))
        for job in jobs.values()
        for step in (job.get("steps") or [])
        if str(step.get("uses", "")).startswith("actions/setup-go@")
    ]
    if not go_versions:
        refuse("no job installs a pinned Go toolchain")
    for version in go_versions:
        if go_directive is None or version != go_directive.group(1):
            refuse(
                "the workflow installs Go %s, not the %s go.mod names"
                % (version, go_directive.group(1) if go_directive else "unstated")
            )
        else:
            note("the workflow installs the Go %s go.mod names" % version)

    reference = set(
        (url, tag)
        for tag, url, _ in CLONE_LINE.findall(read(root, REFERENCE_WORKFLOW))
    )
    mine = set()
    for line in runs:
        match = CLONE_LINE.match(line)
        if match:
            mine.add((match.group(2), match.group(1)))
        elif line.startswith("git clone"):
            refuse(
                "the clone %r does not follow the --depth 1 --branch form %s "
                "uses" % (line, REFERENCE_WORKFLOW)
            )
    if not reference:
        refuse("%s carries no pinned library clone to follow" % REFERENCE_WORKFLOW)
    elif mine != reference:
        refuse(
            "the workflow clones %s, not the %s pinned in %s"
            % (sorted(mine), sorted(reference), REFERENCE_WORKFLOW)
        )
    else:
        note(
            "the workflow clones the same pinned libraries at the same tags as %s"
            % REFERENCE_WORKFLOW
        )

    anza = {
        match.group(1)
        for match in (ANZA_VERSION.search(line) for line in runs)
        if match
    }
    if not anza:
        refuse("no job installs a pinned Solana toolchain for cargo-build-sbf")
    recorded = set()
    for path in sorted(glob.glob(os.path.join(root, "interop", "deploy", "mirror", "*.json"))):
        with open(path, "r", encoding="utf-8") as handle:
            try:
                record = json.load(handle)
            except ValueError:
                continue
        if isinstance(record, dict):
            toolchain = record.get("toolchain")
            if isinstance(toolchain, dict) and "cargo_build_sbf" in toolchain:
                recorded.add(str(toolchain["cargo_build_sbf"]))
    if not recorded:
        refuse("this repository records no cargo-build-sbf version to follow")
    for version in sorted(anza):
        if version in recorded:
            note(
                "the workflow installs the Solana toolchain %s this repository "
                "already records" % version
            )
        else:
            refuse(
                "the workflow installs the Solana toolchain %s, which is none "
                "of the %s this repository records" % (version, sorted(recorded))
            )


def check_actions_are_pinned(root, text):
    others = []
    for path in sorted(glob.glob(os.path.join(root, ".github", "workflows", "*.yml"))):
        if os.path.basename(path) == os.path.basename(WORKFLOW_PATH):
            continue
        with open(path, "r", encoding="utf-8") as handle:
            others.append(handle.read())
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


def check_nothing_is_excused(text):
    if "continue-on-error" in text:
        refuse("the workflow carries continue-on-error")
    else:
        note("no job or step carries continue-on-error")
    if "secrets." in text:
        refuse("the workflow reads a secret, which no bridge leg needs")
    for match in IPV4.finditer(text):
        refuse("the workflow carries the address %s" % match.group(0))
    for match in DATE.finditer(text):
        refuse("the workflow carries the date %s" % match.group(0))


def main():
    workflow, root = sys.argv[1], sys.argv[2]
    text = read(os.path.dirname(workflow) or ".", os.path.basename(workflow))
    document = yaml.safe_load(text)
    spec_text = read(root, SPEC_PATH)
    jobs = document.get("jobs") or {}
    if not isinstance(jobs, dict):
        refuse("the workflow carries no job mapping")
        jobs = {}
    triggers = check_triggers(document)
    check_filters(root, triggers)
    check_jobs_are_the_legs(jobs)
    check_names_claim_only_what_runs(jobs)
    check_commands_run_from_the_root(root, document, jobs, spec_text)
    check_versions_are_the_repository_pins(root, document, jobs)
    check_actions_are_pinned(root, text)
    check_nothing_is_excused(text)
    for line in CHECKED:
        sys.stderr.write("workflow-check: %s\n" % line)
    for line in REFUSALS:
        sys.stderr.write("workflow-check: refused: %s\n" % line)
    return 2 if REFUSALS else 0


if __name__ == "__main__":
    sys.exit(main())
PYCHECK

cat > "$WORK/mutate.py" <<'PYMUTATE'
"""Write a single-change mutant of the bridge workflow, for the negative half."""

import sys

MUTATIONS = {
    "continue-on-error": (
        "    runs-on: ubuntu-24.04\n    timeout-minutes: 30\n",
        "    runs-on: ubuntu-24.04\n    continue-on-error: true\n"
        "    timeout-minutes: 30\n",
        1,
    ),
    "extra-filter": (
        "      - 'bridge/**'\n",
        "      - 'bridge/**'\n      - 'docs/**'\n",
        2,
    ),
    "missing-directory": (
        "      - 'bridge/**'\n",
        "      - 'bridge-does-not-exist/**'\n",
        2,
    ),
    "dropped-command": (
        "      - name: Run forge test for the bridge/evm vault\n"
        "        run: forge test --root bridge/evm -vvv\n",
        "",
        1,
    ),
    "unpinned-action": (
        "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1",
        "actions/checkout@v4",
        1,
    ),
    "renamed-job": ("\n  evm-vault:\n", "\n  everything:\n", 1),
    "floating-foundry": (
        "          version: v1.8.1\n",
        "          version: stable\n",
        1,
    ),
    "working-directory": (
        "        run: forge test --root bridge/evm -vvv\n",
        "        run: forge test --root bridge/evm -vvv\n"
        "        working-directory: bridge/evm\n",
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
    || fail "the bridge workflow does not say what it runs"

for entry in \
    "continue-on-error|continue-on-error" \
    "extra-filter|docs/**" \
    "missing-directory|bridge-does-not-exist/**" \
    "dropped-command|forge test" \
    "unpinned-action|actions/checkout@v4" \
    "renamed-job|evm-vault" \
    "floating-foundry|moving channel" \
    "working-directory|working-directory"; do
    mutation=${entry%%|*}
    keyword=${entry#*|}
    python3 "$WORK/mutate.py" "$mutation" "$WORKFLOW" "$WORK/$mutation.yml" \
        || fail "the $mutation mutation could not be applied to the workflow"
    status=0
    output=$(python3 "$WORK/check.py" "$WORK/$mutation.yml" "$REPO_ROOT" 2>&1) || status=$?
    [ "$status" -eq 2 ] \
        || fail "the check did not refuse the $mutation mutation (exit $status): $output"
    printf '%s\n' "$output" | grep -qF -- "$keyword" \
        || fail "the check refused the $mutation mutation without naming $keyword: $output"
done

printf 'workflow-check: the bridge workflow carries the three path filters, the four legs and no excused step, and refuses every mutation of them\n' >&2
