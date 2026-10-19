#!/usr/bin/env python3
"""Discover the compiler toolchains available on this machine.

The corpus build matrix is defined by what can actually compile here, so the
first step of every corpus run is discovering the toolchains and recording
exactly what they are. This module knows about two kinds of toolchains:

  gcc     the system GNU toolchain (gcc / g++), used for native builds
  zig-cc  the Zig compiler driver in cc mode, used for both native builds and
          cross-compilation (musl, aarch64, windows) without any extra packages

Zig is looked up in this order:

  1. the COMPID_ZIG environment variable (explicit operator override,
     trusted as such)
  2. whatever `zig` is on PATH
  3. ~/.cache/compid/toolchains/zig/zig (the default install location,
     XDG_CACHE_HOME aware)

Fixed filesystem candidates are accepted only when the full path chain is
owned by the invoking user (or root) and nothing on it is group- or
world-writable - and the chain is re-checked immediately before each
execution, so a swapped binary is refused on the next invocation.

For zig the reported compiler family is clang, because `zig cc` is a clang
driver: the code generator that actually emits the binary is clang, and that
is the thing compid fingerprints. The clang version is read from the driver
itself rather than assumed.

Usage:
    python3 corpus/toolchains.py            # human-readable report
    python3 corpus/toolchains.py --json     # machine-readable dump
"""

import argparse
import json
import os
import re
import shutil
import stat
import subprocess
import sys
from dataclasses import dataclass, field, asdict


TOOLCHAIN_TIMEOUT = 30


def _run(argv, timeout=TOOLCHAIN_TIMEOUT):
    """Run a command, return (ok, stdout, stderr).

    Re-validates the executable immediately before use, closing most of the
    check-to-exec window that a discovery-time-only check leaves open.
    """
    if argv and not is_trusted(argv[0]):
        return False, "", f"refusing to execute untrusted path: {argv[0]}"
    try:
        proc = subprocess.run(
            argv,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
        )
        return (
            proc.returncode == 0,
            proc.stdout.decode("utf-8", "replace"),
            proc.stderr.decode("utf-8", "replace"),
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return False, "", str(exc)


@dataclass
class Toolchain:
    """One usable compiler toolchain.

    Attributes:
        name: short id used in the manifest toolchain column (gcc13, zig-cc).
        family: code-generator family (gcc or clang), the compiler_family label.
        version: version of the underlying compiler, the compiler_version label.
        cc: argv prefix to compile/link C.
        cxx: argv prefix to compile/link C++, or "" if unavailable.
        zig: path to the zig binary when family is clang via zig, else "".
        targets: extra -target triples this toolchain can build for.
        notes: free-form detail (raw version lines) kept for honesty in reports.
    """

    name: str = ""
    family: str = ""
    version: str = ""
    cc: list = field(default_factory=list)
    cxx: list = field(default_factory=list)
    zig: str = ""
    targets: list = field(default_factory=list)
    notes: str = ""

    def argv_cc(self, target=None):
        argv = list(self.cc)
        if target and self.family == "clang":
            argv += ["-target", target]
        return argv

    def to_dict(self):
        return asdict(self)


def _gcc_version(cc):
    """Full gcc version string like '13.3.0'."""
    ok, out, _ = _run([cc, "-dumpfullversion"])
    if ok and out.strip():
        return out.strip().splitlines()[0].strip()
    ok, out, _ = _run([cc, "-dumpversion"])
    return out.strip().splitlines()[0].strip() if ok and out.strip() else "unknown"


def is_trusted(path):
    """Re-checkable trust decision for a toolchain binary.

    Every directory from the filesystem root down to the binary's parent must
    be owned by the invoking user or root and must not be group- or
    world-writable, and the binary itself must be a regular, executable file
    owned by the invoking user. The path is resolved first, so symlinked
    layouts are judged by what they point at. Call this again immediately
    before every execution: a swap between check and exec is then caught on
    the next build job rather than trusted from discovery time.
    """
    p = os.path.realpath(path)
    try:
        fst = os.lstat(p)
    except OSError:
        return False
    if not stat.S_ISREG(fst.st_mode) or not os.access(p, os.X_OK):
        return False
    # Distro toolchains are root-owned; a root-owned, non-group/world-writable
    # binary in a root-owned directory is safe, as is a user-owned one.
    if fst.st_uid not in (os.geteuid(), 0):
        return False
    if stat.S_IMODE(fst.st_mode) & 0o022:
        return False
    parent = os.path.dirname(p) or os.sep
    while True:
        try:
            dst = os.stat(parent)
        except OSError:
            return False
        if dst.st_uid not in (os.geteuid(), 0):
            return False
        if stat.S_IMODE(dst.st_mode) & 0o022:
            return False
        if parent in (os.sep, ""):
            break
        nxt = os.path.dirname(parent)
        if nxt == parent:
            break
        parent = nxt
    return True


def _default_zig_cache():
    cache = os.environ.get("XDG_CACHE_HOME") or os.path.join(
        os.path.expanduser("~"), ".cache"
    )
    return os.path.join(cache, "compid", "toolchains", "zig", "zig")


def _find_zig():
    # COMPID_ZIG is an explicit operator override and is trusted as such
    # (still required to be an executable file). Fixed filesystem locations
    # are only accepted when is_trusted() passes for the full path chain.
    env = os.environ.get("COMPID_ZIG")
    if env and os.path.isfile(env) and os.access(env, os.X_OK):
        return env
    which = shutil.which("zig")
    if which:
        return which
    if is_trusted(_default_zig_cache()):
        return _default_zig_cache()
    return None


def _zig_version(zig):
    ok, out, err = _run([zig, "version"])
    if ok and out.strip():
        return out.strip().splitlines()[0].strip()
    return "unknown"


def _zig_clang_version(zig):
    """Ask the zig cc driver which clang it runs, e.g. '19.1.5'."""
    # The driver prints its clang version on the -v banner for a real compile.
    ok, _, err = _run([zig, "cc", "-v", "-E", "-x", "c", os.devnull])
    blob = (err if err else "") + "\n" + ("" if ok else "")
    match = re.search(r"clang version (\d+\.\d+(?:\.\d+)?)", blob)
    if match:
        return match.group(1)
    ok, out, _ = _run([zig, "cc", "--version"])
    match = re.search(r"clang version (\d+\.\d+(?:\.\d+)?)", out)
    if match:
        return match.group(1)
    # Fall back to the zig version itself rather than guessing a clang number.
    return _zig_version(zig)


def _zig_cxx(zig):
    ok, _, _ = _run([zig, "c++", "--version"])
    return [zig, "c++"] if ok else []


ZIG_TARGETS = [
    "x86_64-linux-gnu",
    "x86_64-linux-musl",
    "aarch64-linux-gnu",
    "x86_64-windows-gnu",
]

# Native target for the current machine, used for plain gcc builds.
NATIVE_TARGET = "x86_64-linux-gnu"


def discover():
    """Return the list of usable Toolchain objects, sorted by name."""
    found = []

    gcc = shutil.which("gcc")
    if gcc:
        version = _gcc_version(gcc)
        gxx = shutil.which("g++")
        major = version.split(".")[0]
        found.append(
            Toolchain(
                name=f"gcc{major}",
                family="gcc",
                version=version,
                cc=[gcc],
                cxx=[gxx] if gxx else [],
                targets=[NATIVE_TARGET],
                notes=f"gcc {version} (native {NATIVE_TARGET})",
            )
        )

    zig = _find_zig()
    if zig:
        clang_version = _zig_clang_version(zig)
        found.append(
            Toolchain(
                name="zig-cc",
                family="clang",
                version=clang_version,
                cc=[zig, "cc"],
                cxx=[zig, "c++"],
                zig=zig,
                targets=list(ZIG_TARGETS),
                notes=f"zig {_zig_version(zig)} (clang {clang_version} driver)",
            )
        )

    found.sort(key=lambda t: t.name)
    return found


def by_name(toolchains):
    return {t.name: t for t in toolchains}


def report(toolchains, out=sys.stdout):
    if not toolchains:
        print("No usable toolchains found.", file=out)
        print(
            "Set COMPID_ZIG or install zig under ~/.cache/compid/toolchains/zig "
            "(see corpus/README.md), or put zig on PATH.",
            file=out,
        )
        print("and make sure gcc/g++ are on PATH.", file=out)
        return
    width = max(len(t.name) for t in toolchains)
    print(f"{'toolchain':<{width}}  {'family':<6} {'version':<10} cxx  targets", file=out)
    for t in toolchains:
        targets = ", ".join(t.targets) if t.targets else "-"
        print(
            f"{t.name:<{width}}  {t.family:<6} {t.version:<10} "
            f"{'yes' if t.cxx else 'no':<4} {targets}",
            file=out,
        )
        print(f"{'':<{width}}  notes: {t.notes}", file=out)


def main():
    parser = argparse.ArgumentParser(description="Discover available compiler toolchains.")
    parser.add_argument("--json", action="store_true", help="print JSON instead of a table")
    args = parser.parse_args()

    toolchains = discover()
    if args.json:
        print(json.dumps([t.to_dict() for t in toolchains], indent=2))
    else:
        report(toolchains)
    return 0 if toolchains else 1


if __name__ == "__main__":
    sys.exit(main())
