"""Serve a directory over HTTP with caching disabled (for previewing mockups).

Usage: python tools/serve_static.py <port> <directory>
"""

import functools
import http.server
import sys


class NoCacheHandler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self) -> None:
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, *args) -> None:  # keep the console quiet
        pass


def main() -> None:
    port = int(sys.argv[1])
    directory = sys.argv[2]
    handler = functools.partial(NoCacheHandler, directory=directory)
    with http.server.ThreadingHTTPServer(("127.0.0.1", port), handler) as server:
        print(f"serving {directory} on http://127.0.0.1:{port}", flush=True)
        server.serve_forever()


if __name__ == "__main__":
    main()
