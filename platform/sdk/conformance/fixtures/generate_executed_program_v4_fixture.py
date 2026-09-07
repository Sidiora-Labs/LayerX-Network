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
    subprocess.run([str(args.encoder.resolve()), "--stored-historical-v1",
                    str(Path(__file__).with_name("receipt-programs-positive-v1.json"))], check=True, timeout=120)
