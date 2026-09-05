import os
import select
import signal
import socket
import sys


signal.alarm(60)
with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
    connection.settimeout(30)
    connection.connect(sys.argv[1])
    while True:
        readable, _, _ = select.select([connection, sys.stdin.buffer], [], [], 30)
        if not readable:
            raise TimeoutError("real LNI relay deadline")
        for source in readable:
            if source is connection:
                data = connection.recv(65536)
                if not data:
                    sys.exit(0)
                sys.stdout.buffer.write(data)
                sys.stdout.buffer.flush()
            else:
                data = os.read(sys.stdin.fileno(), 65536)
                if not data:
                    sys.exit(0)
                connection.sendall(data)
