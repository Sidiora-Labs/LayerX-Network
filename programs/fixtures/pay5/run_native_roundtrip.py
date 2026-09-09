import argparse
import os
from pathlib import Path
import shlex
import subprocess
import tempfile

root = Path(__file__).resolve().parents[3]
parser = argparse.ArgumentParser()
parser.add_argument("--write-fixtures", action="store_true")
args = parser.parse_args()
stage = "generate" if args.write_fixtures else "replay"
logs = root / "qual-logs/pay5"
logs.mkdir(parents=True, exist_ok=True)
environment = os.environ.copy()
environment.update(CARGO_BUILD_JOBS="16", MAKEFLAGS="-j16")


def run(name, command, extra=None):
    log = logs / f"{name}.log"
    with log.open("wb") as output:
        result = subprocess.run(command, cwd=root, env=environment | (extra or {}),
                                stdout=output, stderr=subprocess.STDOUT, check=False)
    log.with_suffix(".exit").write_text(f"{result.returncode}\n")
    with (logs / "gates.tsv").open("a") as table:
        table.write(f"{shlex.join(command)}\t{result.returncode}\t{log.relative_to(root)}\n")
    print(f"{name}: exit {result.returncode}; {log.relative_to(root)}", flush=True)
    if result.returncode:
        raise SystemExit(result.returncode)


run(f"pay5c-native-{stage}-dependencies", ["make", "-j16", "build/liblayerx.a", "programs-build"])
(root / "build/tests").mkdir(parents=True, exist_ok=True)
run(f"pay5c-native-{stage}-build", [
    "cc", "-Iinclude", "-Ibuild/generated", "-std=c17", "-pedantic", "-Werror", "-Wall",
    "-Wextra", "-Wconversion", "-Wshadow", "-Wvla", "-fno-strict-aliasing",
    "-ffp-contract=off", "-O2", "programs/fixtures/pay5/native_roundtrip.c",
    "build/liblayerx.a", "programs/target/debug/liblayerx_programs_sandbox.a",
    "-lcrypto", "-pthread", "-ldl", "-lm", "-lsqlite3", "-o", "build/tests/pay5_native_roundtrip",
])
for case in ("token", "merchant"):
    committed = root / "programs/fixtures/pay5" / f"native-{case}"
    output = committed if args.write_fixtures else Path(tempfile.mkdtemp(prefix=f"replay-{case}-", dir=logs))
    output.mkdir(parents=True, exist_ok=True)
    if args.write_fixtures:
        for path in output.iterdir():
            if path.is_file() and path.suffix in {".activity", ".receipt", ".terminal", ".callgraph", ".registry-value"}:
                path.unlink()
    run(f"pay5c-native-{stage}-{case}", ["build/tests/pay5_native_roundtrip", f"--{case}"],
        {"PAY5_FIXTURE_OUT": str(output)})
    if not args.write_fixtures:
        expected = {path.name: path.read_bytes() for path in committed.iterdir() if path.is_file()}
        actual = {path.name: path.read_bytes() for path in output.iterdir() if path.is_file()}
        if not expected or actual != expected:
            raise SystemExit(f"{case}: native fixtures differ")
        print(f"{case}: all canonical activities, receipts and attachments match", flush=True)
