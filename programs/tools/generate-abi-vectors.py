#!/usr/bin/env python3
"""Generate the frozen ABI manifest vectors from the canonical Rust table."""

import argparse
import ast
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
SOURCE = ROOT / "programs/crates/layerx-programs-runtime/src/abi/manifest.rs"
OUTPUT = ROOT / "programs/crates/layerx-programs-runtime/vectors"


def manifest(source: str, version: int) -> bytes:
    match = re.search(
        rf'^pub const ABI_V{version}_MANIFEST: &str = (".*");$', source, re.MULTILINE
    )
    if match is None:
        raise ValueError(f"ABI v{version} manifest is absent")
    return version.to_bytes(2, "big") + ast.literal_eval(match.group(1)).encode()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    check = parser.parse_args().check
    source = SOURCE.read_text(encoding="utf-8")
    changed = []
    for version in (1, 2):
        path = OUTPUT / f"abi-v{version}.hex"
        generated = manifest(source, version).hex() + "\n"
        if check:
            if not path.exists() or path.read_text(encoding="ascii") != generated:
                changed.append(str(path.relative_to(ROOT)))
        else:
            path.write_text(generated, encoding="ascii")
    if changed:
        print("ABI vector drift: " + ", ".join(changed), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
