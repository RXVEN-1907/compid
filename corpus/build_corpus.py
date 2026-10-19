#!/usr/bin/env python3
"""Build the compid source corpus across a toolchain and option matrix.

The matrix covers the axes compid predicts:

  toolchain   gcc13 (system gcc/g++) and zig-cc (clang 19 via zig)
  target      x86_64-linux-gnu, x86_64-linux-musl (zig), aarch64-linux-gnu
              (zig), x86_64-windows-gnu (zig)
  opt         O0 O1 O2 O3 Os, plus Oz for zig
  pie         explicit -pie / -no-pie (native gcc glibc builds only)
  lto         -flto on/off (gcc, linux only)
  static      a handful of -static glibc builds; musl targets are static
              by default

Builds run in parallel with a per-build timeout, skip outputs that already
exist (the run is resumable), record every failure with its compiler stderr,
and write dataset/manifest.csv with the contract columns. Labels that depend
on the produced binary (pie, static) are read back from the ELF header of the
binary itself rather than assumed from the command line, so the manifest says
what each file actually is. Windows PE files keep pie/static set to n/a; they
still enrich the manifest even though extraction focuses on ELF.

Usage:
    python3 corpus/build_corpus.py                  # full matrix
    python3 corpus/build_corpus.py --plan           # print planned counts
    python3 corpus/build_corpus.py --jobs 64        # more workers
    python3 corpus/build_corpus.py --toolchain gcc13
"""

import argparse
import concurrent.futures
import csv
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from toolchains import discover, by_name, is_trusted  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
SOURCES_DIR = REPO / "corpus" / "sources"
DATASET = REPO / "dataset"
BINARIES = DATASET / "binaries"
MANIFEST = DATASET / "manifest.csv"
FAILURES = DATASET / "build_failures.csv"

BUILD_TIMEOUT = 10  # seconds, per build

MANIFEST_COLUMNS = [
    "build_id", "source_id", "toolchain", "compiler_family", "compiler_version",
    "opt_level", "arch", "os", "abi", "pie", "lto", "static", "path", "sha256",
]
FAILURE_COLUMNS = ["build_id", "toolchain", "target", "reason", "stderr_tail"]

# Matrix shape. Subsetting keeps the run under ~25 minutes while still
# covering every axis: all sources take the base axis, slices of the source
# list (deterministic, by sorted position) take the variants.
GCC_BASE_OPTS = ["O0", "O1", "O2", "O3", "Os"]
GCC_NOPIE_OPTS = ["O0", "O2", "Os"]
GCC_LTO_OPTS = ["O1", "O2", "O3"]
GCC_STATIC_OPTS = ["O0", "O2"]
CXX_OPTS = ["O0", "O2", "Os"]
ZIG_GNU_OPTS = ["O0", "O2", "Oz"]
ZIG_MUSL_OPTS = ["O0", "O2"]
ZIG_AARCH64_OPTS = ["O0", "O2"]
ZIG_WINDOWS_OPTS = ["O0", "O2"]
ZIG_CXX_OPTS = ["O0", "O2"]

WARN_FLAGS = ["-w"]
MATH_FLAGS = ["-lm"]


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def inspect_elf(path):
    """Return (pie, static) read from the binary: ('pie'|'no-pie'|'n/a', ...)."""
    try:
        with open(path, "rb") as f:
            head = f.read(4096)
    except OSError:
        return "n/a", "n/a"
    if len(head) < 64 or head[:2] == b"MZ":
        return "n/a", "n/a"
    if head[:4] != b"\x7fELF":
        return "n/a", "n/a"
    e_type = int.from_bytes(head[16:18], "little")
    e_phoff = int.from_bytes(head[32:40], "little")
    e_phentsize = int.from_bytes(head[54:56], "little")
    e_phnum = int.from_bytes(head[56:58], "little")
    pie = "pie" if e_type == 3 else ("no-pie" if e_type == 2 else "n/a")
    has_interp = False
    if e_phoff and e_phentsize and e_phnum:
        for i in range(e_phnum):
            off = e_phoff + i * e_phentsize
            if off + 4 <= len(head):
                if int.from_bytes(head[off:off + 4], "little") == 3:  # PT_INTERP
                    has_interp = True
                    break
    static = "no" if has_interp else "yes"
    return pie, static


def source_files():
    """Sorted (source_id, path, lang) for every corpus source."""
    out = []
    if not SOURCES_DIR.is_dir():
        return out
    for p in sorted(SOURCES_DIR.iterdir()):
        if p.suffix == ".c":
            out.append((p.stem, p, "c"))
        elif p.suffix == ".cpp":
            out.append((p.stem, p, "cxx"))
    return out


def target_labels(target):
    arch, rest = target.split("-", 1)
    osname, abi = rest.split("-", 1)
    return arch, osname, abi


def build_plan(toolchains):
    """Yield job dicts for the whole matrix. Deterministic order."""
    tcs = by_name(toolchains)
    sources = source_files()
    c_srcs = [(sid, path) for sid, path, lang in sources if lang == "c"]
    cxx_srcs = [(sid, path, lang) for sid, path, lang in sources if lang == "cxx"]
    jobs = []

    def add(tc_name, target, opt, pie, lto, static, src_path, lang):
        tc = tcs[tc_name]
        arch, osname, abi = target_labels(target)
        src_id = src_path.stem
        tags = {
            "pie": {"pie": "pie", "no-pie": "nopie"}.get(pie, "def"),
            "lto": "lto" if lto == "on" else "nolto",
            "static": "st" if static == "static" else "dyn",
        }
        build_id = f"{src_id}-{tc_name}-{target}-{opt}-{tags['pie']}-{tags['lto']}-{tags['static']}"
        out_path = BINARIES / build_id
        if tc.family == "clang":
            cc_prefix = list(tc.cc if lang == "c" else tc.cxx)
        else:
            cc_prefix = list(tc.cc if lang == "c" else tc.cxx)
        cmd = cc_prefix + WARN_FLAGS
        if tc.family == "clang":
            cmd += ["-target", target]
        else:
            if pie == "pie":
                cmd += ["-pie"]
            elif pie == "no-pie":
                cmd += ["-no-pie"]
        cmd += ["-std=c11" if lang == "c" else "-std=c++17", "-" + opt]
        if lto == "on":
            cmd += ["-flto"]
        if static == "static":
            cmd += ["-static"]
        # -lm goes after the source: the linker resolves libraries in order,
        # so libm must come once the objects that need it are on the line.
        cmd += ["-o", str(out_path), str(src_path)] + MATH_FLAGS
        jobs.append({
            "build_id": build_id,
            "source_id": src_id,
            "source_path": src_path,
            "lang": lang,
            "toolchain": tc_name,
            "target": target,
            "opt": opt,
            "pie_cmd": pie,
            "lto": lto,
            "static_cmd": static,
            "arch": arch,
            "os": osname,
            "abi": abi,
            "cmd": cmd,
            "out_path": out_path,
        })

    if "gcc13" in tcs:
        for i, (sid, path) in enumerate(c_srcs):
            for opt in GCC_BASE_OPTS:
                add("gcc13", "x86_64-linux-gnu", opt, "pie", "off", "dyn", path, "c")
            if i % 3 == 0:
                for opt in GCC_NOPIE_OPTS:
                    add("gcc13", "x86_64-linux-gnu", opt, "no-pie", "off", "dyn", path, "c")
            if i % 3 == 1:
                for opt in GCC_LTO_OPTS:
                    add("gcc13", "x86_64-linux-gnu", opt, "pie", "on", "dyn", path, "c")
            if i % 4 == 2:
                for opt in GCC_STATIC_OPTS:
                    add("gcc13", "x86_64-linux-gnu", opt, "no-pie", "off", "static", path, "c")
        for sid, path, lang in cxx_srcs:
            for opt in CXX_OPTS:
                add("gcc13", "x86_64-linux-gnu", opt, "pie", "off", "dyn", path, lang)

    if "zig-cc" in tcs:
        for sid, path in c_srcs:
            for opt in ZIG_GNU_OPTS:
                add("zig-cc", "x86_64-linux-gnu", opt, "default", "off", "dyn", path, "c")
            for opt in ZIG_MUSL_OPTS:
                add("zig-cc", "x86_64-linux-musl", opt, "default", "off", "static", path, "c")
            for opt in ZIG_AARCH64_OPTS:
                add("zig-cc", "aarch64-linux-gnu", opt, "default", "off", "dyn", path, "c")
            for opt in ZIG_WINDOWS_OPTS:
                add("zig-cc", "x86_64-windows-gnu", opt, "default", "off", "dyn", path, "c")
        for sid, path, lang in cxx_srcs:
            for opt in ZIG_CXX_OPTS:
                add("zig-cc", "x86_64-linux-gnu", opt, "default", "off", "dyn", path, lang)

    jobs.sort(key=lambda j: j["build_id"])
    return jobs


def warmup(toolchains, jobs):
    """Warm every distinct compiler-cache flavor the plan will actually use.

    zig's cache key includes the target, the optimization mode, the language
    and link mode, so warming only -O2 would leave the first -O0 build for
    each target paying a one-time ~15 s cost and tripping the per-build
    timeout. gcc has no cache, so only clang-driver jobs are warmed. Flavors
    are distinct cache entries and warm safely in a small pool.
    """
    tcs = by_name(toolchains)
    # Private per-run scratch area (mode 0700, random name): warmup sources and
    # outputs are only ever written by and read by this process, and a hostile
    # pre-existing /tmp path can neither be followed into nor injected into.
    warm_dir = Path(tempfile.mkdtemp(prefix="compid-warmup-"))
    src = warm_dir / "warm.c"
    src.write_text('#include <stdio.h>\nint main(void){printf("warm\\n");return 0;}\n')
    srccxx = warm_dir / "warm.cpp"
    srccxx.write_text('#include <cstdio>\nint main(){std::printf("warm\\n");}\n')

    flavors = {}
    for j in jobs:
        tc = tcs[j["toolchain"]]
        if tc.family != "clang":
            continue  # gcc is stateless; nothing to warm
        key = (j["toolchain"], j["lang"], j["target"], j["opt"], j["static_cmd"])
        if key in flavors:
            continue
        out = warm_dir / ("w_%s_%s_%s" % (j["lang"], j["target"], j["opt"]))
        cmd = list(tc.cc if j["lang"] == "c" else tc.cxx)
        cmd += ["-w", "-target", j["target"]]
        cmd += ["-std=c11" if j["lang"] == "c" else "-std=c++17", "-" + j["opt"]]
        if j["static_cmd"] == "static":
            cmd += ["-static"]
        cmd += ["-o", str(out), str(src if j["lang"] == "c" else srccxx), "-lm"]
        flavors[key] = cmd

    if not flavors:
        shutil.rmtree(warm_dir, ignore_errors=True)
        return
    print(f"warming {len(flavors)} zig cache flavors (parallel, patient timeout)...")
    t0 = time.time()

    def once(argv):
        try:
            subprocess.run(argv, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, timeout=300, check=False)
        except (OSError, subprocess.TimeoutExpired):
            pass

    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        list(pool.map(once, flavors.values()))
    print(f"  warmup done in {time.time() - t0:.1f}s")
    shutil.rmtree(warm_dir, ignore_errors=True)


def run_job(job):
    """Run one build. Returns ('ok'|'skip'|'fail', job, reason, stderr_tail)."""
    out_path = job["out_path"]
    if out_path.exists() and out_path.stat().st_size > 0:
        return "skip", job, "already built", ""
    if not is_trusted(job["cmd"][0]):
        return "fail", job, "toolchain binary failed trust re-check", ""
    try:
        proc = subprocess.run(
            job["cmd"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            timeout=BUILD_TIMEOUT,
        )
        ok = proc.returncode == 0
        tail = proc.stderr.decode("utf-8", "replace")[-400:]
        reason = "" if ok else f"exit {proc.returncode}"
    except subprocess.TimeoutExpired as exc:
        ok = False
        tail = (exc.stderr or b"").decode("utf-8", "replace")[-400:] if exc.stderr else ""
        reason = f"timeout after {BUILD_TIMEOUT}s"
    except OSError as exc:
        ok = False
        tail = str(exc)
        reason = "spawn error"
    if ok and out_path.exists() and out_path.stat().st_size > 0:
        return "ok", job, "", tail
    return "fail", job, reason or "no output", tail


def summarize(manifest_rows):
    """Print the per-label count summary over the manifest."""
    from collections import Counter
    combo = Counter()
    per_field = {}
    for row in manifest_rows:
        combo[(row["compiler_family"], row["opt_level"], row["arch"])] += 1
        for field in ("toolchain", "compiler_family", "compiler_version",
                      "opt_level", "arch", "os", "abi", "pie", "lto", "static"):
            per_field.setdefault(field, Counter())[row[field]] += 1

    print("\n=== corpus summary: compiler_family x opt_level x arch ===")
    total = sum(combo.values())
    for key in sorted(combo):
        print(f"  {key[0]:<6} {key[1]:<4} {key[2]:<9} {combo[key]:>5}")
    print(f"  {'TOTAL':<19} {total:>5}")
    print("\n=== per-label counts ===")
    for field, counter in per_field.items():
        parts = ", ".join(f"{k}={v}" for k, v in sorted(counter.items()))
        print(f"  {field:<17} {parts}")


def main():
    parser = argparse.ArgumentParser(
        description="Build the corpus matrix and write dataset/manifest.csv."
    )
    parser.add_argument("--jobs", type=int, default=min(48, os.cpu_count() or 8),
                        help="parallel build workers (default 48)")
    parser.add_argument("--plan", action="store_true",
                        help="print the planned matrix and exit")
    parser.add_argument("--toolchain", action="append",
                        help="restrict to a toolchain name (repeatable)")
    parser.add_argument("--target", action="append",
                        help="restrict to a target triple (repeatable)")
    parser.add_argument("--skip-warmup", action="store_true",
                        help="skip the serial cache warmup pass")
    args = parser.parse_args()

    toolchains = discover()
    if args.toolchain:
        toolchains = [t for t in toolchains if t.name in set(args.toolchain)]
    if not toolchains:
        print("no usable toolchains; run corpus/toolchains.py to debug", file=sys.stderr)
        return 1
    jobs = build_plan(toolchains)
    if args.target:
        keep = set(args.target)
        jobs = [j for j in jobs if j["target"] in keep]
    if not jobs:
        print("no jobs planned (check --toolchain/--target filters)", file=sys.stderr)
        return 1

    BINARIES.mkdir(parents=True, exist_ok=True)

    if args.plan:
        from collections import Counter
        c = Counter((j["toolchain"], j["target"], j["opt"], j["pie_cmd"],
                     j["lto"], j["static_cmd"]) for j in jobs)
        for key in sorted(c):
            print(f"  {key[0]:<8} {key[1]:<22} -{key[2]:<3} pie={key[3]:<8} "
                  f"lto={key[4]:<4} static={key[5]:<7} x{c[key]}")
        print(f"planned builds: {len(jobs)}")
        return 0

    if not args.skip_warmup:
        warmup(toolchains, jobs)

    existing = sum(1 for j in jobs if j["out_path"].exists())
    fresh = len(jobs) - existing
    print(f"matrix: {len(jobs)} jobs ({existing} already built, {fresh} to build), "
          f"{args.jobs} workers")

    rows = []
    failures = []
    t0 = time.time()

    def execute(pass_jobs, pass_no):
        """Run a pool pass, return (rows, failures) and the jobs that failed."""
        out_rows = []
        out_failures = []
        failed_jobs = []
        counter = [0]
        plabel = f"pass {pass_no}"

        def track(_):
            counter[0] += 1
            if counter[0] % 100 == 0 or counter[0] == len(pass_jobs):
                print(f"  [{plabel}] {counter[0]}/{len(pass_jobs)} jobs done "
                      f"({time.time() - t0:.1f}s)")

        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
            futures = {pool.submit(run_job, j): j for j in pass_jobs}
            for fut in concurrent.futures.as_completed(futures):
                job = futures[fut]
                status, job, reason, tail = fut.result()
                track(0)
                if status == "fail":
                    failed_jobs.append(job)
                    out_failures.append({
                        "build_id": job["build_id"],
                        "toolchain": job["toolchain"],
                        "target": job["target"],
                        "reason": reason,
                        "stderr_tail": " | ".join(tail.splitlines()[-3:]),
                    })
                    continue
                pie, static = inspect_elf(job["out_path"])
                out_rows.append({
                    "build_id": job["build_id"],
                    "source_id": job["source_id"],
                    "toolchain": job["toolchain"],
                    "compiler_family": by_name(toolchains)[job["toolchain"]].family,
                    "compiler_version": by_name(toolchains)[job["toolchain"]].version,
                    "opt_level": job["opt"],
                    "arch": job["arch"],
                    "os": job["os"],
                    "abi": job["abi"],
                    "pie": pie,
                    "lto": job["lto"],
                    "static": static,
                    "path": str(job["out_path"].relative_to(REPO)),
                    "sha256": sha256_file(job["out_path"]),
                })
        return out_rows, out_failures, failed_jobs

    pending = jobs
    for attempt in (1, 2):
        pass_rows, pass_failures, failed_jobs = execute(pending, attempt)
        rows.extend(pass_rows)
        failures = pass_failures
        pending = failed_jobs
        if not pending:
            break
        print(f"retrying {len(pending)} failed builds once (cold caches settle "
              f"after the first pass)...")

    rows.sort(key=lambda r: r["build_id"])

    # Sweep sidecars zig leaves for PE builds (*.pdb) and any other file in
    # dataset/binaries that no manifest row owns, so the directory stays
    # exactly one file per build_id.
    known = {r["build_id"] for r in rows}
    swept = 0
    for p in BINARIES.iterdir():
        if p.name not in known:
            p.unlink()
            swept += 1
    if swept:
        print(f"swept {swept} untracked files from dataset/binaries (zig pdb sidecars)")

    with open(MANIFEST, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=MANIFEST_COLUMNS)
        writer.writeheader()
        writer.writerows(rows)

    if failures:
        failures.sort(key=lambda r: r["build_id"])
        with open(FAILURES, "w", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=FAILURE_COLUMNS)
            writer.writeheader()
            writer.writerows(failures)
    elif FAILURES.exists():
        FAILURES.unlink()

    elapsed = time.time() - t0
    print(f"\nwrote {MANIFEST} ({len(rows)} rows) in {elapsed:.1f}s")
    if failures:
        from collections import Counter
        reasons = Counter(fl["reason"].split(" ")[0] for fl in failures)
        print(f"failures: {len(failures)} ({dict(reasons)}); details in {FAILURES}")
    else:
        print("failures: 0")

    summarize(rows)
    return 0


if __name__ == "__main__":
    sys.exit(main())
