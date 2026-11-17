"""Run keepassxc-cli non-interactively.

keepassxc-cli hangs when its stdin is not redirected from a real file or pipe,
so every call goes through subprocess with explicit input.

Usage: python kxc.py [--stdin TEXT] -- <keepassxc-cli args...>
"""

import argparse
import os
import shutil
import subprocess
import sys


def find_cli() -> str:
    explicit = os.environ.get("KEEPASSXC_CLI")
    if explicit:
        return explicit
    found = shutil.which("keepassxc-cli")
    if not found:
        sys.exit("keepassxc-cli not found; set KEEPASSXC_CLI")
    return found


def run(args: list[str], stdin: str = "", timeout: int = 120) -> subprocess.CompletedProcess:
    return subprocess.run(
        [find_cli(), *args],
        input=stdin.encode("utf-8"),
        capture_output=True,
        timeout=timeout,
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--stdin", default="")
    parser.add_argument("rest", nargs=argparse.REMAINDER)
    ns = parser.parse_args()
    rest = ns.rest[1:] if ns.rest[:1] == ["--"] else ns.rest
    proc = run(rest, ns.stdin.replace("\\n", "\n"))
    sys.stdout.buffer.write(proc.stdout)
    sys.stderr.buffer.write(proc.stderr)
    sys.exit(proc.returncode)


if __name__ == "__main__":
    main()
