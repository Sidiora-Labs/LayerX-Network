#!/usr/bin/env python3
import os
import shutil
import stat
import sys


def open_directory(path, create=False):
    absolute = os.path.abspath(path)
    if absolute == os.path.sep:
        raise ValueError("data directory must not be the filesystem root")
    descriptor = os.open(os.path.sep, os.O_RDONLY | os.O_DIRECTORY)
    try:
        for component in absolute.split(os.path.sep)[1:]:
            if create:
                try:
                    os.mkdir(component, mode=0o700, dir_fd=descriptor)
                except FileExistsError:
                    pass
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        metadata = os.fstat(descriptor)
        if metadata.st_uid != os.geteuid():
            raise ValueError("data directory must be owned by the bootstrap user")
        return absolute, descriptor
    except BaseException:
        os.close(descriptor)
        raise


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in ("prepare", "clear"):
        raise ValueError("usage: data_directory.py prepare|clear PATH")
    absolute, descriptor = open_directory(sys.argv[2], sys.argv[1] == "prepare")
    try:
        if sys.argv[1] == "prepare":
            os.fchmod(descriptor, 0o700)
            print(absolute)
        else:
            if not shutil.rmtree.avoids_symlink_attacks:
                raise ValueError("safe directory removal is unavailable")
            for entry in os.listdir(descriptor):
                metadata = os.stat(entry, dir_fd=descriptor, follow_symlinks=False)
                if stat.S_ISDIR(metadata.st_mode):
                    shutil.rmtree(entry, dir_fd=descriptor)
                else:
                    os.unlink(entry, dir_fd=descriptor)
            os.fsync(descriptor)
    finally:
        os.close(descriptor)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print(f"bootstrap data directory refused: {error}", file=sys.stderr)
        sys.exit(1)
