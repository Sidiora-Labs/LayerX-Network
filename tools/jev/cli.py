from __future__ import annotations

import argparse
import importlib
import sys
from pathlib import Path
from typing import Sequence

from tools.jev.checks import CHECKS
from tools.jev.client import DryRunClient, JevClient, JevConfigError, JevDryRun, JevRequestError


USAGE = "usage: python3 -m tools.jev <check> [args] --out DIR [--model M] [--log PATH] [--dry-run]"


def build_parser(name: str) -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog=f"python3 -m tools.jev {name}")
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--model")
    parser.add_argument("--log", type=Path)
    parser.add_argument("--dry-run", action="store_true")
    return parser


def main(arguments: Sequence[str]) -> int:
    if not arguments or arguments[0].startswith("-"):
        print(USAGE, file=sys.stderr)
        return 2
    name = arguments[0]
    try:
        importlib.import_module(f"tools.jev.checks.{name}")
    except ModuleNotFoundError as error:
        print(f"unknown jev check {name}: {error}", file=sys.stderr)
        return 2
    check = CHECKS.get(name)
    if check is None:
        print(f"jev check {name} did not register itself", file=sys.stderr)
        return 2
    parser = build_parser(name)
    check.add_arguments(parser)
    args = parser.parse_args(list(arguments[1:]))
    factory = DryRunClient if args.dry_run else JevClient
    client = factory(model=args.model, log_path=args.log)
    try:
        report = check.run(args, client)
    except JevDryRun as error:
        print(f"{error}", file=sys.stderr)
        return 0
    except JevConfigError as error:
        print(f"jev is not configured: {error}", file=sys.stderr)
        return 2
    except JevRequestError as error:
        print(f"jev call failed: {error}", file=sys.stderr)
        return 1
    _, md_path = report.write(args.out)
    print(md_path)
    return 0
