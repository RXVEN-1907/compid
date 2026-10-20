# compid corpus

The corpus is the ground truth for everything compid predicts: C and C++
programs compiled across a matrix of toolchains, targets, optimization
levels and link modes, with every build recorded in a manifest. The
pipeline is the product — the seed matrix below is whatever this machine
could build on the day it was generated, and the dataset is meant to grow
by rerunning the pipeline as more toolchains and targets become available.

## Layout

    corpus/toolchains.py         discovers available compilers
    corpus/generate_sources.py   writes the deterministic C/C++ programs
    corpus/build_corpus.py       compiles the matrix, writes the manifest
    corpus/build_fixtures.py     builds the tiny labeled test fixtures
    corpus/sources/              generated programs (committed)
    corpus/fixture_sources/      fixture programs (committed)
    dataset/binaries/            compiled binaries (gitignored)
    dataset/manifest.csv         one row per successful build (committed)
    dataset/build_failures.csv   failed builds with compiler stderr, if any
    tests/fixtures/              tiny labeled binaries (committed)

## The source corpus

`generate_sources.py` writes 50 C programs and 8 C++ programs. Every
program is self-contained and deterministic: fixed seeds, own PRNGs, no
`rand()` or `time()`, nothing beyond libc/libm/libstdc++. Each prints a
checksum of its work so optimization cannot delete it. The idioms span
printf-family output, pseudo-random generators, hash functions (FNV-1a,
djb2, CRC-32, murmur), five sorts plus libc qsort, searching, string
handling, parsing, vectorizable array math, matrix multiply in two loop
orders, floating-point evaluation, fixed point, bit twiddling, state
machines, linked structures, binary trees, function-pointer dispatch,
varargs, dense and sparse switch tables, a cellular automaton, a rasterizer
and a stack machine. The C++ programs add iostream formatting, STL
containers and algorithms, templates, virtual dispatch and exception
handling.

One source takes a focused slice instead of the full matrix:
`export_demo.c` carries a portable export macro (`__declspec(dllexport)`
on `_WIN32`, empty elsewhere) around four small functions. Its matrix rows
are the x86_64-windows-gnu DLL builds (`-shared`, O0 and O2, build_ids
like `export_demo-zig-cc-x86_64-windows-gnu-dll-O2-def-nolto-dyn`) - the
produced PEs carry an export directory, which is what `compid functions`
reads PE function boundaries from - plus plain linux-gnu executables at
O0/O2 for both toolchains, so one seed contributes ELF-symbol-table and
PE-export-table functions side by side. It stays out of the standard
loops on purpose: the variant slices pick sources by sorted position, and
inserting one would shift every `i % 3` / `i % 4` pick and churn a
hundred existing rows for no new axis.

The set is intentionally redundant across idioms (three hash families,
several sorts, two matrix layouts) because fingerprints generalize over
families of code, not single programs.

## The build matrix

`build_corpus.py` compiles every source across:

  toolchain  gcc13 (gcc/g++ 13.3.0, native) and zig-cc (clang 19.1.7 via
             zig 0.14.1)
  target     x86_64-linux-gnu (both), x86_64-linux-musl (zig),
             aarch64-linux-gnu (zig), riscv64-linux-gnu (zig),
             riscv64-linux-musl (zig), arm-linux-musleabihf (zig),
             x86_64-windows-gnu (zig), x86_64-macos (zig),
             aarch64-macos (zig)
  opt        O0 O1 O2 O3 Os, plus Oz on zig
  pie        explicit -pie and -no-pie on native gcc glibc builds
  lto        -flto on a slice of gcc linux builds
  static     -static glibc builds on a slice; zig musl targets are static
             by default

All sources take the base axis; deterministic slices of the sorted source
list take the variants, which keeps the full run at 1395 builds and well
under half an hour on this machine. The cross targets (aarch64, riscv64,
arm) and the musl targets take the O0/O2 menu rather than the full
O0-Oz spread so the matrix stays within budget; every C source builds for
every target, and the first two C++ sources in sorted order join the
riscv64 and arm cross builds. `export_demo.c` takes the focused windows
DLL + linux exe slice described above. Every build has a 10 second timeout,
and a warmup pass first primes every zig cache flavor the plan will use
(target x optimization mode x language x link mode, the DLL link mode
included), because zig's cache keys include all of those and an unwarmed
flavor costs ~15 seconds on its first build — enough to trip the timeout.

Two labels are read back from the produced binary instead of assumed from
the command line: `pie` comes from the ELF `e_type` (ET_DYN vs ET_EXEC)
and `static` from the presence of a `PT_INTERP` segment. Windows PE files
keep `pie` and `static` at `n/a`; they still enrich the manifest and the
class balance even though extraction focuses on ELF.

## Manifest

`dataset/manifest.csv` has exactly these columns:

    build_id,source_id,toolchain,compiler_family,compiler_version,opt_level,
    arch,os,abi,pie,lto,static,path,sha256

`build_id` is filesystem-safe and encodes the full configuration, e.g.
`hello-gcc13-x86_64-linux-gnu-O2-pie-nolto-dyn`. `path` is relative to the
repository root and points into `dataset/binaries/`. `compiler_family` is
`gcc` or `clang` — zig-cc builds are labeled clang because clang is the
code generator behind the `zig cc` driver, which is exactly the distinction
compid is meant to learn. Failed builds never enter the manifest; they are
recorded with their stderr tail in `dataset/build_failures.csv`.

## Regenerating

    python3 corpus/toolchains.py          # what can build here today
    python3 corpus/generate_sources.py    # rewrite corpus/sources/
    python3 corpus/build_corpus.py        # full matrix, resumable
    python3 corpus/build_corpus.py --plan # just print the planned counts
    python3 corpus/build_fixtures.py      # refresh tests/fixtures/

The build is resumable: existing outputs are skipped, so interrupting and
rerunning continues where it stopped. `--jobs N` controls parallelism
(default 48). `--toolchain` and `--target` restrict the run.

## Adding a toolchain or target

1. Make the compiler reachable and teach `toolchains.py` to find it:
   either drop zig under `~/.cache/compid/toolchains/zig` (or point
   `COMPID_ZIG` at it) for clang-family builds, or extend `discover()`
   with a new native toolchain.
2. If it is a zig-cc target, add the triple to `ZIG_TARGETS` in
   `toolchains.py`; if it is a native toolchain, add its `targets` entry.
3. Add the axis values to the constants at the top of `build_corpus.py`
   (`ZIG_*_OPTS`, `CXX_OPTS`, or a new target block in `build_plan`) and
   give it a block in `build_plan`.
4. Rerun `generate_sources.py` (only needed if you added programs),
   `build_corpus.py`, and commit the refreshed manifest.

Sources compile with `-std=c11` (C) and `-std=c++17` (C++), so a new
toolchain only needs C11/C++17 plus libc, libm and libstdc++.

## Toolchain setup on this machine

gcc/g++ 13.3.0 came with the system. Docker is not usable here (the
docker socket is permission-blocked and there is no root), so the second
toolchain is a userspace zig install under the cache directory - a private,
owner-checked location rather than shared /tmp:

    mkdir -p ~/.cache/compid/toolchains && cd ~/.cache/compid/toolchains
    curl -LO https://ziglang.org/download/0.14.1/zig-x86_64-linux-0.14.1.tar.xz
    tar xf zig-x86_64-linux-0.14.1.tar.xz
    mv zig-x86_64-linux-0.14.1 zig

`corpus/toolchains.py` finds it there automatically. Zig gives the corpus
its clang-family builds plus cross targets (musl, aarch64, riscv64, arm,
windows) without root or distro packages. If `/tmp` is wiped, rerun the
four lines above and `build_corpus.py --skip-warmup` will rebuild anything
missing.

## Honest numbers

The seed matrix as generated on this machine (224-core Ubuntu 24.04 host):
1395 planned builds, 1395 successes, 0 failures. The original 849-build
run took about 80 seconds once compiler caches were warm; the run that
added the riscv64 and arm targets compiled the 306 fresh binaries in 7.3
seconds after a 19 second cache warmup, and the run that added
`export_demo.c` compiled its 6 focused-slice rows (2 windows DLLs, 4
linux executables) after a warmup pass that also primed the `-shared`
link-mode cache flavors, skipping the 1389 already-built outputs because
the run is resumable. The committed manifest is the
authoritative record; `dataset/build_failures.csv` exists only when a run
had failures and records the compiler stderr for each.

Per-label counts from the generated manifest:

  toolchain         gcc13=424, zig-cc=971
  compiler_family   clang=971, gcc=424
  compiler_version  13.3.0=424, 19.1.7=971
  opt_level         O0=561, O1=65, O2=581, O3=65, Os=74, Oz=49
  arch              aarch64=200, arm=102, riscv64=204, x86_64=889
  os                linux=1091, macos=204, windows=100
  abi               gnu=889, musl=302, n-a=204
  pie               n/a=100, no-pie=742, pie=553
  lto               off=1347, on=48
  static            n/a=304, no=765, yes=326

The 100 `n/a` entries are the windows PE builds (98 executables plus the
two `export_demo` DLLs), where ELF pie/static
concepts do not apply. The `abi` column is the libc family: the
arm-linux-musleabihf triple records as `musl`, with the exact triple kept
in the build_id. The `pie=no-pie` cohort is larger than the `pie`
cohort because zig defaults to non-PIE ET_EXEC even on glibc targets while
gcc on this system defaults to PIE — measured from the binaries, not
assumed, which is the interesting kind of signal for a provenance
classifier. Every new riscv64 and arm binary was verified as a valid ELF
of the right machine type (ELF64 RISC-V, ELF32 ARM EABI5), and every
manifest row's sha256 was checked against the file it points at.

Test fixtures: 38 binaries in `tests/fixtures/` (8 programs x 2 toolchains
x 2 optimization levels, names like `gcc13_O2_x86_64_hello`, plus the
pie_dynamic pair, the two committed Mach-O samples, and the committed PE
DLL `zig-cc_O2_x86_64_dll_sample` built from `corpus/sources/export_demo.c`
by `build_fixtures.py`). gcc fixtures
stay under 17 KB; six zig O0 fixtures land at 98-107 KB because zig
statically links a slice of its bundled libm into loop-containing O0
programs, trimmed with section GC.

Known limitations, stated plainly: the source set is 58 programs, small
enough that per-program identity is learnable alongside toolchain traits;
the aarch64, riscv64, arm and windows builds were verified by file type,
not executed (this host is x86_64-only without qemu user emulation wired
in); and musl appears only through zig, so its label is entangled with the
zig-cc toolchain until a second musl toolchain is added. The pipeline is
the product: rerunning it with more toolchains grows the dataset, and the
committed manifest always reflects the last full run.
