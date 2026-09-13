import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'data_directory.py'


class DataDirectoryTests(unittest.TestCase):
    def run_directory(self, operation, path):
        return subprocess.run([sys.executable, str(SCRIPT), operation, str(path)],
                              capture_output=True, text=True, check=False)

    def test_force_refuses_directory_and_parent_symlinks(self):
        with tempfile.TemporaryDirectory(prefix='node-data-directory-') as directory:
            root = Path(directory)
            target = root / 'target'
            target.mkdir()
            sentinel = target / 'retained'
            sentinel.write_text('retained')
            link = root / 'link'
            link.symlink_to(target, target_is_directory=True)
            for path in (link, link / 'child'):
                for operation in ('prepare', 'clear'):
                    result = self.run_directory(operation, path)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(sentinel.read_text(), 'retained')

    def test_force_removes_owned_contents_without_following_child_symlink(self):
        with tempfile.TemporaryDirectory(prefix='node-data-directory-') as directory:
            root = Path(directory)
            target = root / 'target'
            target.mkdir()
            sentinel = target / 'retained'
            sentinel.write_text('retained')
            data = root / 'data'
            self.assertEqual(self.run_directory('prepare', data).returncode, 0)
            (data / 'link').symlink_to(target, target_is_directory=True)
            (data / 'nested').mkdir()
            (data / 'nested' / 'file').write_text('discard')
            self.assertEqual(self.run_directory('clear', data).returncode, 0)
            self.assertEqual(list(data.iterdir()), [])
            self.assertEqual(sentinel.read_text(), 'retained')
            self.assertEqual(data.stat().st_mode & 0o777, 0o700)


if __name__ == '__main__':
    unittest.main()
