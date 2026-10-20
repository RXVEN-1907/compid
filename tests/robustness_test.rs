//! Deterministic mutation fuzzing of the extractor's parsing path.
//!
//! compid eats untrusted bytes: the container parsers (`src/elfinfo.rs`,
//! `src/peinfo.rs`, `src/machoinfo.rs` via goblin) and the function
//! boundary scanner (`src/functions.rs`) all read offsets, sizes, counts,
//! and addresses straight out of the file. This suite is the in-tree answer
//! to the roadmap's "continuous fuzzing of the extractor's parsing path"
//! item: a seeded, mutation-based fuzzer with no external crates and no
//! nightly dependency, run on every `cargo test`.
//!
//! # What a case is
//!
//! One case takes one committed fixture from `tests/fixtures/`, applies one
//! deterministic mutation (bit flips, byte substitutions including magic
//! corruption, truncation, duplicated/appended section-header tables,
//! inflated length fields, header-field corruption, endianness flips), and
//! pushes the result through the full user-facing surface:
//!
//! - [`compid::extract_binary`] (feature extraction),
//! - [`compid::extract_binary_functions`] (function enumeration, including
//!   the stripped-binary prologue heuristic),
//! - the classify path on whatever features came out: the model-free
//!   cosine compare that `compid compare` uses, plus a real
//!   `model/model.json` predict when the committed model is present.
//!
//! Every path runs under [`std::panic::catch_unwind`]. The assertions: no
//! path panics (any panic is a failure with full repro info, except a
//! small documented allowlist for panics that live inside goblin and
//! cannot be fixed here), the outcome is `Ok` or a graceful `Err` (never a
//! hang - each case runs on its own thread with a hard timeout), and the
//! documented invariants hold (`meta:` keys present on a successful
//! extract; `functions` empty exactly when `reason` is set).
//!
//! # Determinism
//!
//! The mutation stream is a pure function of pinned inputs:
//!
//! ```text
//! case seed = fnv1a-style fold of (FUZZ_SEED, fixture file name),
//!             splitmix64-finalized with the case index
//! operator  = case index % OP_COUNT
//! parameters= xorshift64 stream seeded with the case seed
//! ```
//!
//! Same seed, same fixture tree, same mutations, same outcomes - every
//! run, on every machine, in any worker-parallel order (workers only split
//! up the case list; per-case randomness never depends on scheduling).
//! Raising `MUTATIONS_PER_FIXTURE` extends each fixture's stream without
//! shifting its first cases, because the seed does not depend on the
//! count.
//!
//! # Reproducing a single case
//!
//! ```text
//! COMPID_FUZZ_FIXTURE=gcc13_O2_x86_64_pie_dynamic_stripped \
//! COMPID_FUZZ_CASE=42 \
//! cargo test --test robustness_test -- --ignored repro_single_case --nocapture
//! ```
//!
//! prints the operator, the seed, and both paths' outcomes for that one
//! case.
//!
//! # Extending the corpus
//!
//! Drop any ELF, PE, or Mach-O file into `tests/fixtures/` and it joins
//! the sweep automatically (every regular file is a fixture). Because the
//! per-case seed folds in the file *name*, adding a fixture never changes
//! the mutations the existing fixtures receive. Prefer small, structurally
//! distinct binaries (static, dynamic, stripped, non-x86-64, relocatable
//! objects) - the mutation operators only get interesting when there is a
//! header table to corrupt.
//!
//! # Time budget
//!
//! The full sweep must finish in under 60 seconds wall clock (asserted),
//! with a 30-second hard timeout per case. `[profile.test]` sets
//! opt-level 1 with debug assertions and overflow checks ON for exactly
//! this reason; see the comment in `Cargo.toml`.

use compid::model::Model;
use compid::{extract_binary, extract_binary_functions};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Fixed seed of the whole mutation stream. Any nonzero 64-bit constant
/// works; this one is the ASCII of "COMPID11" and is pinned so the suite is
/// bit-for-bit reproducible. Changing it changes every mutation below: do
/// that only to deliberately explore a new input space, and expect the
/// panic/allowlist picture to change with it.
const FUZZ_SEED: u64 = 0x434F_4D50_4944_3131; // "COMPID11", nonzero by construction

/// Mutations generated per fixture. 37 committed fixtures x 88 = 3256
/// cases, comfortably above the 3000-case bar; raise it to widen the
/// sweep (earlier cases keep their meaning - see the module docs).
const MUTATIONS_PER_FIXTURE: usize = 88;

/// Lower bound on total cases the sweep must run. If you shrink the
/// fixture tree or `MUTATIONS_PER_FIXTURE` below what satisfies this, the
/// sweep fails loudly instead of quietly narrowing coverage.
const MIN_TOTAL_CASES: usize = 3000;

/// Hard per-case timeout. A genuine hang is infinite, so 30 s cannot
/// false-positive on a healthy case; a case that outlives this fails the
/// suite as a hang. (The runner thread is leaked on timeout - std has no
/// thread kill - and reaped at process exit.)
const CASE_HARD_TIMEOUT: Duration = Duration::from_secs(30);

/// Wall-clock budget for the whole sweep, asserted at the end.
const SUITE_TIME_BUDGET: Duration = Duration::from_secs(60);

/// Operator count; case `i` runs operator `i % OP_COUNT`.
const OP_COUNT: usize = 10;

const OP_NAMES: [&str; OP_COUNT] = [
    "single_bit_flip",
    "multi_bit_flips",
    "byte_substitutions",
    "magic_corruption",
    "truncation",
    "append_header_table",
    "inflate_length_fields",
    "header_field_corruption",
    "endianness_flip",
    "mixed_byte_damage",
];

/// Panics that live inside the goblin dependency itself and cannot be fixed
/// in this tree. Each entry is `(fixture file name, case index, payload
/// substring, why)`; a panic is allowed only when fixture, case index, and
/// payload all match, so a fresh overflow anywhere else - in compid's own
/// code or in goblin on a different input - still fails the sweep.
const KNOWN_UPSTREAM_PANICS: &[(&str, usize, &str, &str)] = &[(
    "macho_aarch64_sample",
    16,
    "attempt to add with overflow",
    "goblin 0.10.7's MachO::parse (src/mach/mod.rs:289) computes the LC_MAIN \
     entrypoint as `vmaddr - fileoff + offset` in plain u64 arithmetic; this \
     case inflates a Mach-O segment's length fields so that sum overflows and \
     goblin panics inside its own parser, before compid sees the object. \
     compid cannot patch the dependency from here: the catch_unwind at the \
     call boundary contains it and the parse surfaces as a graceful Err.",
)];

// ---------------------------------------------------------------------------
// PRNG
// ---------------------------------------------------------------------------

/// Seeded xorshift64 (Marsaglia): `x ^= x<<13; x ^= x>>7; x ^= x<<17`.
/// Period 2^64-1, trivially auditable, no crate. A zero state would kill
/// the stream, so a zero seed is remapped.
struct Xorshift64(u64);

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Xorshift64(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform-ish value in `0..n` (`0` when `n == 0`). Modulo bias is
    /// irrelevant for mutation parameters.
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    fn byte(&mut self) -> u8 {
        (self.next_u64() >> 24) as u8
    }
}

/// Per-case seed: FNV-1a-style fold of the fixture file name over
/// [`FUZZ_SEED`], then the case index folded in and finished with one
/// splitmix64 round. Folding the *name* (not the directory position) means
/// adding a fixture never shifts the streams of the existing ones.
fn case_seed(fixture_name: &str, case_index: usize) -> u64 {
    let mut z = FUZZ_SEED;
    for &b in fixture_name.as_bytes() {
        z = (z ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
    }
    z ^= case_index as u64;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

// ---------------------------------------------------------------------------
// Bounds-checked little-endian field helpers. The fuzzer must never panic
// while trying to corrupt something.
// ---------------------------------------------------------------------------

fn get_u16(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
}

fn get_u32(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
}

fn get_u64(d: &[u8], at: usize) -> Option<u64> {
    d.get(at..at + 8)
        .and_then(|s| s.try_into().ok())
        .map(u64::from_le_bytes)
}

fn put_u16(d: &mut [u8], at: usize, v: u16) {
    if let Some(s) = d.get_mut(at..at + 2) {
        s.copy_from_slice(&v.to_le_bytes());
    }
}

fn put_u32(d: &mut [u8], at: usize, v: u32) {
    if let Some(s) = d.get_mut(at..at + 4) {
        s.copy_from_slice(&v.to_le_bytes());
    }
}

fn put_u64(d: &mut [u8], at: usize, v: u64) {
    if let Some(s) = d.get_mut(at..at + 8) {
        s.copy_from_slice(&v.to_le_bytes());
    }
}

/// ELF/PE/Mach-O container kind of the *unmutated* fixture.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Elf,
    Pe,
    Macho,
    Unknown,
}

fn kind_of(d: &[u8]) -> Kind {
    if d.len() >= 4 && &d[0..4] == b"\x7fELF" {
        Kind::Elf
    } else if d.len() >= 2 && &d[0..2] == b"MZ" {
        Kind::Pe
    } else if d.len() >= 4 {
        let magic = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
        // The same four magics lib.rs dispatches on.
        const MH_MAGIC_64: u32 = 0xFEED_FACF;
        const MH_CIGAM_64: u32 = 0xCFFE_DAFE;
        const MH_MAGIC: u32 = 0xFEED_FACE;
        const MH_CIGAM: u32 = 0xCEFA_EDFE;
        match magic {
            MH_MAGIC_64 | MH_CIGAM_64 | MH_MAGIC | MH_CIGAM => Kind::Macho,
            _ => Kind::Unknown,
        }
    } else {
        Kind::Unknown
    }
}

/// `(e_shoff, e_shnum, e_shentsize)` from an ELF header, when the class
/// byte and the table bounds are sane enough to mutate against.
fn elf_section_table(d: &[u8]) -> Option<(usize, usize, usize)> {
    let class = *d.get(4)?;
    let (shoff_at, shnum_at, shentsize_at, shentsize_want) = match class {
        2 => (0x28usize, 0x3C, 0x3A, 64usize),
        1 => (0x20, 0x30, 0x2E, 40),
        _ => return None,
    };
    let shoff = if class == 2 {
        get_u64(d, shoff_at)? as usize
    } else {
        get_u32(d, shoff_at)? as usize
    };
    let shnum = get_u16(d, shnum_at)? as usize;
    let shentsize = get_u16(d, shentsize_at)? as usize;
    if shentsize != shentsize_want {
        return None;
    }
    let table_bytes = shnum.checked_mul(shentsize)?;
    if shoff.checked_add(table_bytes)? > d.len() {
        return None;
    }
    Some((shoff, shnum, shentsize))
}

/// Byte offset of ELF section header `i`'s sh_addr/sh_offset/sh_size field
/// (`field` is 0, 1, or 2; 64-bit and 32-bit layouts differ), bounds-checked.
fn elf_sh_field(d: &[u8], i: usize, field: usize) -> Option<usize> {
    let class = *d.get(4)?;
    let (shoff, shnum, shentsize) = elf_section_table(d)?;
    if i >= shnum {
        return None;
    }
    let hdr = shoff.checked_add(i.checked_mul(shentsize)?)?;
    let (at, width) = match (class, field) {
        (2, 0) => (hdr + 16, 8), // sh_addr
        (2, 1) => (hdr + 24, 8), // sh_offset
        (2, 2) => (hdr + 32, 8), // sh_size
        (1, 0) => (hdr + 12, 4),
        (1, 1) => (hdr + 16, 4),
        (1, 2) => (hdr + 20, 4),
        _ => return None,
    };
    if at + width > d.len() {
        return None;
    }
    Some(at)
}

/// True when ELF section header `i` carries `SHF_EXECINSTR` (0x4), read
/// straight from the raw header bytes.
fn elf_sh_is_executable(d: &[u8], i: usize) -> bool {
    let class = d.get(4).copied().unwrap_or(0);
    let Some((shoff, shnum, shentsize)) = elf_section_table(d) else {
        return false;
    };
    if i >= shnum {
        return false;
    }
    let Some(hdr) = i
        .checked_mul(shentsize)
        .and_then(|off| shoff.checked_add(off))
    else {
        return false;
    };
    let flags = if class == 2 {
        get_u64(d, hdr + 8).unwrap_or(0)
    } else {
        u64::from(get_u32(d, hdr + 8).unwrap_or(0))
    };
    flags & 0x4 != 0
}

// ELF64 header field offsets (the ELF32 counterparts are inline where used).
const ELF64_SHOFF_AT: usize = 0x28;
const ELF64_SHNUM_AT: usize = 0x3C;
const ELF64_SHENTSIZE_AT: usize = 0x3A;
const ELF64_PHNUM_AT: usize = 0x38;
const ELF_TYPE_AT: usize = 0x10;

// PE layout: DOS header `e_lfanew` at 0x3C; `PE\0\0` there; COFF header at
// lfanew+4 (NumberOfSections at +2, SizeOfOptionalHeader at +16); section
// headers at lfanew+24+SizeOfOptionalHeader, 40 bytes each.
const PE_LFANEW_AT: usize = 0x3C;

fn pe_lfanew(d: &[u8]) -> Option<usize> {
    let lfanew = get_u32(d, PE_LFANEW_AT)? as usize;
    if lfanew + 24 > d.len() || d.get(lfanew..lfanew + 4)? != b"PE\0\0" {
        return None;
    }
    Some(lfanew)
}

fn pe_section_at(d: &[u8], i: usize) -> Option<usize> {
    let lfanew = pe_lfanew(d)?;
    let n = get_u16(d, lfanew + 6)? as usize;
    let opt = get_u16(d, lfanew + 20)? as usize;
    if i >= n {
        return None;
    }
    let at = lfanew
        .checked_add(24)?
        .checked_add(opt)?
        .checked_add(i.checked_mul(40)?)?;
    if at + 40 > d.len() {
        return None;
    }
    Some(at)
}

// Mach-O layout: ncmds/sizeofcmds at 16/20; load commands from 32 (64-bit)
// or 28 (32-bit); LC_SEGMENT(_64) sections carry the length fields the
// extractor reads.
const MACHO_NCMDS_AT: usize = 16;
const MACHO_SIZEOF_CMDS_AT: usize = 20;
const LC_SEGMENT: u32 = 0x1;
const LC_SEGMENT_64: u32 = 0x19;

fn macho_is_64(d: &[u8]) -> Option<bool> {
    Some(match get_u32(d, 0)? {
        0xFEED_FACF | 0xCFFE_DAFE => true,
        0xFEED_FACE | 0xCEFA_EDFE => false,
        _ => return None,
    })
}

/// `(segment command offset, section entry offset)` for every
/// `section(_64)` found by walking the load commands. Best effort: stops at
/// the first structure violation (which is exactly the state the corrupting
/// operators leave headers in).
fn macho_sections(d: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let is64 = match macho_is_64(d) {
        Some(is64) => is64,
        None => return out,
    };
    let start = if is64 { 32 } else { 28 };
    let ncmds = get_u32(d, MACHO_NCMDS_AT).unwrap_or(0) as usize;
    let seg_cmd = if is64 { LC_SEGMENT_64 } else { LC_SEGMENT };
    let mut off = start;
    for _ in 0..ncmds {
        let Some(cmd) = get_u32(d, off) else { break };
        let Some(cmdsize) = get_u32(d, off + 4) else {
            break;
        };
        if (cmdsize as usize) < 8 || off + cmdsize as usize > d.len() {
            break;
        }
        if cmd == seg_cmd {
            let nsects = get_u32(d, off + if is64 { 64 } else { 48 }).unwrap_or(0) as usize;
            let sect_at = off + if is64 { 72 } else { 56 };
            let sect_size = if is64 { 80 } else { 68 };
            for s in 0..nsects {
                let at = sect_at + s * sect_size;
                if at + sect_size > d.len() {
                    break;
                }
                out.push((off, at));
            }
        }
        off += cmdsize as usize;
    }
    out
}

// ---------------------------------------------------------------------------
// Mutation operators
// ---------------------------------------------------------------------------

/// Apply operator `op` (see [`OP_NAMES`]) to a copy of `base`.
fn mutate(base: &[u8], op: usize, rng: &mut Xorshift64) -> Vec<u8> {
    let mut d = base.to_vec();
    let kind = kind_of(base);
    match op {
        0 => {
            // Single bit flip at a random position.
            if !d.is_empty() {
                let at = rng.below(d.len());
                d[at] ^= 1 << rng.below(8);
            }
        }
        1 => {
            // Multi bit flips (2..=5).
            let flips = 2 + rng.below(4);
            for _ in 0..flips {
                if d.is_empty() {
                    break;
                }
                let at = rng.below(d.len());
                d[at] ^= 1 << rng.below(8);
            }
        }
        2 => {
            // Byte substitutions (1..=4) with random bytes.
            let subs = 1 + rng.below(4);
            for _ in 0..subs {
                if d.is_empty() {
                    break;
                }
                let at = rng.below(d.len());
                d[at] = rng.byte();
            }
        }
        3 => {
            // Magic-byte corruption: the container-dispatch bytes.
            for at in 0..d.len().min(4) {
                d[at] = rng.byte();
            }
        }
        4 => {
            // Truncation. Half the draws land in the first 512 bytes so the
            // header tables themselves get cut, the rest anywhere.
            let n = if rng.below(2) == 0 {
                rng.below(512.min(d.len() + 1))
            } else {
                rng.below(d.len() + 1)
            };
            d.truncate(n);
        }
        5 => append_header_table(&mut d, kind, rng),
        6 => inflate_length_fields(&mut d, kind, rng),
        7 => corrupt_header_fields(&mut d, kind, rng),
        8 => {
            // Endianness flip: ELF EI_DATA 1<->2; Mach-O magic swapped into
            // its CIGAM form; PE has no endianness byte, so fall back to a
            // byte substitution (COFF is little-endian by definition).
            match kind {
                Kind::Elf if d.len() > 5 && (d[5] == 1 || d[5] == 2) => d[5] = 3 - d[5],
                Kind::Macho if d.len() >= 4 => d[0..4].reverse(),
                _ => {
                    if !d.is_empty() {
                        let at = rng.below(d.len());
                        d[at] = rng.byte();
                    }
                }
            }
        }
        9 => {
            // Mixed damage: 2..=4 random substitutions/flips plus a
            // truncation one time in three.
            let subs = 2 + rng.below(3);
            for _ in 0..subs {
                if d.is_empty() {
                    break;
                }
                let at = rng.below(d.len());
                if rng.below(2) == 0 {
                    d[at] = rng.byte();
                } else {
                    d[at] ^= 1 << rng.below(8);
                }
            }
            if rng.below(3) == 0 {
                let n = rng.below(d.len() + 1);
                d.truncate(n);
            }
        }
        _ => unreachable!("operator index is taken modulo OP_COUNT"),
    }
    d
}

/// Duplicate-section-header injection: append a small crafted section
/// header table (ELF) or section/load-command table (PE, Mach-O) and point
/// the format's count field at it. Half the appended headers copy an
/// existing one verbatim (the literal "duplicate" case), the rest are
/// freshly crafted with extreme field values.
fn append_header_table(d: &mut Vec<u8>, kind: Kind, rng: &mut Xorshift64) {
    match kind {
        Kind::Elf => {
            let Some(class) = d.get(4).copied() else {
                return;
            };
            if class != 1 && class != 2 {
                return;
            }
            let hdr_size = if class == 2 { 64 } else { 40 };
            let (shoff_at, shnum_at, shentsize_at) = if class == 2 {
                (ELF64_SHOFF_AT, ELF64_SHNUM_AT, ELF64_SHENTSIZE_AT)
            } else {
                (0x20, 0x30, 0x2E)
            };
            let old_table = elf_section_table(d);
            let append_at = d.len();
            let count = 2 + rng.below(3);
            for k in 0..count {
                if k % 2 == 1 {
                    // Duplicate an existing header verbatim when there is
                    // one; two headers then claim the same bytes.
                    if let Some((old_shoff, old_shnum, old_shentsize)) = old_table {
                        if old_shnum > 0 {
                            let src = old_shoff + rng.below(old_shnum) * old_shentsize;
                            if src + hdr_size <= append_at {
                                let dup = d[src..src + hdr_size].to_vec();
                                d.extend_from_slice(&dup);
                                continue;
                            }
                        }
                    }
                }
                // Crafted header: zeroed, PROGBITS, ALLOC|EXECINSTR, then
                // extreme addr/offset/size values that stress the clamps
                // and the alignment arithmetic.
                let start = d.len();
                d.extend(std::iter::repeat_n(0u8, hdr_size));
                put_u32(d, start, 0); // sh_name (empty)
                put_u32(d, start + 4, 1); // sh_type = PROGBITS
                put_u32(d, start + 8, 0x6); // sh_flags = ALLOC|EXECINSTR
                let extreme: u64 = [u64::MAX, 0, 0xFFFF_0000_0000_0000, 0x1000][rng.below(4)];
                if class == 2 {
                    put_u64(d, start + 16, extreme); // sh_addr
                    put_u64(d, start + 24, extreme); // sh_offset
                    put_u64(d, start + 32, extreme); // sh_size
                } else {
                    put_u32(d, start + 12, extreme as u32);
                    put_u32(d, start + 16, extreme as u32);
                    put_u32(d, start + 20, extreme as u32);
                }
            }
            if class == 2 {
                put_u64(d, shoff_at, append_at as u64);
            } else {
                put_u32(d, shoff_at, append_at as u32);
            }
            put_u16(d, shentsize_at, hdr_size as u16);
            put_u16(d, shnum_at, count as u16);
        }
        Kind::Pe => {
            // Append crafted PE section headers and grow NumberOfSections.
            let Some(lfanew) = pe_lfanew(d) else {
                if !d.is_empty() {
                    let at = rng.below(d.len());
                    d[at] = rng.byte();
                }
                return;
            };
            let count = 1 + rng.below(3);
            for _ in 0..count {
                let start = d.len();
                d.extend(std::iter::repeat_n(0u8, 40));
                d[start..start + 4].copy_from_slice(b"FUZZ");
                put_u32(d, start + 8, u32::MAX); // VirtualSize
                put_u32(d, start + 16, u32::MAX); // SizeOfRawData
                put_u32(d, start + 20, u32::MAX); // PointerToRawData
                put_u32(d, start + 36, 0x6000_0020); // code, execute, read
            }
            let n = get_u16(d, lfanew + 6).unwrap_or(0);
            put_u16(d, lfanew + 6, n.saturating_add(count as u16));
        }
        Kind::Macho => {
            // Append a crafted LC_SEGMENT(_64) carrying one section with
            // extreme length fields, and grow ncmds/sizeofcmds.
            let Some(is64) = macho_is_64(d) else {
                if !d.is_empty() {
                    let at = rng.below(d.len());
                    d[at] = rng.byte();
                }
                return;
            };
            let start = d.len();
            let (cmd, seg_size, sect_size) = if is64 {
                (LC_SEGMENT_64, 72usize, 80usize)
            } else {
                (LC_SEGMENT, 56, 68)
            };
            let cmdsize = (seg_size + sect_size) as u32;
            d.extend(std::iter::repeat_n(0u8, seg_size + sect_size));
            put_u32(d, start, cmd);
            put_u32(d, start + 4, cmdsize);
            d[start + 8..start + 14].copy_from_slice(b"__FUZZ");
            let sect = start + seg_size;
            d[sect..sect + 6].copy_from_slice(b"__fuzz");
            if is64 {
                put_u64(d, start + 48, u64::MAX); // segment filesize
                put_u32(d, start + 64, 1); // nsects
                put_u64(d, sect + 40, u64::MAX); // section size
                put_u32(d, sect + 48, u32::MAX); // section offset
            } else {
                put_u32(d, start + 36, u32::MAX); // segment filesize
                put_u32(d, start + 48, 1); // nsects
                put_u32(d, sect + 36, u32::MAX); // section size
                put_u32(d, sect + 40, u32::MAX); // section offset
            }
            let n = get_u32(d, MACHO_NCMDS_AT).unwrap_or(0);
            put_u32(d, MACHO_NCMDS_AT, n.saturating_add(1));
            let sz = get_u32(d, MACHO_SIZEOF_CMDS_AT).unwrap_or(0);
            put_u32(d, MACHO_SIZEOF_CMDS_AT, sz.saturating_add(cmdsize));
        }
        Kind::Unknown => {
            if !d.is_empty() {
                let at = rng.below(d.len());
                d[at] = rng.byte();
            }
        }
    }
}

/// Length-field inflation: pick one section (or segment/section on Mach-O)
/// and drive one of its length/address fields to the type's maximum - the
/// shape that produced overflow panics in offset arithmetic before the
/// hardening fixes.
fn inflate_length_fields(d: &mut [u8], kind: Kind, rng: &mut Xorshift64) {
    match kind {
        Kind::Elf => {
            let class = d.get(4).copied();
            let field = rng.below(3); // 0 = sh_addr, 1 = sh_offset, 2 = sh_size
            let n_sh = elf_section_table(d).map(|(_, n, _)| n).unwrap_or(0);
            if n_sh == 0 {
                return;
            }
            let Some(at) = elf_sh_field(d, rng.below(n_sh), field) else {
                return;
            };
            match class {
                Some(2) => put_u64(d, at, u64::MAX),
                Some(1) => put_u32(d, at, u32::MAX),
                _ => {}
            }
        }
        Kind::Pe => {
            let n = {
                let lfanew = pe_lfanew(d).unwrap_or(0);
                get_u16(d, lfanew + 6).unwrap_or(0) as usize
            };
            if n == 0 {
                return;
            }
            let Some(at) = pe_section_at(d, rng.below(n)) else {
                return;
            };
            match rng.below(3) {
                0 => put_u32(d, at + 8, u32::MAX),  // VirtualSize
                1 => put_u32(d, at + 16, u32::MAX), // SizeOfRawData
                _ => put_u32(d, at + 20, u32::MAX), // PointerToRawData
            }
        }
        Kind::Macho => {
            let sections = macho_sections(d);
            if sections.is_empty() {
                return;
            }
            let (seg_at, sect_at) = sections[rng.below(sections.len())];
            let is64 = macho_is_64(d).unwrap_or(true);
            match rng.below(4) {
                0 if is64 => put_u64(d, seg_at + 48, u64::MAX), // segment filesize
                0 => put_u32(d, seg_at + 36, u32::MAX),
                1 => {
                    if is64 {
                        put_u64(d, sect_at + 40, u64::MAX) // section size
                    } else {
                        put_u32(d, sect_at + 36, u32::MAX)
                    }
                }
                2 => put_u32(d, sect_at + if is64 { 48 } else { 40 }, u32::MAX), // offset
                _ if is64 => put_u64(d, seg_at + 24, u64::MAX),                  // segment vmaddr
                _ => put_u32(d, seg_at + 24, u32::MAX),
            }
        }
        Kind::Unknown => {}
    }
}

/// Header-field corruption. On ELF the pool deliberately includes `e_type`
/// (flipping a linked image into `ET_REL` changes how symbol addresses are
/// interpreted) and compounds with a section `sh_offset` inflation, because
/// the interesting arithmetic lives at their intersection. PE corrupts
/// `e_lfanew` and the section counts; Mach-O corrupts `ncmds`,
/// `sizeofcmds`, `filetype`, and `cputype`.
fn corrupt_header_fields(d: &mut [u8], kind: Kind, rng: &mut Xorshift64) {
    match kind {
        Kind::Elf => {
            let class = d.get(4).copied();
            if class != Some(1) && class != Some(2) {
                return;
            }
            let class64 = class == Some(2);
            match rng.below(5) {
                0 => {
                    // e_type into ET_REL / ET_EXEC / garbage.
                    put_u16(d, ELF_TYPE_AT, [1u16, 2, 0xFFFF, 0][rng.below(4)]);
                }
                1 => {
                    if class64 {
                        put_u64(d, ELF64_SHOFF_AT, [u64::MAX, 0, 0x7FFF_FFFF][rng.below(3)]);
                    } else {
                        put_u32(d, 0x20, [u32::MAX, 0, 0x7FFF_FFFF][rng.below(3)]);
                    }
                }
                2 => {
                    let at = if class64 { ELF64_SHNUM_AT } else { 0x30 };
                    put_u16(d, at, [0u16, u16::MAX, 1, 0x7FFF][rng.below(4)]);
                }
                3 => {
                    let at = if class64 { ELF64_PHNUM_AT } else { 0x2C };
                    put_u16(d, at, [0u16, u16::MAX, 1, 0x7FFF][rng.below(4)]);
                }
                _ => {
                    // e_shstrndx out of range.
                    let at = if class64 { 0x3E } else { 0x32 };
                    put_u16(d, at, u16::MAX);
                }
            }
            // Compound: also drive one section's sh_offset to the maximum,
            // so symbol/section offset arithmetic is fed hostile values in
            // a single case.
            let n_sh = elf_section_table(d).map(|(_, n, _)| n).unwrap_or(0);
            if n_sh > 0 {
                if let Some(at) = elf_sh_field(d, rng.below(n_sh), 1) {
                    if class64 {
                        put_u64(d, at, u64::MAX);
                    } else {
                        put_u32(d, at, u32::MAX);
                    }
                }
            }
        }
        Kind::Pe => {
            let Some(lfanew) = pe_lfanew(d) else {
                return;
            };
            match rng.below(3) {
                0 => put_u32(d, PE_LFANEW_AT, [u32::MAX, 0][rng.below(2)]),
                1 => put_u16(d, lfanew + 6, [0u16, u16::MAX, 1][rng.below(3)]),
                _ => put_u16(d, lfanew + 20, [0u16, u16::MAX][rng.below(2)]),
            }
        }
        Kind::Macho => match rng.below(4) {
            0 => put_u32(d, MACHO_NCMDS_AT, [0u32, u32::MAX, 1][rng.below(3)]),
            1 => put_u32(d, MACHO_SIZEOF_CMDS_AT, [0, u32::MAX][rng.below(2)]),
            2 => put_u32(d, 12, rng.next_u64() as u32), // filetype
            _ => put_u32(d, 4, rng.next_u64() as u32),  // cputype
        },
        Kind::Unknown => {}
    }
}

// ---------------------------------------------------------------------------
// Case runner
// ---------------------------------------------------------------------------

/// Outcome of one user-facing path on one mutated binary.
#[derive(Default)]
struct PathOutcome {
    ok: bool,
    panic: Option<String>,
}

/// Everything the sweep needs to know about one case.
#[derive(Default)]
struct CaseReport {
    mutation_panic: Option<String>,
    extract: PathOutcome,
    /// True when a successful `extract_binary` carried its `meta:` key.
    features_invariant_holds: bool,
    functions: PathOutcome,
    /// True when a successful `extract_binary_functions` kept the
    /// functions-empty == reason-set contract.
    functions_invariant_holds: bool,
    classify_panic: Option<String>,
    elapsed: Duration,
}

fn panic_payload(p: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "opaque panic payload".to_string()
    }
}

/// True when the panic is a documented goblin-internal bug on exactly this
/// deterministic case (see [`KNOWN_UPSTREAM_PANICS`]) and may pass with a
/// note.
fn panic_is_upstream(fixture_name: &str, case_index: usize, payload: &str) -> bool {
    KNOWN_UPSTREAM_PANICS
        .iter()
        .any(|(name, cidx, needle, _why)| {
            *name == fixture_name && *cidx == case_index && payload.contains(needle)
        })
}

thread_local! {
    /// Set on case-runner threads only; the panic hook installed by
    /// [`install_quiet_panic_hook`] suppresses the default hook's output on
    /// those threads because every panic there is caught, inspected, and
    /// either allowed or turned into an assertion with full context. Other
    /// tests' panic output is untouched (their threads never set this).
    static QUIET_PANICS: Cell<bool> = const { Cell::new(false) };
}

fn install_quiet_panic_hook() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if QUIET_PANICS.with(|q| q.get()) {
                // One quiet line instead of the default multi-page hook
                // output: every panic on a case thread is caught right
                // below the hook and either allowed or failed with full
                // repro context, so the message and location are all the
                // operator needs here.
                eprintln!(
                    "robustness: caught panic at {}:{}: {}",
                    info.location().map(|l| l.file()).unwrap_or("?"),
                    info.location().map(|l| l.line()).unwrap_or(0),
                    info.payload_as_str().unwrap_or("(non-string payload)")
                );
            } else {
                previous(info);
            }
        }));
    });
}

/// Run one case: mutate, write to the scratch file, and push the result
/// through extract, functions, and classify under `catch_unwind`.
fn run_case(
    base: &[u8],
    base_features: &BTreeMap<String, u64>,
    model: Option<&Model>,
    scratch: &Path,
    op: usize,
    mut rng: Xorshift64,
) -> CaseReport {
    QUIET_PANICS.with(|q| q.set(true));
    let start = Instant::now();
    let mut report = CaseReport::default();

    let mutated =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mutate(base, op, &mut rng)))
        {
            Ok(m) => m,
            Err(p) => {
                report.mutation_panic = Some(panic_payload(&p));
                report.elapsed = start.elapsed();
                return report;
            }
        };

    if std::fs::write(scratch, &mutated).is_err() {
        report.extract.panic = Some("scratch file write failed".to_string());
        report.elapsed = start.elapsed();
        return report;
    }

    // Whole-binary feature extraction (`compid extract`).
    let features =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| extract_binary(scratch))) {
            Ok(Ok(ef)) => {
                report.extract.ok = true;
                report.features_invariant_holds = ef.features.contains_key("meta:windows")
                    || ef.features.contains_key("meta:macos");
                Some(ef.features)
            }
            Ok(Err(_)) => None, // a graceful error is a pass for this path
            Err(p) => {
                report.extract.panic = Some(panic_payload(&p));
                None
            }
        };

    // Function enumeration (`compid functions`), heuristic path included.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        extract_binary_functions(scratch)
    })) {
        Ok(Ok(fx)) => {
            report.functions.ok = true;
            report.functions_invariant_holds = fx.functions.is_empty() == fx.reason.is_some();
        }
        Ok(Err(_)) => {}
        Err(p) => report.functions.panic = Some(panic_payload(&p)),
    }

    // Classify: the model-free cosine compare of `compid compare`, plus a
    // real model predict when the committed model file is loadable.
    if let Some(features) = &features {
        let classify = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _similarity = compid::features::cosine(base_features, features);
            if let Some(m) = model {
                let _prediction = m.predict(features);
            }
        }));
        if let Err(p) = classify {
            report.classify_panic = Some(panic_payload(&p));
        }
    }

    report.elapsed = start.elapsed();
    report
}

/// The committed classify model, loaded once per process. An absent or
/// unloadable model file skips only the predict half of the classify path
/// (the cosine compare still runs); the sweep itself never depends on the
/// model directory.
fn sweep_model(manifest_dir: &Path) -> Option<Arc<Model>> {
    static MODEL: std::sync::OnceLock<Option<Model>> = std::sync::OnceLock::new();
    MODEL
        .get_or_init(
            || match Model::load(&manifest_dir.join("model/model.json")) {
                Ok(m) => Some(m),
                Err(e) => {
                    eprintln!(
                        "robustness sweep: no usable model/model.json ({e}); the classify \
                     predict half is skipped, cosine compare still runs"
                    );
                    None
                }
            },
        )
        .as_ref()
        .map(|m| Arc::new(m.clone()))
}

// ---------------------------------------------------------------------------
// The sweep
// ---------------------------------------------------------------------------

struct Fixture {
    name: String,
    bytes: Arc<Vec<u8>>,
    features: Arc<BTreeMap<String, u64>>,
}

fn load_fixtures() -> Vec<Fixture> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let path = dir.join(&name);
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|e| panic!("fixture {} became unreadable: {e}", path.display()));
            // Valid fixtures must extract: precompute the baseline feature
            // map the cosine compare runs against.
            let features = compid::elfinfo::load(&path)
                .and_then(|info| compid::features::extract(&info))
                .or_else(|_| {
                    compid::peinfo::load(&path).and_then(|info| compid::features::extract_pe(&info))
                })
                .or_else(|_| {
                    compid::machoinfo::load(&path)
                        .and_then(|info| compid::features::extract_macho(&info))
                })
                .unwrap_or_else(|e| {
                    panic!("fixture {} should extract cleanly: {e}", path.display())
                });
            Fixture {
                name,
                bytes: Arc::new(bytes),
                features: Arc::new(features.features),
            }
        })
        .collect()
}

/// Run `cases` (each a `(fixture index, case index)` pair) on the calling
/// worker thread, one case per spawned runner thread with a hard timeout.
/// Appends one line per failed case to `failures`; returns the slowest
/// case for the summary.
fn run_cases(
    cases: &[(usize, usize)],
    fixtures: &[Fixture],
    model: Option<Arc<Model>>,
    scratch_dir: &Path,
    worker: usize,
    failures: &mut Vec<String>,
) -> (Duration, String) {
    let mut slowest = (Duration::ZERO, String::new());
    for &(fidx, cidx) in cases {
        let fixture = &fixtures[fidx];
        let op = cidx % OP_COUNT;
        let seed = case_seed(&fixture.name, cidx);
        let scratch = scratch_dir.join(format!("case_w{worker}.bin"));

        let (tx, rx) = std::sync::mpsc::channel();
        let base = Arc::clone(&fixture.bytes);
        let base_features = Arc::clone(&fixture.features);
        let model = model.clone();
        std::thread::spawn(move || {
            let report = run_case(
                &base,
                &base_features,
                model.as_deref(),
                &scratch,
                op,
                Xorshift64::new(seed),
            );
            let _ = tx.send(report);
            // Detached on purpose: the receiver owns the outcome, and a
            // hung case thread is deliberately leaked (see
            // CASE_HARD_TIMEOUT).
        });

        let label = format!(
            "fixture={} case={cidx} op={} ({}) seed={seed:#018x}",
            fixture.name, op, OP_NAMES[op]
        );
        match rx.recv_timeout(CASE_HARD_TIMEOUT) {
            Ok(report) => {
                if report.elapsed > slowest.0 {
                    slowest = (report.elapsed, label.clone());
                }
                let mut panics: Vec<(&str, &String)> = Vec::new();
                if let Some(p) = &report.mutation_panic {
                    panics.push(("the mutation step", p));
                }
                if let Some(p) = &report.extract.panic {
                    panics.push(("extract_binary", p));
                }
                if let Some(p) = &report.functions.panic {
                    panics.push(("extract_binary_functions", p));
                }
                if let Some(p) = &report.classify_panic {
                    panics.push(("the classify path", p));
                }
                for (path_name, payload) in &panics {
                    if panic_is_upstream(&fixture.name, cidx, payload) {
                        // Documented goblin-internal panic: noted, not
                        // failed. compid cannot fix the dependency here.
                        eprintln!(
                            "robustness: allowed upstream panic (goblin) at {label} \
                             in {path_name}: {payload}"
                        );
                    } else {
                        failures.push(format!("{label} panics in {path_name}: {payload}"));
                    }
                }
                if report.extract.ok && !report.features_invariant_holds {
                    failures.push(format!("{label}: successful extract lost its meta: key"));
                }
                if report.functions.ok && !report.functions_invariant_holds {
                    failures.push(format!("{label}: functions list empty/reason mismatch"));
                }
                if report.elapsed > Duration::from_secs(1) {
                    eprintln!(
                        "robustness: slow case {label} took {:.2}s",
                        report.elapsed.as_secs_f64()
                    );
                }
            }
            Err(_) => failures.push(format!(
                "{label} HUNG past {}s (its runner thread was left running; a hang \
                 is a failure even though std cannot kill a thread)",
                CASE_HARD_TIMEOUT.as_secs()
            )),
        }
    }
    slowest
}

/// The full deterministic mutation sweep over every committed fixture.
#[test]
fn mutation_sweep_extracts_functions_and_classifies_without_panics() {
    let fixtures = load_fixtures();
    if fixtures.is_empty() {
        eprintln!("skipping: no fixtures in tests/fixtures yet");
        return;
    }

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scratch_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("robustness/sweep");
    std::fs::create_dir_all(&scratch_dir).expect("scratch dir creates");

    install_quiet_panic_hook();

    // Case list, worker-independent: workers only split the list, so the
    // worker count changes wall clock, never outcomes.
    let mut cases: Vec<(usize, usize)> = Vec::new();
    for (fidx, _) in fixtures.iter().enumerate() {
        for cidx in 0..MUTATIONS_PER_FIXTURE {
            cases.push((fidx, cidx));
        }
    }
    assert!(
        cases.len() >= MIN_TOTAL_CASES,
        "the sweep runs {} cases but the bar is {MIN_TOTAL_CASES}; add fixtures \
         or raise MUTATIONS_PER_FIXTURE",
        cases.len()
    );

    let started = Instant::now();
    let (failures, slowest, workers) = std::thread::scope(|scope| {
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(1, 8);
        let model = sweep_model(manifest_dir);
        let fixtures_ref: &[Fixture] = &fixtures;
        let scratch_ref: &Path = &scratch_dir;
        let chunks: Vec<Vec<(usize, usize)>> = (0..workers)
            .map(|w| cases.iter().copied().skip(w).step_by(workers).collect())
            .collect();
        let handles: Vec<_> = chunks
            .into_iter()
            .enumerate()
            .map(|(worker, chunk)| {
                let model = model.clone();
                scope.spawn(move || {
                    let mut failures = Vec::new();
                    let slowest = run_cases(
                        &chunk,
                        fixtures_ref,
                        model,
                        scratch_ref,
                        worker,
                        &mut failures,
                    );
                    (failures, slowest)
                })
            })
            .collect();
        let mut all_failures = Vec::new();
        let mut slowest = (Duration::ZERO, String::new());
        for h in handles {
            let (fails, slow) = h.join().expect("worker thread survives the sweep");
            all_failures.extend(fails);
            if slow.0 > slowest.0 {
                slowest = slow;
            }
        }
        (all_failures, slowest, workers)
    });
    let wall = started.elapsed();

    for line in failures.iter().take(20) {
        eprintln!("robustness failure: {line}");
    }
    eprintln!(
        "robustness sweep: {} cases over {} fixtures on {workers} workers in {:.2}s \
         (slowest case {:.3}s)",
        cases.len(),
        fixtures.len(),
        wall.as_secs_f64(),
        slowest.0.as_secs_f64()
    );

    assert!(
        failures.is_empty(),
        "{} of {} mutated binaries panicked, hung, or broke an invariant; first \
         failure:\n  {}\n(reproduce one case: COMPID_FUZZ_FIXTURE=<fixture> \
         COMPID_FUZZ_CASE=<case> cargo test --test robustness_test -- --ignored \
         repro_single_case --nocapture)",
        failures.len(),
        cases.len(),
        failures.first().map(String::as_str).unwrap_or("?")
    );
    assert!(
        wall < SUITE_TIME_BUDGET,
        "the robustness sweep took {:.2}s, over the {:.0}s budget; lower \
         MUTATIONS_PER_FIXTURE or investigate the slow case printed above",
        wall.as_secs_f64(),
        SUITE_TIME_BUDGET.as_secs_f64()
    );

    let _ = std::fs::remove_dir_all(&scratch_dir);
}

/// The determinism contract, verified directly: the same (seed, fixture,
/// case index) yields byte-identical mutations on every call.
#[test]
fn mutation_stream_is_deterministic() {
    let fixtures = load_fixtures();
    for fixture in fixtures.iter().take(4) {
        for cidx in [0, 1, 7, 41, MUTATIONS_PER_FIXTURE - 1] {
            let op = cidx % OP_COUNT;
            let a = mutate(
                &fixture.bytes,
                op,
                &mut Xorshift64::new(case_seed(&fixture.name, cidx)),
            );
            let b = mutate(
                &fixture.bytes,
                op,
                &mut Xorshift64::new(case_seed(&fixture.name, cidx)),
            );
            assert_eq!(
                a, b,
                "fixture {} case {cidx}: the mutation stream is not deterministic",
                fixture.name
            );
        }
    }
}

/// Regression cases: exact sweep cases that panicked before the hardening
/// fixes, pinned so the arithmetic they exercise cannot regress quietly.
/// Each ran through `catch_unwind` in the sweep, so these assert the
/// stronger property directly: no panic on any path, and at least one path
/// still returns `Ok` (the mutation parses deep enough to reach the fixed
/// arithmetic, so the test cannot pass vacuously).
///
/// What each case tripped before the fixes:
///
/// - `gcc13_O2_x86_64_matmul` case 26, `gcc13_O0_x86_64_matmul` case 6:
///   section `sh_offset` inflated to `u64::MAX`; the symbol-table path
///   computed the file offset with plain `+` and overflowed (was
///   `src/elfinfo.rs:312`, `hdr.sh_offset as usize + (st_value - lo)`).
/// - `gcc13_O0_x86_64_hash` case 57, `gcc13_O2_x86_64_recursion` case 27,
///   `gcc13_O2_x86_64_loop` case 87: the compound `e_type`/`sh_offset`
///   corruption feeding the same addition (same fix site).
/// - `zig-cc_O2_x86_64_hello` case 56, `zig-cc_O2_x86_64_hash` case 76:
///   section `sh_addr` inflated; the stripped-binary prologue scan checked
///   alignment with `base_addr + i` (was `src/functions.rs:381`).
#[test]
fn regression_cases_from_the_first_fuzzing_run_stay_clean() {
    let fixtures = load_fixtures();
    let by_name = |name: &str| {
        fixtures
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("fixture {name} missing"))
    };
    let regressions = [
        ("gcc13_O2_x86_64_matmul", 26),
        ("gcc13_O0_x86_64_matmul", 6),
        ("gcc13_O0_x86_64_hash", 57),
        ("gcc13_O2_x86_64_recursion", 27),
        ("gcc13_O2_x86_64_loop", 87),
        ("zig-cc_O2_x86_64_hello", 56),
        ("zig-cc_O2_x86_64_hash", 76),
    ];
    let scratch_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("robustness/regressions");
    std::fs::create_dir_all(&scratch_dir).expect("scratch dir creates");
    install_quiet_panic_hook();
    let model = sweep_model(Path::new(env!("CARGO_MANIFEST_DIR")));

    for (name, cidx) in regressions {
        let fixture = by_name(name);
        let op = cidx % OP_COUNT;
        let seed = case_seed(name, cidx);
        let report = run_case(
            &fixture.bytes,
            &fixture.features,
            model.as_deref(),
            &scratch_dir.join("regression.bin"),
            op,
            Xorshift64::new(seed),
        );
        let panics = [
            report.mutation_panic.as_ref().map(|p| (p, "mutation")),
            report.extract.panic.as_ref().map(|p| (p, "extract")),
            report.functions.panic.as_ref().map(|p| (p, "functions")),
            report.classify_panic.as_ref().map(|p| (p, "classify")),
        ];
        for (payload, path) in panics.into_iter().flatten() {
            assert!(
                panic_is_upstream(name, cidx, payload),
                "{name} case {cidx} ({}) regressed: panic in {path}: {payload}",
                OP_NAMES[op]
            );
        }
        assert!(
            report.extract.ok || report.functions.ok,
            "{name} case {cidx} no longer parses on either path; the case no \
             longer exercises the arithmetic it was pinned for"
        );
    }
    let _ = std::fs::remove_dir_all(&scratch_dir);
}

/// The stripped-binary prologue scan with a maxed-out section virtual
/// address: a crafted header carries `sh_addr = u64::MAX`, and the scan's
/// alignment gate and function addressing must saturate instead of
/// overflowing (was `src/functions.rs:381` `base_addr + i` and
/// `src/functions.rs:459` `section.addr + start`). On real binaries
/// `sh_addr + section length` never approaches `u64::MAX`, so the fix
/// cannot change results for valid input - `scripts/mixed_demo.sh` and the
/// `--measure` sweep confirm the boundary numbers are untouched.
#[test]
fn heuristic_scan_survives_a_maxed_out_section_address() {
    let name = "gcc13_O2_x86_64_pie_dynamic_stripped";
    let fixtures = load_fixtures();
    let fixture = fixtures
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("fixture {name} missing"));
    let mut d = (*fixture.bytes).clone();
    let (_, shnum, _) = elf_section_table(&d).expect("fixture has a section table");
    for i in 0..shnum {
        // Only the executable sections: goblin bounds-checks the string
        // and symbol tables it reads and would reject the image if their
        // addresses overflowed, and a corrupted sh_addr there would turn
        // the case into a graceful parse error instead of reaching the
        // scan.
        if !elf_sh_is_executable(&d, i) {
            continue;
        }
        if let Some(at) = elf_sh_field(&d, i, 0) {
            put_u64(&mut d, at, u64::MAX); // sh_addr = u64::MAX
        }
    }

    let scratch_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("robustness/heuristic");
    std::fs::create_dir_all(&scratch_dir).expect("scratch dir creates");
    let scratch = scratch_dir.join("maxed_addr.bin");
    std::fs::write(&scratch, &d).expect("scratch writes");
    install_quiet_panic_hook();

    // The user-facing paths complete without a panic. The stripped fixture
    // drives `extract_binary_functions` through the prologue heuristic, so
    // this covers both the alignment gate and the synthesized addresses.
    let extracted = extract_binary(&scratch);
    assert!(extracted.is_ok(), "features still extract: {extracted:?}");
    let functions = extract_binary_functions(&scratch);
    assert!(
        functions.is_ok(),
        "functions enumerate without panicking: {functions:?}"
    );
    // The synthesized heuristic addresses must be the saturating sums, not
    // wrapped small values: a saturated address sits at u64::MAX, a wrapped
    // one would land back near zero.
    let fx = functions.expect("functions ok");
    assert!(
        !fx.functions.is_empty(),
        "the heuristic still finds functions"
    );
    for f in &fx.functions {
        let wraps = f.addr.checked_add(f.size).is_none();
        assert!(
            !wraps || f.addr == u64::MAX,
            "heuristic address arithmetic wrapped for {} (addr {:#x}, size {:#x})",
            f.name,
            f.addr,
            f.size
        );
    }
    let _ = std::fs::remove_dir_all(&scratch_dir);
}

/// Single-case reproducer for a failure reported by the sweep:
///
/// ```text
/// COMPID_FUZZ_FIXTURE=<fixture file name> \
/// COMPID_FUZZ_CASE=<case index> \
/// cargo test --test robustness_test -- --ignored repro_single_case --nocapture
/// ```
#[test]
#[ignore = "single-case reproducer; see the module docs for the env vars"]
fn repro_single_case() {
    let name = std::env::var("COMPID_FUZZ_FIXTURE").expect("set COMPID_FUZZ_FIXTURE");
    let cidx: usize = std::env::var("COMPID_FUZZ_CASE")
        .expect("set COMPID_FUZZ_CASE")
        .parse()
        .expect("COMPID_FUZZ_CASE is a case index (integer)");
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(&name);
    let base = std::fs::read(&path).expect("fixture reads");
    let op = cidx % OP_COUNT;
    let seed = case_seed(&name, cidx);
    println!(
        "repro: fixture={name} case={cidx} op={op} ({}) seed={seed:#018x}",
        OP_NAMES[op]
    );
    let mutated = mutate(&base, op, &mut Xorshift64::new(seed));
    println!("mutated bytes: {} -> {}", base.len(), mutated.len());
    let scratch_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("robustness/repro");
    std::fs::create_dir_all(&scratch_dir).expect("scratch dir creates");
    let scratch = scratch_dir.join("repro.bin");
    std::fs::write(&scratch, &mutated).expect("scratch writes");
    let model = sweep_model(Path::new(env!("CARGO_MANIFEST_DIR")));
    let report = run_case(
        &base,
        &BTreeMap::new(),
        model.as_deref(),
        &scratch,
        op,
        Xorshift64::new(seed),
    );
    println!("extract ok: {}", report.extract.ok);
    println!("functions ok: {}", report.functions.ok);
    println!("extract panic: {:?}", report.extract.panic);
    println!("functions panic: {:?}", report.functions.panic);
    println!("classify panic: {:?}", report.classify_panic);
    println!("elapsed: {:.2?}", report.elapsed);
    let _ = std::fs::remove_dir_all(&scratch_dir);
}
