#!/usr/bin/env python3
import hashlib
import json
from pathlib import Path
import shutil
import sys


def verify(root, destination):
    packages = sorted(path.parent for path in root.glob('*/.cargo-checksum.json'))
    if not packages:
        raise ValueError('Vendored package tree is empty')
    destination.mkdir()
    for package in packages:
        if package.is_symlink() or not package.is_dir():
            raise ValueError('Vendored package must be a directory')
        manifest = json.loads((package / '.cargo-checksum.json').read_text())
        expected = manifest['files']
        if not expected:
            raise ValueError('Vendored package checksum manifest is empty')
        target = destination / package.name
        target.mkdir()
        for name, digest in sorted(expected.items()):
            relative = Path(name)
            if relative.is_absolute() or '..' in relative.parts or relative.as_posix() != name:
                raise ValueError('Vendored checksum path is not canonical')
            file = package / relative
            if file.resolve() != file.absolute() or not file.is_file():
                raise ValueError('Vendored symlinks and special files are refused')
            if hashlib.sha256(file.read_bytes()).hexdigest() != digest:
                raise ValueError(f'Vendored source checksum mismatch: {package.name}/{name}')
            output = target / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(file, output)
        shutil.copyfile(package / '.cargo-checksum.json', target / '.cargo-checksum.json')
    print(f'Verified {len(packages)} complete vendored packages')


if __name__ == '__main__':
    verify(Path(sys.argv[1]), Path(sys.argv[2]))
