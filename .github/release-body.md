compid - Shazam for compiled binaries. Hand it any ELF, PE, or Mach-O
binary and it estimates how that binary was built: compiler family and
version, optimization level, architecture, OS, libc, PIE, LTO, static
linking, and hardening - ten label fields, each with a confidence score.

## Highlights

- **Self-contained binaries.** Every artifact carries the trained models
  embedded at build time - unpack and run from anywhere, no model files
  needed. `--model` still accepts a locally retrained model.
- **Ten learned fields**, held out on the 1,395-build seed corpus:
  compiler family/version, arch (x86-64, aarch64, riscv64, arm32), OS
  (linux/windows/macos), libc, PIE, static, LTO, hardening, optimization
  level. Full numbers in [docs/EVALUATION.md](docs/EVALUATION.md).
- **Function-level provenance** across all three container formats:
  symbol/export tables when present, a measured prologue + call-target
  scan for stripped x86-64 (pooled F1 0.95), and mixed-toolchain binaries
  attributed function by function.
- **Calibration you can check**: every retrain publishes a reliability
  diagram; above the default abstention threshold (0.6) buckets deliver
  70-99% observed accuracy, and the `[low confidence]` flag catches the
  overconfident band.
- **Hardened parsing**: 3,256-case deterministic mutation fuzz suite runs
  on every `cargo test`; five overflow panics found and fixed that way.

## Install

Grab the archive for your platform, unpack, and run `compid` (or
`compid.exe`). Every archive ships the binary, shell completions for
bash/zsh/fish/PowerShell, the man page, and docs.

| platform | archive |
|---|---|
| Linux x86-64 (glibc) | `compid-0.2.0-x86_64-unknown-linux-gnu.tar.gz` |
| Linux x86-64 (musl, static) | `compid-0.2.0-x86_64-unknown-linux-musl.tar.gz` |
| Linux x86-64 (32-bit) | `compid-0.2.0-i686-unknown-linux-gnu.tar.gz` |
| Linux aarch64 (glibc / musl) | `compid-0.2.0-aarch64-unknown-linux-{gnu,musl}.tar.gz` |
| Linux armv7 / armv6 (32-bit) | `compid-0.2.0-armv7-unknown-linux-gnueabihf.tar.gz` / `compid-0.2.0-arm-unknown-linux-gnueabihf.tar.gz` |
| Linux riscv64 / loongarch64 / powerpc64le / s390x | `compid-0.2.0-<triple>.tar.gz` |
| Windows x86-64 (MSVC) | `compid-0.2.0-x86_64-pc-windows-msvc.zip` |
| Windows x86-64 / arm64 (GNU) | `compid-0.2.0-x86_64-pc-windows-gnu.zip` / `compid-0.2.0-aarch64-pc-windows-gnullvm.zip` |
| Windows x86-64 (32-bit) | `compid-0.2.0-i686-pc-windows-gnu.zip` |
| macOS aarch64 (Apple silicon) | `compid-0.2.0-aarch64-apple-darwin.tar.gz` |
| macOS x86-64 (Intel) | `compid-0.2.0-x86_64-apple-darwin.tar.gz` |
| macOS universal2 | `compid-0.2.0-universal2-apple-darwin.tar.gz` |
| FreeBSD x86-64 | `compid-0.2.0-x86_64-unknown-freebsd.tar.gz` |

Verify with the combined checksums file:

```console
$ sha256sum -c sha256sums-v0.2.0.txt
```

Or build from source: `cargo install --path .` (Rust 1.75+).

```console
$ compid classify suspicious.exe
$ compid report suspicious.exe       # one rich screen: classify + why + provenance
$ compid functions libmixed.so       # per-function provenance
```

Full documentation in the [README](https://github.com/RXVEN-1907/compid#readme).
