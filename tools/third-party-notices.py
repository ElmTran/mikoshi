"""Writes THIRD-PARTY-NOTICES.txt: the licenses of everything Mikoshi ships.

What ships is the Rust crates linked into the program (normal dependencies of
the `mikoshi` package, for 64-bit Windows), the npm packages bundled into the
UI, and the EFF word list. Each license text is printed once, followed by the
packages that use it, so the file stays readable.

Run from the repository root whenever dependencies change:

    python -I tools/third-party-notices.py
"""

import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD-PARTY-NOTICES.txt"
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)([-._].*)?$", re.I)
NPM_PACKAGES = ["react", "react-dom", "scheduler", "@tauri-apps/api"]


def license_texts(folder: pathlib.Path) -> list[str]:
    texts = []
    for f in sorted(folder.iterdir()):
        if f.is_file() and LICENSE_FILE.match(f.name):
            try:
                texts.append(f.read_text(encoding="utf-8", errors="replace").strip())
            except OSError:
                pass
    return texts


def crates() -> list[tuple[str, str, str, list[str]]]:
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc"],
            cwd=ROOT, capture_output=True, check=True, text=True, encoding="utf-8",
        ).stdout
    )
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = next(p["id"] for p in meta["packages"] if p["name"] == "mikoshi" and p["source"] is None)
    seen, stack = set(), [root]
    while stack:
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            if any(k["kind"] is None for k in d["dep_kinds"]):
                stack.append(d["pkg"])
    out = []
    for i in seen:
        p = packages[i]
        if p["source"] is None:  # our own crates
            continue
        folder = pathlib.Path(p["manifest_path"]).parent
        out.append((p["name"], p["version"], p.get("license") or "see below", license_texts(folder)))
    return out


def npm() -> list[tuple[str, str, str, list[str]]]:
    out = []
    for name in NPM_PACKAGES:
        found = subprocess.run(
            ["node", "-p", f"require.resolve('{name}/package.json', {{ paths: [require.resolve('react-dom')] }})"],
            cwd=ROOT, capture_output=True, text=True, shell=sys.platform == "win32",
        )
        path = pathlib.Path(found.stdout.strip()) if found.returncode == 0 else None
        if not path or not path.is_file():
            sys.exit(f"cannot find the npm package {name}; run pnpm install first")
        pkg = json.loads(path.read_text(encoding="utf-8"))
        out.append((name, pkg["version"], pkg.get("license", "see below"), license_texts(path.parent)))
    return out


EFF = """The passphrase word list (eff_large_wordlist.txt) is by the Electronic
Frontier Foundation, https://www.eff.org/dice, and is used under the Creative
Commons Attribution 3.0 United States license (CC BY 3.0 US):
https://creativecommons.org/licenses/by/3.0/us/"""

MPL_NOTE = """Some Rust crates are under the Mozilla Public License 2.0. Mikoshi uses
them unmodified; their source code is available from https://crates.io under
the names and versions listed with that license below."""


def main() -> None:
    items = sorted(crates(), key=lambda x: (x[0].lower(), x[1])) + npm()
    groups: dict[str, list[str]] = {}
    no_text: list[str] = []
    for name, version, license_id, texts in items:
        label = f"{name} {version} ({license_id})"
        if not texts:
            no_text.append(label)
            continue
        groups.setdefault("\n\n".join(texts), []).append(label)

    lines = [
        "Mikoshi includes the following third-party software.",
        "",
        EFF,
        "",
        MPL_NOTE,
        "",
    ]
    if no_text:
        lines += [
            "The following packages ship without a license file; their license",
            "is the one named after each (see https://spdx.org/licenses/):",
            "",
            *[f"  {x}" for x in no_text],
            "",
        ]
    for text, users in sorted(groups.items(), key=lambda g: g[1][0].lower()):
        lines += ["=" * 78, "Used by:", *[f"  {u}" for u in users], "-" * 78, text, ""]
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{OUT.name}: {len(items)} packages, {len(groups)} distinct license texts, "
          f"{len(no_text)} without a license file, {OUT.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
