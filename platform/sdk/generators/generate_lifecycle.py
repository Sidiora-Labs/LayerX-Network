import argparse
import json
import subprocess
from pathlib import Path


def operations(repo):
    sections = {}
    current = None
    for line in (repo / "platform/sdk/generators/receipt.kvx").read_text().splitlines():
        line = line.strip()
        if line.startswith("["):
            current = line[1:-1]
        elif current and current.startswith("program_lifecycle.") and "=" in line:
            key, value = line.split("=", 1)
            sections.setdefault(current, {})[key.strip()] = json.loads(value.strip())
    entries = list(sections.values())
    if [(entry["operation"], entry["ordinal"], entry["path"]) for entry in entries] != [
        ("program.deploy", 1, "/v1/programs/deploy"),
        ("program.upgrade", 2, "/v1/programs/upgrade"),
        ("program.wind-down", 7, "/v1/programs/wind-down"),
    ]:
        raise ValueError("Programs lifecycle contract differs from module 9 ordinals 1, 2, 7")
    return entries


def render(repo):
    entries = operations(repo)
    go = "package layerx\n\nvar programLifecycleOrdinals = map[string]uint16{\n"
    go += "".join(f'\t"{entry["operation"]}": {entry["ordinal"]},\n' for entry in entries) + "}\n"
    go += "\nvar programLifecyclePaths = map[string]string{\n"
    go += "".join(f'\t"{entry["operation"]}": "{entry["path"]}",\n' for entry in entries) + "}\n"
    go = subprocess.run(["gofmt"], input=go, text=True, capture_output=True, check=True).stdout
    java = "package com.sidiora.layerx.sdk;\n\nimport java.util.Map;\n\nfinal class ProgramLifecycleRoutes {\n    private ProgramLifecycleRoutes() {}\n"
    java += "    static final Map<String, Integer> ORDINALS = Map.of(\n" + ",\n".join(f'        "{entry["operation"]}", {entry["ordinal"]}' for entry in entries) + ");\n"
    java += "    static final Map<String, String> PATHS = Map.of(\n" + ",\n".join(f'        "{entry["operation"]}", "{entry["path"]}"' for entry in entries) + ");\n}\n"
    swift = "enum ProgramLifecycleRoutes {\n    static let ordinals: [String: UInt16] = [\n" + "".join(f'        "{entry["operation"]}": {entry["ordinal"]},\n' for entry in entries) + "    ]\n"
    swift += "    static let paths: [String: String] = [\n" + "".join(f'        "{entry["operation"]}": "{entry["path"]}",\n' for entry in entries) + "    ]\n}\n"
    csharp = "namespace LayerX.Sdk;\n\ninternal static class ProgramLifecycleRoutes\n{\n    internal static readonly IReadOnlyDictionary<string, ushort> Ordinals = new Dictionary<string, ushort>\n    {\n"
    csharp += "".join(f'        ["{entry["operation"]}"] = {entry["ordinal"]},\n' for entry in entries) + "    };\n"
    csharp += "    internal static readonly IReadOnlyDictionary<string, string> Paths = new Dictionary<string, string>\n    {\n" + "".join(f'        ["{entry["operation"]}"] = "{entry["path"]}",\n' for entry in entries) + "    };\n}\n"
    return {
        "platform/sdk/go/program_lifecycle_generated.go": go,
        "platform/sdk/jvm/src/main/java/com/sidiora/layerx/sdk/ProgramLifecycleRoutes.java": java,
        "platform/sdk/swift/Sources/LayerXSDK/Generated/ProgramLifecycleRoutes.swift": swift,
        "platform/sdk/dotnet/Generated/ProgramLifecycleRoutes.cs": csharp,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("repo", nargs="?", default=".")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    repo = Path(args.repo).resolve()
    for relative, contents in render(repo).items():
        path = repo / relative
        if args.check:
            if not path.is_file() or path.read_text() != contents:
                raise SystemExit(f"Lifecycle SDK drift: {relative}")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(contents)


if __name__ == "__main__":
    main()
