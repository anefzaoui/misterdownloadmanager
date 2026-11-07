#!/usr/bin/env python3
"""Serve a folder on 127.0.0.1:PORT at RATE bytes per second: a release whose download takes long
enough to be cancelled half-way (Update now's end-to-end tests).

    slowserve.py DIR PORT RATE
"""
import functools
import http.server
import sys
import time

root, port, rate = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])


class Slow(http.server.SimpleHTTPRequestHandler):
    def copyfile(self, src, dst):
        while True:
            chunk = src.read(max(rate // 10, 1))
            if not chunk:
                return
            dst.write(chunk)
            time.sleep(0.1)

    def log_message(self, *a):
        sys.stderr.write("slowserve: " + (a[0] % a[1:]) + "\n")


http.server.ThreadingHTTPServer(("127.0.0.1", port),
                                functools.partial(Slow, directory=root)).serve_forever()
