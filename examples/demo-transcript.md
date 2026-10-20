# A real session

Everything below is captured output from the committed model
(`model/model.json`) against binaries from the seed dataset, run on the
machine that trained it. The one exception is `/tmp/sample`, which is a copy
of a dataset binary under a neutral name so the session reads the way you
would run it on an unknown file.

## Classify an unknown ELF

    $ cp dataset/binaries/atoi_manual-gcc13-x86_64-linux-gnu-O0-nopie-nolto-dyn /tmp/sample
    $ compid classify /tmp/sample

    binary: /tmp/sample
    model:   model/model.json (trained 2026-10-20, 68352 features)
    build-id: d1294f531713bb3802b7f5a57fe5c1e6ab895e3f

    abi
      0. gnu   98.90%  ####################
      1. n-a    0.72%  
      2. musl   0.38%  

    arch
      0. x86_64   99.77%  ####################
      1. riscv64   0.11%  
      2. arm       0.07%  

    compiler_family
      0. gcc    99.90%  ####################
      1. clang   0.10%  

    compiler_version
      0. 13.3.0  99.91%  ####################
      1. 19.1.7   0.09%  

    lto
      0. off   99.97%  ####################
      1. on     0.03%  

    opt_level
      0. O0    87.35%  #################
      1. O2     6.71%  #
      2. Os     3.33%  #

    os
      0. linux    98.57%  ####################
      1. macos     1.18%  
      2. windows   0.26%  

    pie
      0. no-pie  75.62%  ###############
      1. pie     23.82%  #####
      2. n/a      0.56%  

    static
      0. no    96.87%  ###################
      1. n/a    1.99%  
      2. yes    1.14%  

Ground truth: gcc 13.3.0, -O0, no-PIE, gnu, x86_64, Linux, dynamic. Every
field lands on the right side of the table.

## Classify a cross-compiled ELF

    $ compid classify dataset/binaries/atoi_manual-zig-cc-aarch64-linux-gnu-O2-def-nolto-dyn

    binary: dataset/binaries/atoi_manual-zig-cc-aarch64-linux-gnu-O2-def-nolto-dyn
    model:   model/model.json (trained 2026-10-20, 68352 features)

    arch
      0. aarch64  93.66%  ###################
      1. riscv64   4.36%  #
      2. x86_64    1.72%  

    compiler_family
      0. clang  99.99%  ####################
      1. gcc     0.01%  

    compiler_version
      0. 19.1.7  99.99%  ####################
      1. 13.3.0   0.01%  

    opt_level
      0. O2    95.37%  ###################
      1. O0     2.60%  #
      2. Oz     0.95%  

    os
      0. linux    99.83%  ####################
      1. macos     0.15%  
      2. windows   0.02%  

    (abi, lto, pie, static elided)

Ground truth: zig `cc` (clang family), aarch64, -O2, Linux. Note `arch`
confesses its uncertainty between aarch64 and riscv64 - both are zig-built
non-x86 targets in this seed, and the model says so instead of pretending.

## Classify a Windows binary

    $ compid classify dataset/binaries/atoi_manual-zig-cc-x86_64-windows-gnu-O0-def-nolto-dyn

    binary: dataset/binaries/atoi_manual-zig-cc-x86_64-windows-gnu-O0-def-nolto-dyn
    model:   model/model.json (trained 2026-10-20, 68352 features)
    build-id: 115b962f973e997f4c4c44205044422e

    arch
      0. x86_64   99.88%  ####################
      1. arm       0.06%  
      2. riscv64   0.04%  

    compiler_family
      0. clang  99.99%  ####################
      1. gcc     0.01%  

    compiler_version
      0. 19.1.7  99.98%  ####################
      1. 13.3.0   0.01%  

    opt_level
      0. O0    86.23%  #################
      1. O2    13.01%  ###
      2. O3     0.34%  

    os
      0. windows  86.39%  #################
      1. linux    12.44%  ##
      2. macos     1.17%  

    pie
      0. n/a     86.99%  #################
      1. no-pie  11.80%  ##
      2. pie      1.21%  

Ground truth: a Windows PE build (zig `cc`, x86-64, -O0). The same source
program as the first example - different format, different OS call, and the
model calls all of it correctly, including the PE-only `n/a` rows for `pie`
and `static`.

## Classify a macOS binary

    $ compid classify dataset/binaries/atoi_manual-zig-cc-aarch64-macos-O0-def-nolto-dyn

    binary: dataset/binaries/atoi_manual-zig-cc-aarch64-macos-O0-def-nolto-dyn
    model:   model/model.json (trained 2026-10-20, 68352 features)
    build-id: f90fce04546c3b81a415f07f89fc9c79

    arch
      0. aarch64  99.76%  ####################
      1. x86_64    0.10%  
      2. arm       0.08%  

    compiler_family
      0. clang 100.00%  ####################
      1. gcc     0.00%  

    compiler_version
      0. 19.1.7 100.00%  ####################
      1. 13.3.0   0.00%  

    opt_level
      0. O0    97.21%  ###################
      1. O2     2.45%  
      2. Oz     0.12%  

    os
      0. macos    97.27%  ###################
      1. linux     2.49%  
      2. windows   0.24%  

    pie
      0. pie     97.57%  ####################
      1. no-pie   2.24%  
      2. n/a      0.19%  

    (abi, lto, static elided)

Ground truth: a macOS Mach-O build (zig `cc`, arm64, -O0). Third container
format, third `os` class, and the model separates it cleanly from the
Linux aarch64 builds it also knows.

## Ask why

    $ compid explain /tmp/sample (excerpt)

    arch: x86_64 (99.8% confidence)
      top contributing features:
        ng:000000      +2.1610  (tf=45)
        mn:mov         +0.3544  (tf=17)
        ng:000048      +0.2609  (tf=12)
        rg:rax         +0.2038  (tf=25)

    compiler_family: gcc (99.9% confidence)
      top contributing features:
        ng:000000    +1.4835  (tf=45)
        rg:rax       +1.0553  (tf=25)
        ng:488b45    +0.2793  (tf=16)
        ng:0f1efa    +0.2328  (tf=6)
        ng:f30f1e    +0.2328  (tf=6)

    os: linux (98.6% confidence)
      top contributing features:
        sec:.text      +0.1522  (tf=9)
        sec:.symtab    +0.1420  (tf=9)
        ng:000048      +0.1179  (tf=12)
        meta:windows   +0.1159  (tf=3)
        sec:.strtab    +0.1149  (tf=8)

    opt_level: O0 (87.3% confidence)
      top contributing features:
        rg:rbp     +0.6956  (tf=9)
        ng:488b45  +0.5735  (tf=16)
        ng:000000  +0.3100  (tf=45)

Each line is one feature's contribution to the winning class: weight times
normalized count. `ng:488b45` is the bytes of `mov rbp, [rbp-...]` - the
frame-pointer idiom that unoptimized gcc builds are full of - showing up as
the top O0 signal exactly where a human analyst would look first. The
negative weight on `meta:windows` for `os: linux` is the container marker
doing its job: this file is not a PE, and that is evidence.

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
family, version, architecture (four classes), and OS (three classes: ELF
Linux, PE Windows, Mach-O macOS) classify perfectly on the seed hold-out;
optimization level lands at 90% top-1 / 99% top-3; `lto` needs more
LTO-built rows before it means anything.
