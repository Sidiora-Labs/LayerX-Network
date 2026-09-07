import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[4]
CASES = {
    "receipt-programs-executed-v4.json": "--dump-executed-v4",
    "receipt-programs-principal-v4.json": "--dump-principal-v4",
    "receipt-programs-mutated-leg-v4.json": "--dump-mutated-leg-v4",
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--encoder", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="terminal-v4-", dir=ROOT / "qual-logs") as directory:
        for name, flag in CASES.items():
            raw = subprocess.run([str(args.encoder.resolve()), flag], check=True, stdout=subprocess.PIPE, timeout=120)
            if len(raw.stdout) > 10_485_760:
                raise ValueError("native fixture exceeds bound")
            document = json.loads(raw.stdout)
            document["provenance"] = {
                "generator": "tests/programs/test_call_activity.c " + flag,
                "description": "Real kernel and Wasm execution with deterministic test account funding and native receipt signing; no external custody or finality claim.",
                "mutation": "Applied amount byte toggled after real execution; applied and terminal digests recomputed and receipt signed again; transfer root retained." if "mutated" in name else "none",
            }
            Path(directory, name).write_text(json.dumps(document, indent=2) + "\n")
        subprocess.run(["cargo", "test", "--locked", "--manifest-path", str(ROOT / "agent/Cargo.toml"),
                        "-p", "layerx-proof", "--test", "terminal_v4"], check=True,
                       env={**os.environ, "LAYERX_TERMINAL_FIXTURE_DIR": directory, "CARGO_BUILD_JOBS": "4"})
        for name in CASES:
            contents = Path(directory, name).read_text()
            destination = Path(__file__).with_name(name)
            if args.check:
                if destination.read_text() != contents:
                    raise ValueError("executed V4 fixture drift: " + name)
            else:
                destination.write_text(contents)


if __name__ == "__main__":
    main()
