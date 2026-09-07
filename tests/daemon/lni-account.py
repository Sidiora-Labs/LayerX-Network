import json
from pathlib import Path
import subprocess
import sys

fixture = json.loads(Path('platform/hosted/gateway/tests/fixtures/maintained-authority.json').read_text())
maintenance = bytes.fromhex(fixture['authority']['batch_evidence']['batch_identity']['receipt_hex'])
subprocess.run([sys.argv[1]], input=maintenance, check=True)
