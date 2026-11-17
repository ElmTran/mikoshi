"""Generate Mikoshi's KDBX test fixtures.

TEST DATA ONLY. Every password, key file and secret in here is public.

What it does:
1. Writes the content XML files under tests/fixtures/src/ (KeePass 2 XML export
   format). These are also what you import by hand in KeePassXC / KeePass 2 to
   build the GUI fixtures (see tests/fixtures/README.md).
2. Writes the key files under tests/fixtures/keys/.
3. Uses keepassxc-cli to import the XML into .kdbx files under
   tests/fixtures/kxc/.

Usage:  python tests/fixtures/generate.py
Needs:  keepassxc-cli on PATH, or KEEPASSXC_CLI pointing at it.
"""

from __future__ import annotations

import base64
import gzip
import hashlib
import random
import struct
import sys
import uuid
import xml.etree.ElementTree as ET
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1] / "tools"))
import kxc  # noqa: E402

PASSWORD = "mikoshi-test"
NS = uuid.UUID("6d696b6f-7368-692d-6669-787475726573")  # "mikoshi-fixtures"


# ---------------------------------------------------------------- helpers


def uid(name: str) -> str:
    return base64.b64encode(uuid.uuid5(NS, name).bytes).decode()


def b64(data: bytes) -> str:
    return base64.b64encode(data).decode()


def rand_bytes(seed: int, n: int) -> bytes:
    return random.Random(seed).randbytes(n)


def png(rgb: tuple[int, int, int], size: int = 16) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    row = b"\x00" + bytes(rgb) * size
    raw = row * size
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )


def sub(parent: ET.Element, tag: str, text: str | None = None, **attrs: str) -> ET.Element:
    el = ET.SubElement(parent, tag, attrs)
    if text is not None:
        el.text = text
    return el


def times(parent: ET.Element, created: str, modified: str | None = None,
          expires: bool = False, expiry: str = "2030-01-01T00:00:00Z", usage: int = 0) -> None:
    t = sub(parent, "Times")
    sub(t, "CreationTime", created)
    sub(t, "LastModificationTime", modified or created)
    sub(t, "LastAccessTime", modified or created)
    sub(t, "ExpiryTime", expiry)
    sub(t, "Expires", "True" if expires else "False")
    sub(t, "UsageCount", str(usage))
    sub(t, "LocationChanged", created)


def string(entry: ET.Element, key: str, value: str, protected: bool = False) -> None:
    s = sub(entry, "String")
    sub(s, "Key", key)
    v = sub(s, "Value", value)
    if protected:
        v.set("ProtectInMemory", "True")


def custom_data(parent: ET.Element, items: list[tuple[str, str]], v41: bool) -> None:
    cd = sub(parent, "CustomData")
    for key, value in items:
        item = sub(cd, "Item")
        sub(item, "Key", key)
        sub(item, "Value", value)
        if v41:
            sub(item, "LastModificationTime", "2024-03-01T10:00:00Z")


# ---------------------------------------------------------------- content


class Content:
    """Builds the fixture content. level is "3.1", "4.0" or "4.1" and controls
    which format-specific features are emitted, so KeePassXC writes that
    version."""

    def __init__(self, level: str):
        self.level = level
        self.v40 = level in ("4.0", "4.1")
        self.v41 = level == "4.1"
        self.binaries: list[tuple[bytes, bool]] = []

    def binary(self, data: bytes, compressed: bool = False) -> str:
        self.binaries.append((data, compressed))
        return str(len(self.binaries) - 1)

    def entry(self, group: ET.Element, title: str, *, history_of: str | None = None,
              created: str = "2024-01-10T08:00:00Z", modified: str | None = None,
              expires: bool = False, expiry: str = "2030-01-01T00:00:00Z",
              username: str = "", password: str = "", url: str = "", notes: str = "",
              tags: str | None = None, icon: int = 0, extra=None) -> ET.Element:
        e = sub(group, "Entry")
        sub(e, "UUID", uid("entry:" + (history_of or title)))
        sub(e, "IconID", str(icon))
        if tags is not None:
            sub(e, "Tags", tags)
        times(e, created, modified, expires, expiry)
        string(e, "Title", title)
        string(e, "UserName", username)
        string(e, "Password", password, protected=True)
        string(e, "URL", url)
        string(e, "Notes", notes)
        if extra:
            extra(e)
        return e

    def group(self, parent: ET.Element, name: str, *, icon: int = 48, notes: str | None = None,
              expanded: bool = True) -> ET.Element:
        g = sub(parent, "Group")
        sub(g, "UUID", uid("group:" + name))
        sub(g, "Name", name)
        if notes is not None:
            sub(g, "Notes", notes)
        sub(g, "IconID", str(icon))
        times(g, "2024-01-01T00:00:00Z")
        sub(g, "IsExpanded", "True" if expanded else "False")
        return g

    def build(self) -> ET.ElementTree:
        root_el = ET.Element("KeePassFile")
        meta = sub(root_el, "Meta")
        root = sub(root_el, "Root")

        top = self.group(root, "Mikoshi Fixture", notes="Root group of the test fixture")

        # -------------------------------------------------- standard fields
        general = self.group(top, "General", notes="Group notes\nsecond line")
        sub(general, "DefaultAutoTypeSequence", "{USERNAME}{TAB}{PASSWORD}{ENTER}")
        if self.v41:
            sub(general, "Tags", "group-tag-1;group-tag-2")
        if self.v40:
            custom_data(general, [("mikoshi.group.key", "group value")], self.v41)

        def colors(e: ET.Element) -> None:
            sub(e, "ForegroundColor", "#FF0000")
            sub(e, "BackgroundColor", "#00FF00")
            sub(e, "OverrideURL", "cmd://notepad.exe")

        self.entry(
            general, "Full Fields", icon=12, tags="work;important",
            username="alice@example.com",
            password="P@ss<w>&'\"rd 中文 🔑",
            url="https://example.com/login?a=1&b=2",
            notes="line 1\n  indented line 2\nline 3 <xml> & \"quotes\"\n中文备注 🔒",
            extra=colors,
        )
        self.entry(general, "Expired", username="old", password="expired-pw",
                   expires=True, expiry="2020-06-30T12:00:00Z")
        self.entry(general, "Expires Later", username="new", password="future-pw",
                   expires=True, expiry="2099-12-31T23:59:59Z")

        def custom_fields(e: ET.Element) -> None:
            string(e, "Plain Field", "plain value")
            string(e, "Secret Field", "secret value", protected=True)
            string(e, "Empty Field", "")
            string(e, "Empty Secret", "", protected=True)
            string(e, "Unicode 字段 ü", "unicode key")
            string(e, "Multiline", "a\nb\nc")

        self.entry(general, "Custom Fields", username="bob", password="custom-pw",
                   extra=custom_fields)

        def padded(e: ET.Element) -> None:
            string(e, "Padded Field", "  padded value  ")
            string(e, "Spaces Only", "   ")

        self.entry(general, "Whitespace Edge", username=" leading space",
                   password=" padded secret ", url="https://example.com/trailing ",
                   notes="\n  starts with newline and ends with spaces  ", extra=padded)

        def custom_icon(e: ET.Element) -> None:
            sub(e, "CustomIconUUID", uid("icon:red"))

        self.entry(general, "Custom Icon Entry", icon=12, password="icon-pw", extra=custom_icon)

        # -------------------------------------------------- attachments
        att = self.group(top, "Attachments")
        text_id = self.binary("Attachment text 附件内容\nsecond line\n".encode("utf-8"))
        rnd_id = self.binary(rand_bytes(1, 4096))
        empty_id = self.binary(b"")
        shared_id = self.binary(b"shared attachment content")
        large_id = self.binary(rand_bytes(2, 64 * 1024))
        compressed_id = self.binary(("compressible " * 200).encode(), compressed=True)

        def attach(*pairs: tuple[str, str]):
            def f(e: ET.Element) -> None:
                for name, ref in pairs:
                    b = sub(e, "Binary")
                    sub(b, "Key", name)
                    sub(b, "Value", Ref=ref)
            return f

        self.entry(att, "Several Attachments", password="att-pw",
                   extra=attach(("note.txt", text_id), ("数据.bin", rnd_id), ("empty.txt", empty_id)))
        self.entry(att, "Shared Attachment A", extra=attach(("shared.txt", shared_id)))
        self.entry(att, "Shared Attachment B", extra=attach(("shared.txt", shared_id)))
        self.entry(att, "Large Attachment", extra=attach(("large.bin", large_id)))
        self.entry(att, "Compressed Binary", extra=attach(("compressible.txt", compressed_id)))

        # -------------------------------------------------- history
        hist = self.group(top, "History")
        old_att = self.binary(b"attachment that only exists in history")
        current = self.entry(hist, "With History", username="hist-user-v4", password="history-v4",
                             created="2024-01-01T00:00:00Z", modified="2024-04-01T00:00:00Z",
                             tags="current")
        h = sub(current, "History")

        def hist_custom(e: ET.Element) -> None:
            string(e, "Old Custom", "only in v3", protected=True)

        self.entry(h, "With History", history_of="With History", username="hist-user-v1",
                   password="history-v1", created="2024-01-01T00:00:00Z",
                   modified="2024-01-01T00:00:00Z")
        self.entry(h, "With History (renamed)", history_of="With History", username="hist-user-v2",
                   password="history-v2", created="2024-01-01T00:00:00Z",
                   modified="2024-02-01T00:00:00Z", extra=attach(("old.txt", old_att)))
        self.entry(h, "With History", history_of="With History", username="hist-user-v3",
                   password="history-v3", created="2024-01-01T00:00:00Z",
                   modified="2024-03-01T00:00:00Z", tags="old-tag", extra=hist_custom)

        # -------------------------------------------------- TOTP
        totp = self.group(top, "TOTP")

        def otp(uri: str):
            return lambda e: string(e, "otp", uri, protected=True)

        self.entry(totp, "TOTP Default", username="totp-user", password="totp-pw",
                   extra=otp("otpauth://totp/Example:alice?secret=JBSWY3DPEHPK3PXP&period=30&digits=6&issuer=Example"))
        self.entry(totp, "TOTP SHA256 8 digits",
                   extra=otp("otpauth://totp/Example:bob?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&period=60&digits=8&issuer=Example&algorithm=SHA256"))
        # edge cases seen in the wild: optional parameters left out, unpadded
        # or lower-case Base32 secrets
        self.entry(totp, "TOTP Minimal URI", extra=otp("otpauth://totp/minimal?secret=JBSWY3DPEHPK3PXP"))
        self.entry(totp, "TOTP Unpadded Secret", extra=otp("otpauth://totp/unpadded?secret=MZXW6YTBOI&period=30&digits=6"))
        self.entry(totp, "TOTP Lowercase Secret", extra=otp("otpauth://totp/lower?secret=jbswy3dpehpk3pxp&period=30&digits=6"))

        def kp2_totp(e: ET.Element) -> None:
            string(e, "TimeOtp-Secret-Base32", "JBSWY3DPEHPK3PXP", protected=True)
            string(e, "TimeOtp-Length", "8")
            string(e, "TimeOtp-Period", "60")
            string(e, "TimeOtp-Algorithm", "HMAC-SHA-256")

        self.entry(totp, "TOTP KeePass2 Native", extra=kp2_totp)

        # -------------------------------------------------- auto-type
        at = self.group(top, "Auto-Type")

        def autotype(enabled: bool, obfuscation: str, sequence: str, assoc: list[tuple[str, str]]):
            def f(e: ET.Element) -> None:
                a = sub(e, "AutoType")
                sub(a, "Enabled", "True" if enabled else "False")
                sub(a, "DataTransferObfuscation", obfuscation)
                sub(a, "DefaultSequence", sequence)
                for window, seq in assoc:
                    x = sub(a, "Association")
                    sub(x, "Window", window)
                    sub(x, "KeystrokeSequence", seq)
            return f

        self.entry(at, "AutoType Custom", username="at-user", password="at-pw",
                   extra=autotype(True, "1", "{USERNAME}{TAB}{TAB}{PASSWORD}{ENTER}", [
                       ("*Notepad*", "{PASSWORD}{ENTER}"),
                       ("Login - Example*", ""),
                       ("//^Bank .*$//", "{S:Secret Field}{DELAY 100}{ENTER}"),
                   ]))
        self.entry(at, "AutoType Disabled", password="at-off",
                   extra=autotype(False, "0", "", []))

        # -------------------------------------------------- nesting / flags
        nested = self.group(top, "Nested")
        level2 = self.group(nested, "Level 2", expanded=False)
        level3 = self.group(level2, "Level 3")
        sub(level3, "EnableAutoType", "False")
        sub(level3, "EnableSearching", "False")
        self.entry(level3, "Deep Entry", username="deep", password="deep-pw")

        # -------------------------------------------------- KDBX 4.x specifics
        if self.v40:
            v4 = self.group(top, "KDBX 4")

            def entry_cd(e: ET.Element) -> None:
                if self.v41:
                    sub(e, "QualityCheck", "False")
                    sub(e, "PreviousParentGroup", uid("group:General"))
                custom_data(e, [("KPXC_BROWSER_example", "browser data"),
                                ("mikoshi.entry.key", "entry value")], self.v41)

            self.entry(v4, "Entry CustomData", password="weak", extra=entry_cd)
            if self.v41:
                g = self.group(v4, "Group With Icon")
                sub(g, "CustomIconUUID", uid("icon:blue"))
                sub(g, "PreviousParentGroup", uid("group:Nested"))

        # -------------------------------------------------- templates
        templates = self.group(top, "Templates", icon=1)
        self.entry(templates, "Template Login", username="{USERNAME}", password="")

        # -------------------------------------------------- recycle bin
        bin_group = self.group(top, "Recycle Bin", icon=43, expanded=False)
        sub(bin_group, "EnableAutoType", "False")
        sub(bin_group, "EnableSearching", "False")

        def prev_parent(e: ET.Element) -> None:
            if self.v41:
                sub(e, "PreviousParentGroup", uid("group:General"))

        self.entry(bin_group, "Deleted Entry", username="gone", password="deleted-pw", extra=prev_parent)
        old_group = self.group(bin_group, "Old Group")
        self.entry(old_group, "Entry In Deleted Group", password="old-group-pw")

        deleted = sub(root, "DeletedObjects")
        for name, when in (("permanently-deleted-1", "2024-02-02T02:02:02Z"),
                           ("permanently-deleted-2", "2024-03-03T03:03:03Z")):
            d = sub(deleted, "DeletedObject")
            sub(d, "UUID", uid("entry:" + name))
            sub(d, "DeletionTime", when)

        # -------------------------------------------------- meta
        sub(meta, "Generator", "Mikoshi fixture generator")
        sub(meta, "DatabaseName", "Mikoshi Fixture 中文")
        sub(meta, "DatabaseNameChanged", "2024-01-01T00:00:00Z")
        sub(meta, "DatabaseDescription", "TEST DATA ONLY")
        sub(meta, "DatabaseDescriptionChanged", "2024-01-01T00:00:00Z")
        sub(meta, "DefaultUserName", "alice")
        sub(meta, "DefaultUserNameChanged", "2024-01-01T00:00:00Z")
        sub(meta, "MaintenanceHistoryDays", "30")
        sub(meta, "Color", "#3366CC")
        sub(meta, "MasterKeyChanged", "2024-01-01T00:00:00Z")
        sub(meta, "MasterKeyChangeRec", "180")
        sub(meta, "MasterKeyChangeForce", "365")
        sub(meta, "MasterKeyChangeForceOnce", "True")
        mp = sub(meta, "MemoryProtection")
        for k, v in (("ProtectTitle", "False"), ("ProtectUserName", "False"),
                     ("ProtectPassword", "True"), ("ProtectURL", "False"), ("ProtectNotes", "False")):
            sub(mp, k, v)
        icons = sub(meta, "CustomIcons")
        for name, rgb in (("red", (220, 30, 30)), ("blue", (30, 60, 220))):
            icon = sub(icons, "Icon")
            sub(icon, "UUID", uid("icon:" + name))
            sub(icon, "Data", b64(png(rgb)))
            if self.v41:
                sub(icon, "Name", name.title() + " Icon")
                sub(icon, "LastModificationTime", "2024-01-05T00:00:00Z")
        sub(meta, "RecycleBinEnabled", "True")
        sub(meta, "RecycleBinUUID", uid("group:Recycle Bin"))
        sub(meta, "RecycleBinChanged", "2024-01-01T00:00:00Z")
        sub(meta, "EntryTemplatesGroup", uid("group:Templates"))
        sub(meta, "EntryTemplatesGroupChanged", "2024-01-01T00:00:00Z")
        sub(meta, "HistoryMaxItems", "10")
        sub(meta, "HistoryMaxSize", "6291456")
        sub(meta, "LastSelectedGroup", uid("group:General"))
        sub(meta, "LastTopVisibleGroup", uid("group:Mikoshi Fixture"))
        binaries = sub(meta, "Binaries")
        for i, (data, compressed) in enumerate(self.binaries):
            payload = gzip.compress(data, mtime=0) if compressed else data
            sub(binaries, "Binary", b64(payload), ID=str(i), Compressed="True" if compressed else "False")
        custom_data(meta, [("mikoshi.meta.key", "meta value"),
                           ("mikoshi.meta.number", "1000")], self.v41)

        ET.indent(root_el, space="\t")
        return ET.ElementTree(root_el)


# ---------------------------------------------------------------- key files


def write_keys(keys: Path) -> dict[str, Path]:
    keys.mkdir(parents=True, exist_ok=True)
    out: dict[str, Path] = {}

    k = rand_bytes(10, 32)
    digest = hashlib.sha256(k).hexdigest()[:8].upper()
    hexk = k.hex().upper()
    groups = " ".join(hexk[i:i + 8] for i in range(0, 64, 8))
    lines = f"{groups[:35]}\n\t\t\t{groups[36:]}"
    out["xml-v2"] = keys / "xml-v2.keyx"
    out["xml-v2"].write_text(
        '<?xml version="1.0" encoding="utf-8"?>\n<KeyFile>\n\t<Meta>\n\t\t<Version>2.0</Version>\n\t</Meta>\n'
        f'\t<Key>\n\t\t<Data Hash="{digest}">\n\t\t\t{lines}\n\t\t</Data>\n\t</Key>\n</KeyFile>\n',
        encoding="utf-8", newline="\r\n",
    )

    out["xml-v1"] = keys / "xml-v1.key"
    out["xml-v1"].write_text(
        '<?xml version="1.0" encoding="utf-8"?>\n<KeyFile>\n\t<Meta>\n\t\t<Version>1.00</Version>\n\t</Meta>\n'
        f'\t<Key>\n\t\t<Data>{b64(rand_bytes(11, 32))}</Data>\n\t</Key>\n</KeyFile>\n',
        encoding="utf-8", newline="\r\n",
    )

    out["bin32"] = keys / "binary-32.key"
    out["bin32"].write_bytes(rand_bytes(12, 32))

    out["hex64"] = keys / "hex-64.key"
    out["hex64"].write_bytes(rand_bytes(13, 32).hex().encode())

    out["arbitrary"] = keys / "arbitrary.dat"
    out["arbitrary"].write_bytes(rand_bytes(14, 1000))
    return out


# ---------------------------------------------------------------- main


def import_db(xml: Path, target: Path, password: bool, keyfile: Path | None) -> None:
    target.unlink(missing_ok=True)
    args = ["import", "-q", "-t", "100"]
    stdin = ""
    if password:
        args.append("-p")
        stdin = f"{PASSWORD}\n{PASSWORD}\n"
    if keyfile:
        args += ["--set-key-file", str(keyfile)]
    args += [str(xml), str(target)]
    proc = kxc.run(args, stdin)
    if proc.returncode != 0 or not target.exists():
        sys.exit(f"import failed for {target.name}:\n{proc.stdout.decode()}{proc.stderr.decode()}")


def kdbx_version(path: Path) -> str:
    head = path.read_bytes()[:12]
    minor, major = struct.unpack("<HH", head[8:12])
    return f"{major}.{minor}"


def main() -> None:
    src = HERE / "src"
    src.mkdir(exist_ok=True)
    xmls = {}
    for level in ("3.1", "4.0", "4.1"):
        path = src / f"content-{level.replace('.', '')}.xml"
        Content(level).build().write(path, encoding="utf-8", xml_declaration=True)
        xmls[level] = path

    keys = write_keys(HERE / "keys")
    out = HERE / "kxc"
    out.mkdir(exist_ok=True)

    plan = [
        # (file name, content level, password?, key file)
        ("kdbx41-pw.kdbx", "4.1", True, None),
        ("kdbx41-keyxml2.kdbx", "4.1", False, "xml-v2"),
        ("kdbx41-pw-keyxml2.kdbx", "4.1", True, "xml-v2"),
        ("kdbx41-pw-arbitrary.kdbx", "4.1", True, "arbitrary"),
        ("kdbx40-pw.kdbx", "4.0", True, None),
        ("kdbx31-pw.kdbx", "3.1", True, None),
        ("kdbx31-keyxml1.kdbx", "3.1", False, "xml-v1"),
        ("kdbx31-bin32.kdbx", "3.1", False, "bin32"),
        ("kdbx31-hex64.kdbx", "3.1", False, "hex64"),
        ("kdbx31-arbitrary.kdbx", "3.1", False, "arbitrary"),
    ]
    for name, level, password, keyname in plan:
        target = out / name
        import_db(xmls[level], target, password, keys[keyname] if keyname else None)
        print(f"{name:32} KDBX {kdbx_version(target)}  (content {level})")


if __name__ == "__main__":
    main()
