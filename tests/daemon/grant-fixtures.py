import argparse
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--encoder', required=True, type=Path)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
root = Path(__file__).resolve().parents[2]
for kind in ('capability', 'budget'):
    encoded = subprocess.run([str(args.encoder.resolve()), '--encode-' + kind],
                             check=True, capture_output=True).stdout
    target = root / 'agent/crates/layerx-crypto/tests/fixtures' / ('authority-grant-' + kind + '.bin')
    if args.check:
        assert target.read_bytes() == encoded, str(target) + ': canonical grant fixture drift'
    else:
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(encoded)
