import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

binary = str(Path(sys.argv[1]).resolve())
asset = 'b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898'
base = ['generate', '--network-id', '1', '--protocol-version', '3', '--asset', asset,
        '--symbol', 'LXT', '--currency', 'LXT', '--decimals', '18']


def run(args, success=True):
    result = subprocess.run([binary, *args], capture_output=True, timeout=15)
    assert (result.returncode == 0) == success, result.stderr.decode()
    if not success:
        assert result.stdout == b''
    return result.stdout


expected = {'schema_version': 2, 'assets': [{'asset': asset, 'symbol': 'LXT',
             'currency': 'LXT', 'decimals': 18}], 'modules': [
             {'module': 1, 'ordinals': list(range(1, 12))},
             {'module': 7, 'ordinals': [1, 2, 3, 5, 6]},
             {'module': 9, 'ordinals': list(range(1, 11))}]}
assert json.loads(run(base)) == expected
assert run(base) == run(base)
for option, values in {
    '--asset': ['', '0' * 64, asset.upper(), asset[:-1], 'g' * 64],
    '--symbol': ['', 'x', 'A"', 'A\n', 'A' * 33],
    '--currency': ['', '\t', 'A' * 33],
    '--decimals': ['39', '-1', '01', '1.0', '4294967296', '9999999999999999999'],
    '--network-id': ['0', '-1', '4294967296'],
    '--protocol-version': ['2', '4', '03'],
}.items():
    for value in values:
        args = base.copy()
        args[args.index(option) + 1] = value
        run(args, False)
for args in ([], base[:-1], base + ['--symbol', 'LXT'], base + ['--unknown', '1'],
             base + ['--socket', '/missing'], base + ['--custody-profile', '/missing']):
    run(args, False)
for decimals in ('0', '38'):
    args = base.copy()
    args[-1] = decimals
    assert json.loads(run(args))['assets'][0]['decimals'] == int(decimals)
with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    key = root / 'key.pem'
    subprocess.run(['openssl', 'genpkey', '-algorithm', 'ed25519', '-out', str(key)], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    public = subprocess.check_output(['openssl', 'pkey', '-in', str(key), '-pubout', '-outform', 'DER'])[-32:]
    reserve = b'system:paxeer-reserve'
    profile = (b'LXBC1' + (1).to_bytes(8, 'big') + bytes.fromhex('11' * 20)
               + bytes.fromhex('22' * 32) + public + bytes.fromhex(asset)
               + hashlib.sha256(b'LX:ACCOUNT:v1' + len(reserve).to_bytes(4, 'big') + reserve).digest()
               + (1).to_bytes(8, 'big') + bytes.fromhex('33' * 32)
               + (1).to_bytes(4, 'big') + (3).to_bytes(2, 'big'))
    path = root / 'profile'
    path.write_bytes(profile)
    args = base + ['--custody-profile', str(path)]
    expected['modules'].insert(2, {'module': 8, 'ordinals': [1]})
    assert json.loads(run(args)) == expected
    for bad in (profile[:-1], profile + b'\0', bytes(207),
                profile[:201] + (2).to_bytes(4, 'big') + profile[205:],
                profile[:97] + bytes.fromhex('44' * 32) + profile[129:]):
        path.write_bytes(bad)
        run(args, False)
    path.write_bytes(profile)
    link = root / 'link'
    link.symlink_to(path)
    run(base + ['--custody-profile', str(link)], False)
    run(['read-node', '--network-id', '1', '--protocol-version', '3',
         '--socket', str(root / 'absent.sock'), '--actor', 'did:layerx:test'], False)
print('module registry real-interface JSON, custody profile and input refusals passed')
