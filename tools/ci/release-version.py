#!/usr/bin/env python3
"""Resolve the release version the source revision declares and refuse drift.

usage: tools/ci/release-version.py [--tag <sdk-v...>]

Without --tag the version is the agent workspace version and every published
manifest must agree with it. With --tag the version is taken from the release
tag, must be a beta pre-release (<major>.<minor>.<patch>-beta.<n>), every
published manifest must declare exactly that version, and every dependency one
published package has on another must pin exactly that version so installing
from a registry resolves to the published pre-release and nothing else.

The resolved version is printed as version=<version> and appended to
$GITHUB_OUTPUT when that file is named.
"""
import json
import os
import re
import sys
import xml.etree.ElementTree as ElementTree
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TAG_PREFIX = "sdk-v"
BETA = re.compile(r"^(\d+)\.(\d+)\.(\d+)-beta\.(\d+)$")

CARGO_WORKSPACES = ["agent/Cargo.toml", "interop/Cargo.toml", "programs/Cargo.toml"]
CRATES = {
    "layerx-agent-api": "agent/crates/layerx-agent-api/Cargo.toml",
    "layerx-client": "agent/crates/layerx-client/Cargo.toml",
    "layerx-crypto": "agent/crates/layerx-crypto/Cargo.toml",
    "layerx-mirror": "interop/crates/layerx-mirror/Cargo.toml",
    "layerx-programs-runtime": "programs/crates/layerx-programs-runtime/Cargo.toml",
    "layerx-proof": "agent/crates/layerx-proof/Cargo.toml",
    "layerx-sdk": "agent/crates/layerx-sdk/Cargo.toml",
    "layerx-types": "agent/crates/layerx-types/Cargo.toml",
    "layerx-wire": "agent/crates/layerx-wire/Cargo.toml",
}
NPM_PACKAGES = {
    "@sidiora/layerx-agent-integrations": "platform/integrations/agents/package.json",
    "@sidiora/layerx-agent-middleware": "platform/middleware/agent/package.json",
    "@sidiora/layerx-buyer-middleware": "platform/middleware/buyer/package.json",
    "@sidiora/layerx-express": "platform/integrations/express/package.json",
    "@sidiora/layerx-merchant-middleware": "platform/middleware/merchant/package.json",
    "@sidiora/layerx-next": "platform/integrations/next/package.json",
    "@sidiora/layerx-sdk": "agent/sdk/typescript/package.json",
    "@sidiora/layerx-seller-middleware": "platform/middleware/seller/package.json",
}
PYPROJECTS = {
    "layerx-fastapi": "platform/integrations/fastapi/pyproject.toml",
    "layerx-sdk": "agent/sdk/python/pyproject.toml",
}
POMS = {
    "com.sidiora.layerx:layerx-android": "platform/integrations/android/pom.xml",
    "com.sidiora.layerx:layerx-sdk": "platform/sdk/jvm/pom.xml",
    "com.sidiora.layerx:layerx-spring-boot-starter": "platform/integrations/spring/pom.xml",
}
CSPROJ = {"LayerX.Sdk": "platform/sdk/dotnet/LayerX.Sdk.csproj"}
POM_NS = "{http://maven.apache.org/POM/4.0.0}"

problems = []


def problem(message):
    problems.append(message)


def read(relative):
    return (ROOT / relative).read_text(encoding="utf-8")


def workspace_version(relative):
    text = read(relative)
    section = re.search(r"^\[workspace\.package\]\n(.*?)(?=^\[|\Z)", text, re.S | re.M)
    if section is None:
        problem(f"{relative}: no [workspace.package] section")
        return None
    match = re.search(r'^version = "([^"]+)"', section.group(1), re.M)
    if match is None:
        problem(f"{relative}: [workspace.package] declares no version")
        return None
    return match.group(1)


def pep440(version):
    return BETA.sub(lambda m: f"{m.group(1)}.{m.group(2)}.{m.group(3)}b{m.group(4)}", version)


def check_cargo(version, release):
    for workspace in CARGO_WORKSPACES:
        declared = workspace_version(workspace)
        if declared is not None and declared != version:
            problem(f"{workspace}: workspace version {declared} is not {version}")
    for crate, manifest in CRATES.items():
        text = read(manifest)
        if not re.search(r"^version\.workspace = true$", text, re.M):
            problem(f"{manifest}: {crate} must take its version from the workspace")
        if not release:
            continue
        for match in re.finditer(r"^(layerx-[a-z0-9-]+) = \{([^}]*)\}", text, re.M):
            dependency, body = match.group(1), match.group(2)
            if dependency not in CRATES:
                continue
            pin = re.search(r'version = "([^"]+)"', body)
            if pin is None or pin.group(1) != f"={version}":
                problem(
                    f"{manifest}: dependency {dependency} must pin version = \"={version}\" for the release, "
                    f"got {pin.group(1) if pin else 'no version'}"
                )


def check_npm(version):
    for package, manifest in NPM_PACKAGES.items():
        data = json.loads(read(manifest))
        if data.get("name") != package:
            problem(f"{manifest}: package name {data.get('name')!r} is not {package}")
        if data.get("version") != version:
            problem(f"{manifest}: version {data.get('version')!r} is not {version}")
        if data.get("private") is not False:
            problem(f"{manifest}: {package} must declare \"private\": false")
        repository = data.get("repository") or {}
        if not isinstance(repository, dict) or not repository.get("url") or not repository.get("directory"):
            problem(f"{manifest}: {package} must declare repository.url and repository.directory for npm provenance")
        for field in ("dependencies", "peerDependencies", "optionalDependencies"):
            for dependency, requirement in (data.get(field) or {}).items():
                if dependency in NPM_PACKAGES and requirement != version:
                    problem(f"{manifest}: {field} {dependency} must be pinned to {version}, got {requirement!r}")


def check_python(version, release):
    expected = pep440(version)
    for package, manifest in PYPROJECTS.items():
        text = read(manifest)
        project = re.search(r"^\[project\]\n(.*?)(?=^\[|\Z)", text, re.S | re.M)
        if project is None:
            problem(f"{manifest}: no [project] table")
            continue
        body = project.group(1)
        name = re.search(r'^name = "([^"]+)"', body, re.M)
        declared = re.search(r'^version = "([^"]+)"', body, re.M)
        if name is None or name.group(1) != package:
            problem(f"{manifest}: project name is not {package}")
        if declared is None or pep440(declared.group(1)) != expected:
            problem(f"{manifest}: version {declared.group(1) if declared else None!r} is not {version}")
        dependencies = re.search(r"^dependencies = \[(.*?)\]", body, re.M | re.S)
        for requirement in re.findall(r'"([^"]+)"', dependencies.group(1) if dependencies else ""):
            dependency = re.split(r"[<>=!~ \[;]", requirement, 1)[0]
            if dependency in PYPROJECTS and release and requirement != f"{dependency}=={expected}":
                problem(f"{manifest}: dependency {requirement!r} must pin {dependency}=={expected} for the release")


def check_maven(version):
    for coordinate, manifest in POMS.items():
        group, artifact = coordinate.split(":")
        root = ElementTree.fromstring(read(manifest))
        declared = {child.tag.removeprefix(POM_NS): (child.text or "").strip() for child in root}
        if declared.get("groupId") != group or declared.get("artifactId") != artifact:
            problem(f"{manifest}: coordinates are not {coordinate}")
        if declared.get("version") != version:
            problem(f"{manifest}: version {declared.get('version')!r} is not {version}")
        properties = root.find(f"{POM_NS}properties")
        if properties is not None:
            pin = properties.find(f"{POM_NS}layerx.sdk.version")
            if pin is not None and (pin.text or "").strip() != version:
                problem(f"{manifest}: layerx.sdk.version {pin.text!r} is not {version}")
        for field in ("url", "licenses", "developers", "scm"):
            if root.find(f"{POM_NS}{field}") is None:
                problem(f"{manifest}: Maven Central requires <{field}>")


def check_dotnet(version):
    for package, manifest in CSPROJ.items():
        text = read(manifest)
        package_id = re.search(r"<PackageId>([^<]+)</PackageId>", text)
        declared = re.search(r"<Version>([^<]+)</Version>", text)
        if package_id is None or package_id.group(1) != package:
            problem(f"{manifest}: PackageId is not {package}")
        if declared is None or declared.group(1) != version:
            problem(f"{manifest}: Version {declared.group(1) if declared else None!r} is not {version}")


def main(argv):
    tag = None
    args = list(argv)
    while args:
        argument = args.pop(0)
        if argument == "--tag" and args:
            tag = args.pop(0)
        elif argument in ("-h", "--help"):
            print(__doc__)
            return 0
        else:
            print(__doc__, file=sys.stderr)
            return 2
    release = tag is not None
    if release:
        if not tag.startswith(TAG_PREFIX):
            print(f"release-version: tag {tag!r} does not start with {TAG_PREFIX}", file=sys.stderr)
            return 1
        version = tag[len(TAG_PREFIX):]
        if BETA.match(version) is None:
            print(f"release-version: {version!r} is not a beta pre-release <major>.<minor>.<patch>-beta.<n>", file=sys.stderr)
            return 1
    else:
        version = workspace_version(CARGO_WORKSPACES[0])
        if version is None:
            print("\n".join(problems), file=sys.stderr)
            return 1
    check_cargo(version, release)
    check_npm(version)
    check_python(version, release)
    check_maven(version)
    check_dotnet(version)
    if problems:
        for entry in problems:
            print(f"release-version: {entry}", file=sys.stderr)
        return 1
    print(f"version={version}")
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as handle:
            handle.write(f"version={version}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
