#!/usr/bin/env python3
"""Build the tiny labeled test fixtures into tests/fixtures/.

Fixtures are small binaries whose ground truth is encoded in the file name,
so cargo tests and manual checks can classify them without reading a
manifest:

    <toolchain>_<opt>_<arch>_<program>

for example gcc13_O2_x86_64_hello. Each one is a short C program compiled
with both toolchains at O0 and O2 for the native glibc target and stripped;
gcc builds stay under 17 KB, zig builds under 110 KB.

Usage:
    python3 corpus/build_fixtures.py [--force]
"""

import argparse
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from toolchains import discover, by_name  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
FIXTURES = REPO / "tests" / "fixtures"
FIXTURE_SRC = REPO / "corpus" / "fixture_sources"

PROGRAMS = [
    ("hello", r"""
#include <stdio.h>
int main(void) {
    printf("compid fixture hello\n");
    return 0;
}
"""),
    ("loop", r"""
#include <stdio.h>
int main(void) {
    unsigned s = 12345, acc = 0;
    for (int i = 0; i < 512; i++) {
        s = s * 1664525u + 1013904223u;
        acc += s >> 24;
    }
    printf("fixture loop acc=%u\n", acc);
    return 0;
}
"""),
    ("switch", r"""
#include <stdio.h>
static int step(int op, int v) {
    switch (op) {
        case 0: return v + 1;
        case 1: return v * 2;
        case 2: return v - 3;
        case 3: return v ^ 0x2A;
        case 4: return v << 1;
        case 5: return v / 2;
        default: return 0;
    }
}
int main(void) {
    int acc = 0;
    for (int i = 0; i < 64; i++) acc += step(i % 7, i);
    printf("fixture switch acc=%d\n", acc);
    return 0;
}
"""),
    ("hash", r"""
#include <stdio.h>
static unsigned djb2(const char *s) {
    unsigned h = 5381u;
    while (*s) h = h * 33u ^ (unsigned char)*s++;
    return h;
}
int main(void) {
    unsigned acc = 0;
    const char *w[] = {"alpha", "beta", "gamma"};
    for (int i = 0; i < 3; i++) acc ^= djb2(w[i]);
    printf("fixture hash acc=%u\n", acc);
    return 0;
}
"""),
    ("string", r"""
#include <stdio.h>
#include <string.h>
int main(void) {
    char buf[64] = "fixture string";
    size_t n = strlen(buf);
    for (size_t i = 0; i < n / 2; i++) {
        char t = buf[i];
        buf[i] = buf[n - 1 - i];
        buf[n - 1 - i] = t;
    }
    printf("%s\n", buf);
    return 0;
}
"""),
    ("matmul", r"""
#include <stdio.h>
int main(void) {
    double a[2][2] = {{1, 2}, {3, 4}}, b[2][2] = {{2, 0}, {0, 2}}, c[2][2];
    for (int i = 0; i < 2; i++)
        for (int j = 0; j < 2; j++) {
            double s = 0;
            for (int k = 0; k < 2; k++) s += a[i][k] * b[k][j];
            c[i][j] = s;
        }
    printf("fixture matmul %.0f %.0f %.0f %.0f\n", c[0][0], c[0][1], c[1][0], c[1][1]);
    return 0;
}
"""),
    ("recursion", r"""
#include <stdio.h>
static unsigned long fib(unsigned n) {
    return n < 2 ? n : fib(n - 1) + fib(n - 2);
}
int main(void) {
    printf("fixture recursion fib=%lu\n", fib(18));
    return 0;
}
"""),
    ("statemachine", r"""
#include <stdio.h>
int main(void) {
    int state = 0, ticks = 0;
    const int pattern[3] = {1, 2, 3};
    for (int i = 0; i < 48; i++) {
        if (state == 3) { state = 0; ticks++; }
        if (i % 4 == pattern[state]) state++;
    }
    printf("fixture statemachine cycles=%d\n", ticks);
    return 0;
}
"""),
]

SIZE_CAP = 120 * 1024

# zig 0.14.1 statically links a slice of its bundled libm into O0 builds
# whose program contains a loop, which lands six of the zig fixtures around
# 216 KB. Section GC trims those to ~100 KB without changing the code the
# compiler generated, so zig fixture builds pass it; gcc fixtures do not
# need it and stay under 17 KB.
ZIG_FIXTURE_FLAGS = ["-ffunction-sections", "-fdata-sections", "-Wl,--gc-sections"]


def build_one(cmd, out_path):
    try:
        proc = subprocess.run(cmd, stdout=subprocess.DEVNULL,
                              stderr=subprocess.PIPE, timeout=60)
    except (OSError, subprocess.TimeoutExpired) as exc:
        return f"spawn/timeout: {exc}"
    if proc.returncode != 0:
        return proc.stderr.decode("utf-8", "replace")[-300:]
    return ""


def main():
    parser = argparse.ArgumentParser(description="Build tiny labeled fixtures.")
    parser.add_argument("--force", action="store_true",
                        help="rebuild even if the fixture already exists")
    args = parser.parse_args()

    tcs = by_name(discover())

    FIXTURE_SRC.mkdir(parents=True, exist_ok=True)
    FIXTURES.mkdir(parents=True, exist_ok=True)
    for name, code in PROGRAMS:
        src = FIXTURE_SRC / f"{name}.c"
        src.write_text(code.lstrip("\n"))

    plan = []
    for tc_name in ("gcc13", "zig-cc"):
        if tc_name not in tcs:
            continue
        tc = tcs[tc_name]
        for opt in ("O0", "O2"):
            for prog_name, _ in PROGRAMS:
                fixture = f"{tc_name}_{opt}_x86_64_{prog_name}"
                plan.append((tc, tc_name, opt, prog_name, fixture))

    built = skipped = failed = 0
    for tc, tc_name, opt, prog_name, fixture in plan:
        out_path = FIXTURES / fixture
        if out_path.exists() and not args.force:
            skipped += 1
            continue
        cmd = list(tc.cc) + ["-w", "-std=c11", f"-{opt}", "-o", str(out_path)]
        if tc.family == "clang":
            cmd += ["-target", "x86_64-linux-gnu", "-s"] + ZIG_FIXTURE_FLAGS
        cmd += [str(FIXTURE_SRC / f"{prog_name}.c"), "-lm"]  # -lm after source
        err = build_one(cmd, out_path)
        if err:
            print(f"FAIL {fixture}: {err}")
            failed += 1
            continue
        size = out_path.stat().st_size
        if size > SIZE_CAP:
            print(f"WARN {fixture}: {size} bytes exceeds {SIZE_CAP}")
        built += 1

    print(f"fixtures: {built} built, {skipped} skipped, {failed} failed "
          f"-> {FIXTURES}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
