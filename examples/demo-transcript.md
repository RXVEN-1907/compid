# A real session

Everything below is captured output from the committed model
(`model/model.json`) against binaries from the seed dataset, run on the
machine that trained it. The one exception is `/tmp/sample`, which is a copy
of a dataset binary under a neutral name so the session reads the way you
would run it on an unknown file.

## Classify an unknown binary

    $ cp dataset/binaries/atoi_manual-gcc13-x86_64-linux-gnu-O0-nopie-nolto-dyn /tmp/sample
    $ compid classify /tmp/sample

    binary: /tmp/sample
    model:   model/model.json (trained 2026-10-20, 43896 features)
    build-id: d1294f531713bb3802b7f5a57fe5c1e6ab895e3f

    abi
      0. gnu   99.99%  ####################
      1. musl   0.01%  

    arch
      0. x86_64   99.72%  ####################
      1. riscv64   0.12%  
      2. aarch64   0.08%  

    compiler_family
      0. gcc    99.93%  ####################
      1. clang   0.07%  

    compiler_version
      0. 13.3.0  99.94%  ####################
      1. 19.1.7   0.06%  

    lto
      0. off   99.96%  ####################
      1. on     0.04%  

    opt_level
      0. O0    83.88%  #################
      1. O2     8.02%  ##
      2. Os     4.72%  #

    os
      0. linux   100.00%  ####################
      1. windows   0.00%  

    pie
      0. no-pie  79.52%  ################
      1. pie     19.77%  ####
      2. n/a      0.71%  

    static
      0. no    97.98%  ####################
      1. yes    1.76%  
      2. n/a    0.26%  

Ground truth: gcc 13.3.0, -O0, no-PIE, gnu, x86_64, Linux, dynamic. Every
field lands on the right side of the table.

## Classify a cross-compiled binary

    $ compid classify dataset/binaries/atoi_manual-zig-cc-aarch64-linux-gnu-O2-def-nolto-dyn

    binary: dataset/binaries/atoi_manual-zig-cc-aarch64-linux-gnu-O2-def-nolto-dyn
    model:   model/model.json (trained 2026-10-20, 43896 features)

    arch
      0. aarch64  89.46%  ##################
      1. riscv64   7.19%  #
      2. x86_64    2.90%  #

    compiler_family
      0. clang  99.99%  ####################
      1. gcc     0.01%  

    compiler_version
      0. 19.1.7  99.99%  ####################
      1. 13.3.0   0.01%  

    opt_level
      0. O2    98.17%  ####################
      1. Oz     0.92%  
      2. Os     0.53%  

    os
      0. linux   100.00%  ####################
      1. windows   0.00%  

    (abi, lto, pie, static elided)

Ground truth: zig `cc` (clang family), aarch64, -O2, Linux. Note `arch`
confesses its uncertainty between aarch64 and riscv64 - both are zig-built
non-x86 targets in this seed, and the model says so instead of pretending.

## Classify a Windows binary

    $ compid classify dataset/binaries/atoi_manual-zig-cc-x86_64-windows-gnu-O0-def-nolto-dyn

    binary: dataset/binaries/atoi_manual-zig-cc-x86_64-windows-gnu-O0-def-nolto-dyn
    model:   model/model.json (trained 2026-10-20, 43896 features)
    build-id: 115b962f973e997f4c4c44205044422e

    arch
      0. x86_64   99.82%  ####################
      1. arm       0.08%  
      2. riscv64   0.06%  

    compiler_family
      0. clang  99.98%  ####################
      1. gcc     0.02%  

    compiler_version
      0. 19.1.7  99.98%  ####################
      1. 13.3.0   0.02%  

    opt_level
      0. O0    85.45%  #################
      1. O2    13.67%  ###
      2. O3     0.42%  

    os
      0. windows  94.29%  ###################
      1. linux     5.71%  #

    pie
      0. n/a     86.55%  #################
      1. no-pie  12.57%  ###
      2. pie      0.88%  

Ground truth: a Windows PE build (zig `cc`, x86-64, -O0). The same source
program as the first example - different format, different OS call, and the
model calls all of it correctly, including the PE-only `n/a` rows for `pie`
and `static`.

## Ask why

    $ compid explain /tmp/sample (excerpt)

    arch: x86_64 (99.7% confidence)
      top contributing features:
        ng:000000      +2.0451  (tf=45)
        mn:mov         +0.3609  (tf=17)
        ng:000048      +0.2839  (tf=12)
        rg:rax         +0.2421  (tf=25)

    compiler_family: gcc (99.9% confidence)
      top contributing features:
        ng:000000    +1.8873  (tf=45)
        rg:rax       +0.9324  (tf=25)
        ng:488b45    +0.2376  (tf=16)
        ng:0f1efa    +0.1920  (tf=6)
        ng:f30f1e    +0.1920  (tf=6)

    os: linux (100.0% confidence)
      top contributing features:
        mn:mov         +0.2062  (tf=17)
        ng:ffffff      +0.1295  (tf=4)
        sec:.symtab    +0.0844  (tf=9)
        sec:.text      +0.0841  (tf=9)

    opt_level: O0 (83.9% confidence)
      top contributing features:
        rg:rbp     +0.6604  (tf=9)
        ng:488b45  +0.5420  (tf=16)
        ng:000000  +0.3547  (tf=45)

Each line is one feature's contribution to the winning class: weight times
normalized count. `ng:488b45` is the bytes of `mov rbp, [rbp-...]` - the
frame-pointer idiom that unoptimized gcc builds are full of - showing up as
the top O0 signal exactly where a human analyst would look first.

## Compare two binaries

    $ compid compare /tmp/sample \
        dataset/binaries/atoi_manual-zig-cc-aarch64-linux-gnu-O2-def-nolto-dyn

    cosine similarity: 0.2267
    very different; these binaries share almost no code shape

Same source program (`atoi_manual`), different toolchain, different
architecture - and the raw feature vectors barely overlap. Comparing two
builds of the same program by the same toolchain scores far higher; that gap
is the quick way to spot which of two unknown samples came from the same
build pipeline.

## Where the numbers come from

Held-out metrics for the shipped model, per label field, are in
[`docs/EVALUATION.md`](../docs/EVALUATION.md). The short version: compiler
family, version, architecture (four classes), OS (ELF vs PE), PIE, and
static classify perfectly on the seed hold-out; optimization level lands at
89% top-1 / 98% top-3; `lto` needs more LTO-built rows before it means
anything.
