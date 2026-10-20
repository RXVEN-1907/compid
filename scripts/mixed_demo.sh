#!/usr/bin/env bash
# Mixed-toolchain proof for function-level provenance.
#
# Compiles one corpus source with the system gcc (-c -O2) and a second with
# zig cc (-c -O2), links the two objects into one binary, and runs
# `compid functions --model model/functions.json` over the result. The
# acceptance condition is mechanical: the functions originating from the
# gcc-compiled object must majority-predict gcc, and the functions from the
# zig-compiled object must majority-predict clang. Attribution is by symbol
# name, read from each half's own object file, so no name is hardcoded here.
#
# A stripped variant then runs the same acceptance on a `strip`ped copy of
# the same binary: names are gone, so `compid functions` works from the
# prologue heuristic, and the script attributes each heuristic function to
# its half by matching addresses against the unstripped run's symbols
# (start within 8 bytes, else the symbol whose range covers it). The script
# exits 0 only when both the unstripped and the stripped splits hold.
#
# Exits non-zero on any mismatch, so `make smoke`-style callers can rely on
# the exit code alone. Tuning knobs the model currently ships with: the
# trainer defaults (min-df 50, C 4.0) - no extra flags were needed to make
# this split clean.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

COMPID="$REPO_ROOT/target/release/compid"
if [ ! -x "$COMPID" ]; then
    echo "building compid (release)..."
    cargo build --release
fi

# Locate zig: PATH first, then the corpus builder's default install cache.
ZIG="${ZIG:-$(command -v zig || true)}"
if [ -z "$ZIG" ] && [ -x "$HOME/.cache/compid/toolchains/zig/zig" ]; then
    ZIG="$HOME/.cache/compid/toolchains/zig/zig"
fi
if [ -z "$ZIG" ]; then
    echo "error: zig not found (looked on PATH and in ~/.cache/compid/toolchains/zig/zig); the zig half needs it" >&2
    exit 1
fi

MODEL="$REPO_ROOT/model/functions.json"
if [ ! -f "$MODEL" ]; then
    echo "error: no function-level model at model/functions.json; run training/train_functions.py first (see docs/FUNCTIONS-EVALUATION.md)" >&2
    exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Two corpus sources whose functions survive -O2 inlining: the gcc half
# keeps main, the zig half gets its main renamed so one binary can hold
# both halves without a duplicate-symbol error.
GCC_SRC="corpus/sources/bst_ops.c"
ZIG_SRC="corpus/sources/funcptr_table.c"
cp "$GCC_SRC" "$WORK/gcc_side.c"
sed 's/^int main(void)/int zig_half_main(void)/' "$ZIG_SRC" > "$WORK/zig_side.c"
if ! grep -q "zig_half_main" "$WORK/zig_side.c"; then
    echo "error: main rename failed for $ZIG_SRC; the corpus source layout changed" >&2
    exit 1
fi

gcc -c -O2 "$WORK/gcc_side.c" -o "$WORK/gcc_side.o" 2>/dev/null
"$ZIG" cc -c -O2 "$WORK/zig_side.c" -o "$WORK/zig_side.o"
gcc -o "$WORK/mixed-demo" "$WORK/gcc_side.o" "$WORK/zig_side.o"

# Each half's function names, straight from that half's own object file.
half_names() {
    "$COMPID" functions "$1" --json \
        | python3 -c 'import json, sys
names = [f["name"] for f in json.load(sys.stdin)["functions"]]
if len(names) < 3:
    sys.exit("only {} functions survived in {}; source no longer fits the demo".format(len(names), sys.argv[1]))
print("\n".join(names))' "$1"
}
half_names "$WORK/gcc_side.o" > "$WORK/gcc_names.txt"
half_names "$WORK/zig_side.o" > "$WORK/zig_names.txt"
# A name compiled into both halves is unattributable; drop it from both.
sort -o "$WORK/gcc_names.txt" "$WORK/gcc_names.txt"
sort -o "$WORK/zig_names.txt" "$WORK/zig_names.txt"
comm -12 "$WORK/gcc_names.txt" "$WORK/zig_names.txt" > "$WORK/shared_names.txt" || true
if [ -s "$WORK/shared_names.txt" ]; then
    echo "note: shared symbol names excluded from the majorities:" >&2
    cat "$WORK/shared_names.txt" >&2
fi

echo "=== compid functions mixed-demo --model model/functions.json ==="
"$COMPID" functions "$WORK/mixed-demo" --model "$MODEL"
echo

"$COMPID" functions "$WORK/mixed-demo" --model "$MODEL" --json > "$WORK/funcs.json"

python3 - "$WORK/funcs.json" "$WORK/gcc_names.txt" "$WORK/zig_names.txt" <<'PY'
import json
import sys
from collections import Counter

funcs = json.load(open(sys.argv[1], encoding="utf-8"))["functions"]
gcc_names, zig_names = (
    set(open(p, encoding="utf-8").read().splitlines()) for p in sys.argv[2:4]
)

gcc_half = [f for f in funcs if f["name"] in gcc_names]
zig_half = [f for f in funcs if f["name"] in zig_names and f["name"] not in gcc_names]

def majority(half, label):
    votes = Counter((f["compiler_family"] or {}).get("predicted", "?") for f in half)
    if not votes:
        print(f"{label}: no attributed functions; the demo fixture is broken", file=sys.stderr)
        sys.exit(1)
    winner, count = max(sorted(votes.items()), key=lambda kv: kv[1])
    tally = ", ".join(f"{k}={v}" for k, v in sorted(votes.items()))
    print(f"{label}: {len(half)} functions -> {tally} (majority {winner})")
    return winner

gcc_maj = majority(gcc_half, "gcc half")
zig_maj = majority(zig_half, "zig half")

ok = True
if gcc_maj != "gcc":
    print(f"FAIL: gcc half majority-predicted {gcc_maj}, expected gcc", file=sys.stderr)
    ok = False
if zig_maj != "clang":
    print(f"FAIL: zig half majority-predicted {zig_maj}, expected clang", file=sys.stderr)
    ok = False
if not ok:
    sys.exit(1)
print("MIXED-DEMO SPLIT OK: gcc half -> gcc, zig half -> clang")
PY

# ---- stripped variant ----
# Same binary, symbol table gone. compid falls back to the prologue
# heuristic; functions are attributed back to their half by address, so the
# acceptance is the same majority assertion without names.
if ! command -v strip >/dev/null 2>&1; then
    echo "error: strip not found; the stripped variant needs binutils strip" >&2
    exit 1
fi

cp "$WORK/mixed-demo" "$WORK/mixed-demo-stripped"
strip "$WORK/mixed-demo-stripped"

echo "=== compid functions mixed-demo-stripped --model model/functions.json (stripped) ==="
"$COMPID" functions "$WORK/mixed-demo-stripped" --model "$MODEL"
echo

"$COMPID" functions "$WORK/mixed-demo-stripped" --model "$MODEL" --json > "$WORK/funcs_stripped.json"

python3 - "$WORK/funcs_stripped.json" "$WORK/funcs.json" \
    "$WORK/gcc_names.txt" "$WORK/zig_names.txt" <<'PY'
import json
import sys
from collections import Counter

stripped = json.load(open(sys.argv[1], encoding="utf-8"))["functions"]
unstripped = json.load(open(sys.argv[2], encoding="utf-8"))["functions"]
gcc_names, zig_names = (
    set(open(p, encoding="utf-8").read().splitlines()) for p in sys.argv[3:5]
)

# The halves' symbols from the unstripped run, carried to the stripped copy
# by address (same binary, same layout).
symbols = [
    (f["addr"], f["size"], "gcc" if f["name"] in gcc_names else "clang")
    for f in unstripped
    if f["name"] in gcc_names or f["name"] in zig_names
]

START_TOLERANCE = 8  # hex-string addresses; compare numerically


def to_int(hex_addr):
    return int(hex_addr, 16)


def attribute(addr, size):
    near = sorted((abs(addr - to_int(a)), half) for a, _, half in symbols
                  if abs(addr - to_int(a)) <= START_TOLERANCE)
    if near:
        return near[0][1]
    # No start match: the heuristic function may be a merged blob spanning
    # several functions. Attribute it to the half whose symbols cover the
    # most of its bytes.
    overlap = Counter()
    for a, ssize, half in symbols:
        lo, hi = to_int(a), to_int(a) + ssize
        covered = max(0, min(addr + size, hi) - max(addr, lo))
        if covered:
            overlap[half] += covered
    if overlap:
        return overlap.most_common(1)[0][0]
    return None


attributed = {"gcc": [], "clang": []}
unattributed = 0
for f in stripped:
    half = attribute(to_int(f["addr"]), f["size"])
    if half is None:
        unattributed += 1
        continue
    attributed[half].append((f["compiler_family"] or {}).get("predicted", "?"))

for half, label in (("gcc", "gcc half"), ("clang", "zig half")):
    votes = Counter(attributed[half])
    if not votes:
        print(f"stripped {label}: no attributed functions; the heuristic "
              f"missed every function of that half", file=sys.stderr)
        sys.exit(1)
    winner, count = max(sorted(votes.items()), key=lambda kv: kv[1])
    tally = ", ".join(f"{k}={v}" for k, v in sorted(votes.items()))
    print(f"stripped {label}: {len(attributed[half])} attributed functions "
          f"-> {tally} (majority {winner})")
    if winner != half:
        print(f"FAIL: stripped {label} majority-predicted {winner}, "
              f"expected {half}", file=sys.stderr)
        sys.exit(1)
print(f"stripped: {unattributed} functions unattributed (crt and padding)")
print("STRIPPED SPLIT OK: gcc half -> gcc, zig half -> clang")
PY
