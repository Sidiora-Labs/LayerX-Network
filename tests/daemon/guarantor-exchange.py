#!/usr/bin/env python3
import pathlib
import subprocess
import sys
import tempfile


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: guarantor-exchange.py TEST_BINARY")
    binary = str(pathlib.Path(sys.argv[1]).resolve())
    with tempfile.TemporaryDirectory(prefix="guarantor-exchange-") as directory:
        root = pathlib.Path(directory)
        def openssl(*arguments):
            subprocess.run(["openssl", *arguments], cwd=root, check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        openssl("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
                "-nodes", "-keyout", "ca.key", "-out", "ca.pem", "-days", "1",
                "-subj", "/CN=Guarantor transport test CA", "-addext", "basicConstraints=critical,CA:TRUE")
        openssl("req", "-new", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
                "-nodes", "-keyout", "peer.key", "-out", "peer.csr", "-subj", "/CN=localhost")
        (root / "extensions").write_text("subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth,clientAuth\nbasicConstraints=critical,CA:FALSE\n")
        openssl("x509", "-req", "-in", "peer.csr", "-CA", "ca.pem", "-CAkey", "ca.key",
                "-CAcreateserial", "-out", "peer.pem", "-days", "1", "-extfile", "extensions")
        subprocess.run([binary, str(root / "peer.pem"), str(root / "peer.key"), str(root / "ca.pem")], check=True)


if __name__ == "__main__":
    main()
