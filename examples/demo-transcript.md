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
    model:   model/model.json (trained 2026-10-20, 6253 features)
    build-id: d1294f531713bb3802b7f5a57fe5c1e6ab895e3f

    abi
      0. gnu   99.99%  ####################
      1. musl   0.01%  

    arch
      0. x86_64  100.00%  ####################
      1. aarch64   0.00%  

    compiler_family
      0. gcc    99.96%  ####################
      1. clang   0.04%  

    compiler_version
      0. 13.3.0  99.96%  ####################
      1. 19.1.7   0.04%  

    lto
      0. off   99.96%  ####################
      1. on     0.04%  

    opt_level
      0. O0    85.12%  #################
      1. O2     7.97%  ##
      2. Os     3.95%  #

    pie
      0. no-pie  87.97%  ##################
      1. pie     12.03%  ##

    static
      0. no    99.94%  ####################
      1. yes    0.06%  

Ground truth: gcc 13.3.0, -O0, no-PIE, gnu, x86_64, dynamic. Every field
lands on the right side of the table.

## Classify a cross-compiled binary

    $ compid classify dataset/binaries/atoi_manual-zig-cc-aarch64-linux-gnu-O2-def-nolto-dyn

    binary: dataset/binaries/atoi_manual-zig-cc-aarch64-linux-gnu-O2-def-nolto-dyn
    model:   model/model.json (trained 2026-10-20, 6253 features)

    abi
      0. gnu  100.00%  ####################
      1. musl   0.00%  

    arch
      0. aarch64  98.93%  ####################
      1. x86_64    1.07%  

    compiler_family
      0. clang  99.97%  ####################
      1. gcc     0.03%

Ground truth: zig `cc` (clang family), aarch64, -O2. `compid` has never seen
aarch64 from gcc in this seed, and the abi row reflects exactly that: the
matrix ties aarch64 to the gnu triple, so the model says gnu at full
confidence - a fair guess from the evidence it was given, and a good example
of reading the low-information fields with the seed matrix in mind.

## Ask why

    $ compid explain /tmp/sample

    binary: /tmp/sample

    abi: gnu (100.0% confidence)
      top contributing features:
        rg:rax         +0.4855  (tf=25)
        rg:rbp         +0.1431  (tf=9)
        ng:488b45      +0.1276  (tf=16)
        sec:.dynamic   +0.1192  (tf=8)
        sec:.text      +0.0970  (tf=9)
        sec:.symtab    +0.0900  (tf=9)
        sec:.shstrtab  +0.0898  (tf=8)
        sec:.dynsym    +0.0759  (tf=6)

    arch: x86_64 (100.0% confidence)
      top contributing features:
        ng:000000      +2.9837  (tf=45)
        ng:000048      +0.3751  (tf=12)
        mn:mov         +0.3703  (tf=17)
        rg:rax         +0.3056  (tf=25)
        rg:rbp         +0.1203  (tf=9)
        sec:.rela.dyn  +0.1124  (tf=5)
        ng:ffffff      +0.0737  (tf=4)

Each line is one feature's contribution to the winning class:
weight times normalized count. `ng:000000` dominating `arch` is the x86-64
null-padding pattern doing exactly what it looks like it is doing.

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
family, version, architecture, ABI, and PIE classify perfectly on the seed
hold-out; optimization level lands at 84% top-1 / 99% top-3; `lto` needs more
LTO-built rows before it means anything.
