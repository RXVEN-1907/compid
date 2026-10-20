//! Function-level provenance: same feature namespaces, scoped to one
//! function's byte range.
//!
//! A function's feature vector holds exactly the byte- and code-shape
//! families the whole-binary extractor uses - `ng:`, `mn:`, `rg:`, `cn:`
//! over the function's own bytes - plus one size indicator `fn:<log2size>`
//! (floor of log2 of the byte length, clamped to 0..=24) and the container
//! marker `meta:windows` (clean 64-byte windows within the function). No
//! `sec:`/`ent:` geometry and no `cmt:` residue: those describe the
//! container, not a function.
//!
//! Function boundaries come from a symbol source the container itself
//! carries, and fall back to a conservative heuristic when there is none.
//! Symbol sources: the ELF symbol table (SHT_SYMTAB STT_FUNC entries with a
//! nonzero `st_size` - unstripped ELF and relocatable objects), the PE
//! export directory's named exports (sizes derived from the next export's
//! RVA), and the Mach-O `LC_SYMTAB` (`N_SECT` entries in executable `__TEXT`
//! sections, sizes derived from the next symbol's value). When the container
//! carries no symbol source at all and the code is x86-64, the boundaries
//! come from a conservative prologue scan over the executable sections (see
//! [`PROLOGUE_PATTERNS`]) unioned with direct call-target analysis (see
//! [`collect_direct_branch_targets`]), with synthesized `func_<hex addr>`
//! names - addresses are RVAs on PE and virtual addresses on Mach-O, the
//! same convention the container's own symbol source reports. What stays an
//! explicit reason rather than a guess: a non-x86-64 stripped binary (any
//! container), a symbol source that exists but holds no usable functions
//! (an export table without usable exports, a symtab without usable
//! symbols), and a heuristic that finds no candidates - an empty function
//! list with a reason string instead of guessed boundaries.

use crate::elfinfo::{ElfInfo, FunctionSymbol, SectionInfo};
use crate::features::{disassemble_window, log2_size, Labels, WINDOW_SIZE};
use crate::machoinfo::MachoInfo;
use crate::peinfo::PeInfo;
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use yaxpeax_arch::LengthedInstruction;
use yaxpeax_x86::long_mode::InstDecoder;

/// Upper clamp for the `fn:` size bucket (floor of log2 of the byte length).
pub const FN_SIZE_BUCKET_MAX: u64 = 24;

/// One function's scoped feature vector, ready for the table, the training
/// JSONL, or the model.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionFeatures {
    /// Raw symbol-table name, never demangled.
    pub name: String,
    /// `st_value` as recorded: a virtual address in a linked binary, an
    /// offset within the section in a relocatable object.
    pub addr: u64,
    /// `st_size` in bytes.
    pub size: u64,
    pub features: BTreeMap<String, u64>,
}

/// Where a [`FunctionsExtraction`]'s function boundaries came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionSource {
    /// A symbol source the container itself carries: the ELF symbol table
    /// (SHT_SYMTAB function symbols - unstripped ELF and relocatable
    /// objects), the PE export directory, or the Mach-O `LC_SYMTAB`.
    SymbolTable,
    /// Conservative prologue-pattern scan plus direct call-target analysis
    /// over the executable sections of a stripped x86-64 binary (ELF, PE,
    /// or Mach-O); names are synthesized (`func_<hex addr>`, RVAs on PE and
    /// virtual addresses on Mach-O).
    Heuristic,
}

/// Per-function extraction result for one binary. `functions` is empty
/// exactly when `reason` is set.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionsExtraction {
    pub build_id: String,
    /// The extractor's own best-effort labels for the container (what
    /// `compid extract` would report); batch mode replaces these with the
    /// manifest's ground-truth labels.
    pub labels: Labels,
    /// How the function boundaries were found.
    pub source: FunctionSource,
    /// One entry per function, ordered by address.
    pub functions: Vec<FunctionFeatures>,
    /// Set when function-level extraction does not apply: a stripped binary
    /// whose heuristic found no candidate starts in the executable sections,
    /// a symbol source that exists but carries no usable functions (an
    /// export table without usable exports, a symtab without usable
    /// function symbols), or a stripped non-x86-64 binary the scanner
    /// cannot read boundaries for.
    pub reason: Option<String>,
}

/// One training-record row of the functions JSONL: `{"build_id", "name",
/// "addr", "size", "labels", "features"}`, the per-function counterpart of
/// the whole-binary feature record. `addr` is preformatted hexadecimal.
#[derive(Debug, Serialize)]
pub struct FunctionTrainingRow<'a> {
    pub build_id: &'a str,
    pub name: &'a str,
    pub addr: String,
    pub size: u64,
    pub labels: &'a Labels,
    pub features: &'a BTreeMap<String, u64>,
}

/// The `fn:` feature key for a function of `len` bytes: `fn:<floor(log2)>`,
/// clamped to 0..=24.
pub fn size_bucket_key(len: usize) -> String {
    format!("fn:{}", log2_size(len as u64).min(FN_SIZE_BUCKET_MAX))
}

/// Feature counts over one function's bytes: byte 3-grams always; mnemonic,
/// register, and immediate counts from clean 64-byte windows; the clean
/// window count under `meta:windows`; and the size bucket `fn:<log2size>`
/// with value 1. No `sec:`, `ent:`, or `cmt:` keys, ever.
///
/// Like the whole-binary path, disassembly uses x86-64 long mode; functions
/// from other ISAs simply contribute no `mn:`/`rg:`/`cn:` counts and a
/// `meta:windows` of 0, while their byte 3-grams still flow.
pub fn function_features(bytes: &[u8]) -> BTreeMap<String, u64> {
    let mut f: BTreeMap<String, u64> = BTreeMap::new();
    for gram in bytes.windows(3) {
        *f.entry(format!("ng:{:02x}{:02x}{:02x}", gram[0], gram[1], gram[2]))
            .or_insert(0) += 1;
    }

    let decoder = InstDecoder::default();
    let mut clean_windows = 0u64;
    for window in bytes.chunks_exact(WINDOW_SIZE) {
        if let Some(stats) = disassemble_window(&decoder, window) {
            clean_windows += 1;
            for m in &stats.mnemonics {
                *f.entry(format!("mn:{m}")).or_insert(0) += 1;
            }
            for r in &stats.registers {
                *f.entry(format!("rg:{r}")).or_insert(0) += 1;
            }
            for imm in &stats.immediates {
                *f.entry(format!("cn:{imm:x}")).or_insert(0) += 1;
            }
        }
    }
    f.insert("meta:windows".to_string(), clean_windows);

    f.insert(size_bucket_key(bytes.len()), 1);
    f
}

/// One conservative x86-64 function-prologue pattern for stripped binaries.
/// The set is deliberately small: every pattern here is a byte sequence
/// mainstream compilers emit at a function entry and essentially never as
/// mid-function code at a 16-byte boundary. Future tuning adds or removes
/// entries here and re-runs `compid functions <bin> --measure` - nothing
/// else in the scanner needs to change.
struct ProloguePattern {
    /// Leading bytes the candidate must match exactly.
    bytes: &'static [u8],
    /// When true, a candidate is accepted only at a 16-byte-aligned virtual
    /// address - the alignment every mainstream linker gives function
    /// starts. Non-aligned prologue-shaped bytes are overwhelmingly
    /// mid-instruction coincidences.
    aligned_only: bool,
    /// When true the match is an `endbr64` site: Intel CET places those at
    /// function entries (and indirect jump targets) with no alignment
    /// guarantee beyond the instruction itself, so the site is accepted at
    /// any alignment (with a followup) and, at a 16-byte-aligned site,
    /// without one; PLT stub shapes are excluded either way.
    endbr64: bool,
    /// When non-empty, the match counts only when one of these byte
    /// sequences appears within [`ENDBR64_FOLLOWUP_WINDOW`] bytes after
    /// the match - the paired-instruction shape a lone hit cannot
    /// distinguish from mid-function code.
    followups: &'static [&'static [u8]],
}

/// `endbr64` (`f3 0f 1e fa`) alone also marks every indirect jump target,
/// so an UNALIGNED `endbr64` site counts as a function start only when one
/// of these frame-setup or callee-saved-push sequences appears within the
/// next [`ENDBR64_FOLLOWUP_WINDOW`] bytes. A 16-byte-aligned `endbr64`
/// site needs no followup: mainstream linkers align function entries, and
/// the glibc -O2 idiom (`endbr64; mov rax, rdi; ...`) has no frame setup
/// at all. What the alignment gate cannot exclude are PLT stubs, which are
/// also 16-byte-aligned `endbr64` sites - [`is_plt_stub_after_endbr64`]
/// rejects those.
const ENDBR64_FOLLOWUPS: &[&[u8]] = &[
    b"\x55",         // push rbp
    b"\x53",         // push rbx
    b"\x41\x57",     // push r15
    b"\x41\x56",     // push r14
    b"\x41\x55",     // push r13
    b"\x41\x54",     // push r12
    b"\x48\x89\xe5", // mov rbp, rsp
    b"\x48\x83\xec", // sub rsp, imm8
    b"\x48\x81\xec", // sub rsp, imm32
];

/// How many bytes after an `endbr64` match a frame-setup sequence may sit.
const ENDBR64_FOLLOWUP_WINDOW: u64 = 8;

/// Mnemonics whose instruction ends a straight-line decode (the next byte
/// belongs to a different function or a jump target, never a fallthrough).
const TERMINATORS: &[&str] = &["ret", "retf", "jmp", "ud2", "hlt", "int3"];

/// The bodies a PLT stub shows right after its `endbr64`:
/// `jmp *offset(%rip)` (`ff 25 <rel32>`), its `bnd`-prefixed form
/// (`f2 ff 25 <rel32>`), and the index-push form (`push imm32; jmp rel32`).
const PLT_STUB_AFTER: &[&[u8]] = &[b"\xff\x25", b"\xf2\xff\x25", b"\x68"];

/// The bodies an index-less PLT stub starts with (`push *GOT(%rip)` first
/// entry, plain and `bnd` `jmp *GOT`), checked at a section start.
const PLT_STUB_LEAD: &[&[u8]] = &[b"\xff\x35", b"\xff\x25", b"\xf2\xff\x35", b"\xf2\xff\x25"];

/// The pattern set. `push rbp; mov rbp, rsp` covers frame-pointer builds
/// (every -O0/-O1 function, -O2 functions that keep a frame); `endbr64`
/// covers CET-enabled builds, where every function entry is marked;
/// `push rbx; mov rbx, rdi` is the canonical non-frame-pointer prologue
/// that spills the first argument into a callee-saved register; the
/// paired-`lea` shape is the crt stub that carries no endbr64 and no
/// frame setup.
const PROLOGUE_PATTERNS: &[ProloguePattern] = &[
    ProloguePattern {
        // push rbp; mov rbp, rsp
        bytes: b"\x55\x48\x89\xe5",
        aligned_only: true,
        endbr64: false,
        followups: &[],
    },
    ProloguePattern {
        // endbr64 (CET function entries and indirect jump targets). At a
        // 16-byte-aligned site accepted as-is; unaligned, only with a
        // frame-setup followup - see ENDBR64_FOLLOWUPS.
        bytes: b"\xf3\x0f\x1e\xfa",
        aligned_only: false,
        endbr64: true,
        followups: ENDBR64_FOLLOWUPS,
    },
    ProloguePattern {
        // push rbx; mov rbx, rdi
        bytes: b"\x53\x48\x89\xfb",
        aligned_only: true,
        endbr64: false,
        followups: &[],
    },
    ProloguePattern {
        // lea disp32(%rip), rdi followed by a second rip-relative lea -
        // the deregister_tm_clones/register_tm_clones pair. Aligned-only,
        // and the second lea is what separates the stub from ordinary
        // mid-function argument setup.
        bytes: b"\x48\x8d\x3d",
        aligned_only: true,
        endbr64: false,
        followups: &[b"\x48\x8d"],
    },
];

/// Smallest gap at which two candidates are always kept as separate
/// functions; within this gap, [`merges_into`] decides by decode - an
/// earlier candidate whose code flows into the later one merges it, a
/// terminator (a thunk's `jmp`) keeps them apart.
const CANDIDATE_MIN_GAP: u64 = 16;

/// Smallest function the scan reports; a candidate with fewer bytes than
/// this before the next candidate (or the section end) is dropped.
const FUNCTION_MIN_SIZE: u64 = 16;

/// How many bytes of straight-line decoding a candidate must survive. A
/// final instruction may run past the mark - real instructions do not
/// respect the window - but a decode error inside it disqualifies the
/// candidate.
const PROLOGUE_DECODE_DEPTH: usize = 16;

/// Alignment mainstream linkers give function starts.
const FUNCTION_ALIGNMENT: u64 = 16;

/// True when instructions decode without error from the start of `bytes`
/// through the first [`PROLOGUE_DECODE_DEPTH`] bytes (a trailing
/// instruction may overrun the mark).
fn prologue_decodes(decoder: &InstDecoder, bytes: &[u8]) -> bool {
    let mut off = 0usize;
    while off < PROLOGUE_DECODE_DEPTH.min(bytes.len()) {
        match decoder.decode_slice(&bytes[off..]) {
            Ok(inst) => {
                let len = LengthedInstruction::len(&inst).to_const() as usize;
                if len == 0 {
                    return false;
                }
                off += len;
            }
            Err(_) => return false,
        }
    }
    true
}

/// True when one of `followups` appears within the
/// [`ENDBR64_FOLLOWUP_WINDOW`] bytes after a pattern match.
fn endbr64_has_followup(after: &[u8], followups: &[&[u8]]) -> bool {
    let window_len = (ENDBR64_FOLLOWUP_WINDOW as usize).min(after.len());
    let window = &after[..window_len];
    followups
        .iter()
        .any(|followup| window.windows(followup.len()).any(|w| w == *followup))
}

/// True when the bytes right after an `endbr64` open with a PLT stub body:
/// a `jmp *GOT(%rip)` (plain or `bnd`), or the index-push form
/// `push imm32; jmp rel32`.
fn is_plt_stub_after_endbr64(after: &[u8]) -> bool {
    for shape in PLT_STUB_AFTER {
        if !after.starts_with(shape) {
            continue;
        }
        match shape[0] {
            0x68 => {
                if after.len() >= 6 && after[5] == 0xe9 {
                    return true;
                }
            }
            _ => return true,
        }
    }
    false
}

/// True when a section's first bytes open with an index-less PLT stub
/// (`push *GOT` or `jmp *GOT`, plain or `bnd`).
fn is_plt_stub_lead(bytes: &[u8]) -> bool {
    PLT_STUB_LEAD.iter().any(|shape| bytes.starts_with(shape))
}

/// True when a later candidate `b` is part of the code the earlier
/// candidate `a` already claimed: either `b` sits inside one of `a`'s
/// instructions, or `a`'s straight-line decode reaches `b` on an
/// instruction boundary without a terminator in between (the CET
/// double-mark of one entry). A terminator (`ret`, `jmp`, ...) ending
/// exactly at `b` means `b` starts its own code - the linker-thunk case,
/// where an `endbr64; jmp` stub sits 9 bytes before the function it
/// tail-calls. A decode error before `b` falls back to the 16-byte gap
/// rule.
fn merges_into(bytes: &[u8], a: u64, b: u64) -> bool {
    if b - a >= CANDIDATE_MIN_GAP {
        // Farther apart than the gap: only a mid-instruction overlap
        // justifies merging, and a 15-byte instruction cannot reach that
        // far, so these are always distinct.
        return false;
    }
    // Within the gap window: decide by decode, or merge on doubt.
    let mut off = a as usize;
    let end = b as usize;
    for _ in 0..32 {
        if off >= end {
            return true;
        }
        match InstDecoder::default().decode_slice(&bytes[off..]) {
            Ok(inst) => {
                let len = LengthedInstruction::len(&inst).to_const() as usize;
                if len == 0 {
                    return true;
                }
                if off + len > end {
                    return true; // b is inside an instruction of a
                }
                if off + len == end {
                    let m = inst.opcode().to_string();
                    return !TERMINATORS.contains(&m.as_str());
                }
                off += len;
            }
            Err(_) => return true, // cannot tell: keep the gap rule
        }
    }
    true
}

/// The prologue scan's raw pattern hits over one executable byte range, in
/// file order and unsorted: every [`PROLOGUE_PATTERNS`] match that survives
/// its alignment, followup, and PLT-shape gates, plus offset 0 (the linker
/// places a function at an executable section start by construction, unless
/// the section opens with a PLT stub shape). Split from
/// [`scan_prologue_candidates`] so the call-target collector can union its
/// own hits into the same pipeline; `base_addr` is the virtual address of
/// `bytes[0]` and drives the alignment gate.
fn prologue_hits(bytes: &[u8], base_addr: u64) -> Vec<u64> {
    let mut hits: Vec<u64> = Vec::new();
    for pattern in PROLOGUE_PATTERNS {
        let plen = pattern.bytes.len();
        for i in 0..bytes.len().saturating_sub(plen - 1) {
            if !bytes[i..].starts_with(pattern.bytes) {
                continue;
            }
            // base_addr comes straight from the (untrusted) section header,
            // so the addition saturates instead of panicking on overflow; on
            // real binaries sh_addr + len never approaches u64::MAX, so the
            // alignment gate behaves exactly as before.
            let aligned = base_addr
                .saturating_add(i as u64)
                .is_multiple_of(FUNCTION_ALIGNMENT);
            let after = &bytes[i + plen..];
            if pattern.endbr64 {
                if is_plt_stub_after_endbr64(after) {
                    // endbr64; jmp *GOT(%rip): a PLT stub, not an entry.
                    continue;
                }
                if !aligned && !endbr64_has_followup(after, ENDBR64_FOLLOWUPS) {
                    continue;
                }
            } else {
                if pattern.aligned_only && !aligned {
                    continue;
                }
                if !pattern.followups.is_empty() && !endbr64_has_followup(after, pattern.followups)
                {
                    continue;
                }
            }
            hits.push(i as u64);
        }
    }
    // The first byte of an executable section is a function start by
    // construction (.init, .fini, .text.unlikely all open with one) unless
    // the section is a PLT.
    let after_lead = &bytes[4.min(bytes.len())..];
    if !bytes.is_empty() && !is_plt_stub_lead(bytes) && !is_plt_stub_after_endbr64(after_lead) {
        hits.push(0);
    }
    hits
}

/// Whether direct `jmp rel32` (E9) targets join `call rel32` (E8) targets
/// in the call-target candidate set. E9 targets may include tail-call
/// thunks (a real `jmp other_function` entry), but they also include every
/// long-range intra-function branch - loop back edges and goto joins -
/// which splits otherwise-correct functions and costs far more precision
/// than the thunks add recall. Both modes were measured on the 40-binary
/// sweep (see docs/FUNCTIONS-EVALUATION.md): E8-only lands at pooled F1
/// 0.9486 (precision 0.9819, recall 0.9174); adding E9 targets drops
/// precision to 0.4334 for a recall of 0.9215 (F1 0.5896). E8-only keeps
/// the better F1, so E9 targets stay out.
const CALL_TARGETS_INCLUDE_JMP: bool = false;

/// One direct branch target found by the linear sweep: a decoded 5-byte
/// instruction opening with the opcode byte (`E8` call rel32, or `E9` jmp
/// rel32 when `include_jmp`), as a section-relative offset. A target is
/// recorded only when it falls inside the scanned range - cross-section
/// branches (into `.plt`, for instance) are thereby excluded, which is the
/// PLT exclusion for call targets that section boundaries already give the
/// prologue scan. Targets may include tail-thunks and mid-function branch
/// labels; the qualification filters (see [`call_target_hits`]) and the
/// overlap merge decide what survives.
fn collect_direct_branch_targets(bytes: &[u8], include_jmp: bool) -> Vec<u64> {
    let decoder = InstDecoder::default();
    let mut targets: Vec<u64> = Vec::new();
    let mut off = 0usize;
    while off < bytes.len() {
        let inst = match decoder.decode_slice(&bytes[off..]) {
            Ok(inst) => inst,
            // Undecodable byte: skip it and resynchronize, the same
            // recovery the window disassembler applies per window.
            Err(_) => {
                off += 1;
                continue;
            }
        };
        let len = LengthedInstruction::len(&inst).to_const() as usize;
        if len == 0 || off + len > bytes.len() {
            // A zero-length decode or an instruction running past the end
            // of the section is not trustworthy; advance one byte.
            off += 1;
            continue;
        }
        // The length-aware linear decode guarantees `off` is an instruction
        // boundary, so a 5-byte instruction opening with E8/E9 is exactly
        // the direct rel32 form: opcode byte, then a signed 32-bit
        // displacement relative to the next instruction.
        if len == 5 && (bytes[off] == 0xE8 || (include_jmp && bytes[off] == 0xE9)) {
            let disp = i32::from_le_bytes([
                bytes[off + 1],
                bytes[off + 2],
                bytes[off + 3],
                bytes[off + 4],
            ]);
            // `next` is at most the section length (well under i64::MAX) and
            // `disp` fits in i32, so the sum cannot overflow.
            let next = (off + 5) as i64;
            let target = next + i64::from(disp);
            if target >= 0 && (target as u64) < bytes.len() as u64 {
                targets.push(target as u64);
            }
        }
        off += len;
    }
    targets.sort_unstable();
    targets.dedup();
    targets
}

/// True when the bytes at a call-target site open with a PLT-stub shape,
/// the same exclusion the prologue scan applies to its candidates: an
/// `endbr64` followed by `jmp *GOT(%rip)` (plain or `bnd`), a bare
/// `jmp *GOT` / `push *GOT` lead, or the index-push form
/// `push imm32; jmp rel32`.
fn is_plt_stub_shape_at(site: &[u8]) -> bool {
    if site.starts_with(b"\xf3\x0f\x1e\xfa") && is_plt_stub_after_endbr64(&site[4..]) {
        return true;
    }
    if is_plt_stub_lead(site) {
        return true;
    }
    site.len() >= 6 && site.starts_with(b"\x68") && site[5] == 0xe9
}

/// The call-target hits that qualify as candidate function starts: of
/// [`collect_direct_branch_targets`]'s raw targets, only those whose first
/// [`PROLOGUE_DECODE_DEPTH`] bytes disassemble cleanly (a jump-table-looking
/// byte sequence fails here and yields no start) and whose site does not
/// open with a PLT-stub shape. Sorted and deduplicated.
fn call_target_hits(bytes: &[u8], include_jmp: bool) -> Vec<u64> {
    let decoder = InstDecoder::default();
    collect_direct_branch_targets(bytes, include_jmp)
        .into_iter()
        .filter(|&t| {
            let site = &bytes[t as usize..];
            prologue_decodes(&decoder, site) && !is_plt_stub_shape_at(site)
        })
        .collect()
}

/// The decode-aware overlap merge shared by every candidate source: sort,
/// deduplicate, drop candidates that are part of an earlier candidate's code
/// ([`merges_into`]) or whose first [`PROLOGUE_DECODE_DEPTH`] bytes fail to
/// disassemble cleanly, then give each survivor the range to the next
/// candidate (or the range end) and drop survivors shorter than
/// [`FUNCTION_MIN_SIZE`]. Returns `(offset, size)` pairs into `bytes`,
/// ordered by offset.
fn merge_and_size_candidates(bytes: &[u8], mut hits: Vec<u64>) -> Vec<(u64, u64)> {
    hits.sort_unstable();
    hits.dedup();

    let decoder = InstDecoder::default();
    let mut starts: Vec<u64> = Vec::new();
    for h in hits {
        if let Some(&last) = starts.last() {
            if merges_into(bytes, last, h) {
                continue;
            }
        }
        if prologue_decodes(&decoder, &bytes[h as usize..]) {
            starts.push(h);
        }
    }

    let mut out: Vec<(u64, u64)> = Vec::new();
    for (n, &start) in starts.iter().enumerate() {
        let next = starts.get(n + 1).copied().unwrap_or(bytes.len() as u64);
        let end = next.min(bytes.len() as u64);
        if end - start < FUNCTION_MIN_SIZE {
            continue;
        }
        out.push((start, end - start));
    }
    out
}

/// Scan one executable byte range for candidate function starts from the
/// prologue patterns alone. Returns `(offset, size)` pairs into `bytes`,
/// ordered by offset: `offset` is where the function starts, `size` runs to
/// the next candidate or the end of the range. Candidates that are part of
/// an earlier candidate's code (see [`merges_into`]) merge into it;
/// candidates whose first [`PROLOGUE_DECODE_DEPTH`] bytes fail to
/// disassemble cleanly, or whose slice is shorter than
/// [`FUNCTION_MIN_SIZE`], are dropped.
pub fn scan_prologue_candidates(bytes: &[u8], base_addr: u64) -> Vec<(u64, u64)> {
    merge_and_size_candidates(bytes, prologue_hits(bytes, base_addr))
}

/// The full stripped-binary candidate scan: prologue-pattern hits unioned
/// with direct call-target hits, run through the same decode-aware overlap
/// merge. A start found only by a call target gets the same synthesized
/// `func_<hex addr>` name scheme - the merge is address-based and treats
/// both sources identically, and a call target at an address a prologue
/// pattern already proposed collapses onto it.
pub fn scan_heuristic_candidates(bytes: &[u8], base_addr: u64) -> Vec<(u64, u64)> {
    let mut hits = prologue_hits(bytes, base_addr);
    hits.extend(call_target_hits(bytes, CALL_TARGETS_INCLUDE_JMP));
    merge_and_size_candidates(bytes, hits)
}

/// The heuristic scan's per-section pipeline, shared by every container:
/// run [`scan_heuristic_candidates`] (prologue patterns plus direct call
/// targets) over each section's file bytes and name each candidate
/// `func_<hex virtual address>` - `addr` is the section's own address
/// convention (ELF `sh_addr`, PE RVA, Mach-O virtual address), so a
/// heuristic address is directly comparable with what the same container's
/// symbol source reports. `sections` is the pre-filtered list of
/// executable, byte-backed sections in scan order; `bytes_of` fetches one
/// section's file bytes (clamped to the file extent). Returns functions
/// ordered by address.
fn heuristic_section_symbols<'a, I, F>(sections: I, bytes_of: F) -> Vec<FunctionSymbol>
where
    I: IntoIterator<Item = &'a SectionInfo>,
    F: Fn(&SectionInfo) -> Vec<u8>,
{
    let mut out: Vec<FunctionSymbol> = Vec::new();
    for section in sections {
        let bytes = bytes_of(section);
        if bytes.is_empty() {
            continue;
        }
        for (start, size) in scan_heuristic_candidates(&bytes, section.addr) {
            // Both additions saturate: `addr` is an untrusted header value
            // plus a scan offset, and a crafted header can carry u64::MAX
            // values.
            let addr = section.addr.saturating_add(start);
            out.push(FunctionSymbol {
                name: format!("func_{addr:x}"),
                addr,
                size,
                file_offset: section.offset.saturating_add(start as usize),
            });
        }
    }
    out.sort_by(|a, b| a.addr.cmp(&b.addr).then_with(|| a.name.cmp(&b.name)));
    out
}

/// The sections the heuristic scans in one container: executable
/// (`SHF_EXECINSTR` on ELF, the execute flag on PE, a `__TEXT` section on
/// Mach-O) with bytes actually in the file.
fn heuristic_eligible(section: &&SectionInfo) -> bool {
    section.executable && !section.no_bits && section.size > 0
}

/// Function boundaries for a stripped x86-64 ELF: run
/// [`scan_heuristic_candidates`] (prologue patterns plus direct call
/// targets) over every executable, byte-backed section. Returns functions
/// ordered by address. Empty when the binary exposes no executable section
/// bytes or the scan finds nothing.
pub fn heuristic_functions(info: &ElfInfo) -> Vec<FunctionSymbol> {
    heuristic_section_symbols(info.sections.iter().filter(heuristic_eligible), |s| {
        info.file_slice(s.offset, s.size)
    })
}

/// Function boundaries for a stripped x86-64 PE: run
/// [`scan_heuristic_candidates`] over every executable, byte-backed section
/// of the section table. Candidates align against and report as RVAs
/// (`SectionInfo::addr` is the section's `VirtualAddress`), while the bytes
/// scanned and the recorded file offsets come from the section's raw data
/// (`PointerToRawData`) - the same RVA-to-file-offset mapping
/// [`crate::peinfo::PeInfo::functions`] applies to exports, so a heuristic
/// address is directly comparable with an export-table address. Empty for a
/// non-x86-64 machine (the prologue patterns and the decoder read x86-64
/// only) and when the image exposes no executable section bytes or the scan
/// finds nothing. Returns functions ordered by address.
pub fn heuristic_functions_pe(info: &PeInfo) -> Vec<FunctionSymbol> {
    if !info.disassembles_x86_64() {
        return Vec::new();
    }
    heuristic_section_symbols(info.sections.iter().filter(heuristic_eligible), |s| {
        info.section_data(s.offset, s.size)
    })
}

/// The `__TEXT` sections the Mach-O heuristic may scan. Mach-O has no
/// per-section execute bit - `SectionInfo::executable` marks every
/// `__TEXT`-segment section, metadata included - and a prologue scan over
/// `__unwind_info`'s records or `__eh_frame`'s CFI entries can only misread
/// structured data as function starts. The code an x86-64 image carries
/// lives in `__text` plus the stub-helper thunks; `__stubs` is a PLT by
/// construction and is a no-op under the PLT-shape gates anyway.
const MACHO_HEURISTIC_SECTIONS: &[&str] = &["__TEXT,__text", "__TEXT,__stub_helper"];

/// Function boundaries for a stripped x86-64 Mach-O: run
/// [`scan_heuristic_candidates`] over the byte-backed code sections of the
/// `__TEXT` segment (see [`MACHO_HEURISTIC_SECTIONS`]). Candidates align
/// against and report as virtual addresses (`SectionInfo::addr` is
/// `sect.addr`), matching what the `LC_SYMTAB` source reports. Empty for a
/// non-x86-64 cputype (an arm64 image's boundaries are never guessed) and
/// when the image exposes no code section bytes or the scan finds nothing.
/// Returns functions ordered by address.
pub fn heuristic_functions_macho(info: &MachoInfo) -> Vec<FunctionSymbol> {
    if !info.disassembles_x86_64() {
        return Vec::new();
    }
    heuristic_section_symbols(
        info.sections.iter().filter(|s| {
            heuristic_eligible(s) && MACHO_HEURISTIC_SECTIONS.contains(&s.name.as_str())
        }),
        |s| info.section_data(s.offset, s.size),
    )
}

/// Per-function extraction from parsed ELF material. An unstripped ELF
/// lists its symbol-table functions. A stripped x86-64 ELF falls back to
/// [`heuristic_functions`] (with `source: FunctionSource::Heuristic`);
/// when even the heuristic finds nothing, the list stays empty with an
/// explicit reason. A stripped non-x86-64 ELF keeps its "needs an
/// unstripped ELF" reason - boundaries are never guessed for architectures
/// the scanner does not read. Every listed function's bytes are read
/// through [`ElfInfo::file_slice`].
pub fn extract_elf(info: &ElfInfo) -> FunctionsExtraction {
    let labels = Labels::from_elf(info);
    let build_id = info.build_id.clone();

    if info.functions.is_empty() && !info.has_symtab && info.labels.arch == "x86-64" {
        // Stripped x86-64: the boundary heuristic (prologue patterns plus
        // call targets) owns the boundaries.
        let functions = heuristic_functions(info)
            .into_iter()
            .map(|fs| FunctionFeatures {
                name: fs.name.clone(),
                addr: fs.addr,
                size: fs.size,
                features: function_features(&info.file_slice(fs.file_offset, fs.size)),
            })
            .collect::<Vec<_>>();
        let reason = functions.is_empty().then(|| {
            "no symbol table: binary looks stripped, and the stripped-binary \
             heuristic found no candidate function starts in the executable \
             sections"
                .to_string()
        });
        return FunctionsExtraction {
            build_id,
            labels,
            source: FunctionSource::Heuristic,
            functions,
            reason,
        };
    }

    let functions = if info.functions.is_empty() {
        Vec::new()
    } else {
        info.functions
            .iter()
            .map(|fs| FunctionFeatures {
                name: fs.name.clone(),
                addr: fs.addr,
                size: fs.size,
                features: function_features(&info.file_slice(fs.file_offset, fs.size)),
            })
            .collect()
    };
    let reason = if functions.is_empty() {
        Some(
            if info.has_symtab {
                "symbol table carries no usable function symbols (STT_FUNC with \
                 a size in an executable section)"
            } else {
                "no symbol table: binary looks stripped; function extraction \
                 needs an unstripped ELF"
            }
            .to_string(),
        )
    } else {
        None
    };
    FunctionsExtraction {
        build_id,
        labels,
        source: FunctionSource::SymbolTable,
        functions,
        reason,
    }
}

/// Per-function extraction from parsed PE material: the export directory is
/// the symbol source, the prologue heuristic the fallback. An image with
/// named exports in an executable section (DLLs, and executables that
/// export symbols) lists them with sizes derived from the export table -
/// the delta to the next export's RVA, the section end for the last one. A
/// plain executable with no export directory falls back to
/// [`heuristic_functions_pe`] for x86-64 (with
/// `source: FunctionSource::Heuristic`) and keeps an explicit reason for
/// any other machine; an export table that carries no usable function
/// exports is a reason, not a heuristic trigger, mirroring the ELF side's
/// symtab-present rule. Every listed function's bytes are read through
/// [`PeInfo::section_data`].
pub fn extract_pe(info: &PeInfo) -> FunctionsExtraction {
    let labels = Labels::from_pe(info);
    let build_id = info.build_id.clone();

    if info.functions.is_empty() && !info.has_exports && info.disassembles_x86_64() {
        // No export table, x86-64 code: the boundary heuristic owns the
        // boundaries.
        let functions = heuristic_functions_pe(info)
            .into_iter()
            .map(|fs| FunctionFeatures {
                name: fs.name.clone(),
                addr: fs.addr,
                size: fs.size,
                features: function_features(&info.section_data(fs.file_offset, fs.size)),
            })
            .collect::<Vec<_>>();
        let reason = functions.is_empty().then(|| {
            "no export table: binary looks stripped, and the stripped-binary \
             heuristic found no candidate function starts in the executable \
             sections"
                .to_string()
        });
        return FunctionsExtraction {
            build_id,
            labels,
            source: FunctionSource::Heuristic,
            functions,
            reason,
        };
    }

    let functions = info
        .functions
        .iter()
        .map(|fs| FunctionFeatures {
            name: fs.name.clone(),
            addr: fs.addr,
            size: fs.size,
            features: function_features(&info.section_data(fs.file_offset, fs.size)),
        })
        .collect::<Vec<_>>();
    let reason = functions.is_empty().then(|| {
        if info.has_exports {
            "export table carries no usable function exports (named exports \
             landing in an executable section)"
        } else {
            // Reached only for a non-x86-64 image: the x86-64 case returned
            // through the heuristic branch above.
            "no export table: binary looks stripped; the stripped-binary \
             heuristic reads x86-64 only, so function extraction needs \
             exported symbols"
        }
        .to_string()
    });
    FunctionsExtraction {
        build_id,
        labels,
        source: FunctionSource::SymbolTable,
        functions,
        reason,
    }
}

/// How many bytes a heuristic start may sit from a symbol-table start and
/// still count as the same function in [`measure_boundaries`].
pub const MEASURE_TOLERANCE: u64 = 8;

/// Aggregate result of measuring the prologue heuristic against
/// symbol-table ground truth on one unstripped binary.
#[derive(Debug, Clone, Serialize)]
pub struct BoundaryMetrics {
    /// Distinct symbol-table function starts (ground truth).
    pub symbol_starts: usize,
    /// Distinct heuristic starts the scan produced.
    pub heuristic_starts: usize,
    /// Heuristic starts with a symbol start within `tolerance` bytes.
    pub matched: usize,
    /// The match tolerance in bytes.
    pub tolerance: u64,
    /// `matched / heuristic_starts` (0.0 when the scan found nothing).
    pub precision: f64,
    /// Share of symbol starts the scan found within `tolerance`.
    pub recall: f64,
    /// Harmonic mean of precision and recall.
    pub f1: f64,
}

/// Count elements of `a` with some element of `b` within `tol` bytes.
/// Both slices must be sorted; a `b` element may back several `a`
/// elements (with tolerance 8, two `a` starts 16 apart can both reach the
/// same `b`), which is the honest reading of "start within tolerance".
fn count_within(a: &[u64], b: &[u64], tol: u64) -> usize {
    let mut j = 0usize;
    let mut n = 0usize;
    for &x in a {
        while j < b.len() && b[j] < x.saturating_sub(tol) {
            j += 1;
        }
        if j < b.len() && b[j] <= x.saturating_add(tol) {
            n += 1;
        }
    }
    n
}

/// Measure the prologue heuristic against symbol-table ground truth on one
/// parsed ELF: precision (share of heuristic starts that sit within
/// [`MEASURE_TOLERANCE`] bytes of a symbol start), recall (share of symbol
/// starts found), and their F1. Errors for a non-x86-64 ELF or one without
/// symbol-table ground truth (a stripped binary has nothing to measure
/// against).
pub fn measure_boundaries(info: &ElfInfo) -> Result<BoundaryMetrics> {
    if info.labels.arch != "x86-64" {
        bail!(
            "boundary measurement is implemented for x86-64 ELF only (this binary is {})",
            info.labels.arch
        );
    }
    if info.function_starts.is_empty() {
        bail!(
            "no symbol-table ground truth to measure against: pass an \
             unstripped ELF with STT_FUNC symbols (a stripped binary has no \
             boundary truth)"
        );
    }

    let truth: &[u64] = &info.function_starts;
    let mut heur: Vec<u64> = heuristic_functions(info)
        .into_iter()
        .map(|f| f.addr)
        .collect();
    heur.sort_unstable();
    heur.dedup();

    let matched_heur = count_within(&heur, truth, MEASURE_TOLERANCE);
    let matched_truth = count_within(truth, &heur, MEASURE_TOLERANCE);
    let precision = if heur.is_empty() {
        0.0
    } else {
        matched_heur as f64 / heur.len() as f64
    };
    let recall = if truth.is_empty() {
        0.0
    } else {
        matched_truth as f64 / truth.len() as f64
    };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };
    Ok(BoundaryMetrics {
        symbol_starts: truth.len(),
        heuristic_starts: heur.len(),
        matched: matched_heur,
        tolerance: MEASURE_TOLERANCE,
        precision,
        recall,
        f1,
    })
}

/// Per-function extraction from parsed Mach-O material: the `LC_SYMTAB`
/// symbol table is the symbol source, the prologue heuristic the fallback.
/// An unstripped image lists its defined, non-stab symbols in executable
/// `__TEXT` sections (raw names with the ABI's leading underscore; sizes
/// derived from the next symbol's value - see
/// [`crate::machoinfo::MachoInfo::functions`]). A stripped image falls back
/// to [`heuristic_functions_macho`] for x86-64 (with
/// `source: FunctionSource::Heuristic`) and keeps an explicit reason for
/// any other architecture; a symtab that holds no usable functions is a
/// reason, not a heuristic trigger, mirroring the ELF side's
/// symtab-present rule. Every listed function's bytes are read through
/// [`MachoInfo::section_data`].
pub fn extract_macho(info: &MachoInfo) -> FunctionsExtraction {
    let labels = Labels::from_macho(info);
    let build_id = info.build_id.clone();

    if info.functions.is_empty() && !info.has_symtab && info.disassembles_x86_64() {
        // No symbol table, x86-64 code: the boundary heuristic owns the
        // boundaries.
        let functions = heuristic_functions_macho(info)
            .into_iter()
            .map(|fs| FunctionFeatures {
                name: fs.name.clone(),
                addr: fs.addr,
                size: fs.size,
                features: function_features(&info.section_data(fs.file_offset, fs.size)),
            })
            .collect::<Vec<_>>();
        let reason = functions.is_empty().then(|| {
            "no symbol table: binary looks stripped, and the stripped-binary \
             heuristic found no candidate function starts in the __TEXT code \
             sections"
                .to_string()
        });
        return FunctionsExtraction {
            build_id,
            labels,
            source: FunctionSource::Heuristic,
            functions,
            reason,
        };
    }

    let functions = info
        .functions
        .iter()
        .map(|fs| FunctionFeatures {
            name: fs.name.clone(),
            addr: fs.addr,
            size: fs.size,
            features: function_features(&info.section_data(fs.file_offset, fs.size)),
        })
        .collect::<Vec<_>>();
    let reason = functions.is_empty().then(|| {
        if info.has_symtab {
            "symbol table carries no usable function symbols (defined, \
             non-stab symbols in an executable __TEXT section with a \
             derived size)"
        } else {
            // Reached only for a non-x86-64 image: the x86-64 case returned
            // through the heuristic branch above.
            "no symbol table: binary looks stripped; the stripped-binary \
             heuristic reads x86-64 only, so function extraction needs an \
             unstripped Mach-O"
        }
        .to_string()
    });
    FunctionsExtraction {
        build_id,
        labels,
        source: FunctionSource::SymbolTable,
        functions,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_features_hold_only_scoped_namespaces() {
        // 64 bytes of plausible code: push rbp; mov rbp, rsp; mov eax, 0x2a;
        // pop rbp; ret; nop padding.
        let mut bytes = vec![
            0x55, 0x48, 0x89, 0xe5, 0xb8, 0x2a, 0x00, 0x00, 0x00, 0x5d, 0xc3,
        ];
        bytes.resize(64, 0x90);
        let f = function_features(&bytes);

        assert_eq!(f.get("fn:6"), Some(&1), "log2(64) = 6");
        assert_eq!(f.get("meta:windows"), Some(&1));
        assert_eq!(f.get("mn:push"), Some(&1));
        assert_eq!(f.get("cn:2a"), Some(&1));
        // 64 bytes -> 62 3-grams.
        let total: u64 = f
            .iter()
            .filter(|(k, _)| k.starts_with("ng:"))
            .map(|(_, v)| v)
            .sum();
        assert_eq!(total, 62);
        // No container-only namespaces leak into a function's features.
        assert!(!f.keys().any(|k| k.starts_with("sec:")));
        assert!(!f.keys().any(|k| k.starts_with("ent:")));
        assert!(!f.keys().any(|k| k.starts_with("cmt:")));
    }

    #[test]
    fn size_bucket_floors_and_clamps() {
        assert_eq!(size_bucket_key(0), "fn:0");
        assert_eq!(size_bucket_key(1), "fn:0");
        assert_eq!(size_bucket_key(64), "fn:6");
        assert_eq!(size_bucket_key(4096), "fn:12");
        assert_eq!(size_bucket_key(1 << 24), "fn:24");
        assert_eq!(size_bucket_key(1 << 30), "fn:24", "clamped at 24");
    }

    #[test]
    fn short_functions_still_carry_the_size_bucket() {
        // ret alone: 1 byte, no 3-grams, no clean window, but fn:0 present.
        let f = function_features(&[0xc3]);
        assert!(!f.contains_key("ng:c3"), "no 3-gram fits in one byte");
        assert_eq!(f.get("meta:windows"), Some(&0));
        assert_eq!(f.get("fn:0"), Some(&1));
    }

    // ---- prologue scanner, on synthetic .text blobs ----

    /// 16 bytes of plausible filler: `mov eax, 0; nop`-shaped bytes that
    /// decode cleanly and match no prologue pattern.
    fn filler() -> [u8; 16] {
        [
            0xb8, 0x00, 0x00, 0x00, 0x00, // mov eax, 0
            0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, 0x90, // nops
        ]
    }

    #[test]
    fn finds_an_aligned_frame_pointer_prologue() {
        // 0x06 (push es) is invalid in 64-bit mode, so the blob's first
        // byte is not a section-start candidate; the prologue at offset 16
        // is the only hit.
        let mut text = vec![0x06];
        text.extend_from_slice(&filler()[1..]);
        text.extend_from_slice(&[0x55, 0x48, 0x89, 0xe5]); // push rbp; mov rbp,rsp
        text.extend_from_slice(&[0xb8, 0x2a, 0x00, 0x00, 0x00, 0xc3]); // mov eax,0x2a; ret
        text.resize(48, 0x90);
        let found = scan_prologue_candidates(&text, 0);
        assert_eq!(
            found,
            vec![(16, 32)],
            "one candidate at offset 16 (aligned)"
        );
    }

    #[test]
    fn section_start_is_always_a_candidate() {
        // The linker places a function at an executable section start even
        // when the first bytes match no prologue pattern.
        let mut text = filler().to_vec();
        text.extend_from_slice(&[0x48, 0x8d, 0x3d, 0x11, 0x2b, 0x00, 0x00]); // lea rip+0x2b11,rdi
        text.resize(48, 0x90);
        assert_eq!(scan_prologue_candidates(&text, 0), vec![(0, 48)]);
    }

    #[test]
    fn rejects_a_frame_pointer_prologue_off_alignment() {
        // Same bytes, but the prologue sits at offset 1: no candidates
        // anywhere (0x06 kills the section start, the prologue is
        // misaligned).
        let mut text = vec![0x06, 0x55, 0x48, 0x89, 0xe5];
        text.extend_from_slice(&[0xb8, 0x2a, 0x00, 0x00, 0x00, 0xc3]);
        text.resize(24, 0x90);
        assert!(
            scan_prologue_candidates(&text, 0).is_empty(),
            "misaligned push rbp/mov rbp,rsp is not a candidate"
        );
    }

    #[test]
    fn finds_an_endbr64_site_at_any_alignment() {
        // endbr64 at a deliberately unaligned offset, then `push rbp`
        // 4 bytes later (within the 8-byte followup window).
        let mut text = vec![0x06];
        text.extend_from_slice(&[0xf3, 0x0f, 0x1e, 0xfa]); // endbr64 @1
        text.extend_from_slice(&[0x55]); // push rbp @5
        text.extend_from_slice(&[0x48, 0x89, 0xe5]); // mov rbp,rsp
        text.extend_from_slice(&[0x5d, 0xc3]); // pop rbp; ret
        text.resize(32, 0x90);
        let found = scan_prologue_candidates(&text, 0);
        assert_eq!(found, vec![(1, 31)], "endbr64 site accepted unaligned");
    }

    #[test]
    fn endbr64_alignment_and_followup_rules() {
        // Aligned endbr64 with no frame setup (the glibc -O2 idiom) is a
        // candidate; the same bytes off alignment (and off the section
        // start) are not.
        let mut text = vec![0xf3, 0x0f, 0x1e, 0xfa, 0x48, 0x89, 0xf8]; // endbr64; mov rax,rdi
        text.extend_from_slice(&[0x48, 0x01, 0xf8, 0xc3]); // add rax,rdi; ret
        text.resize(32, 0x90);
        assert_eq!(
            scan_prologue_candidates(&text, 0),
            vec![(0, 32)],
            "aligned endbr64 without frame setup is an entry"
        );
        let mut unaligned = vec![0x06];
        unaligned.extend_from_slice(&text[..text.len() - 1]);
        assert!(
            scan_prologue_candidates(&unaligned, 1).is_empty(),
            "unaligned endbr64 without frame setup is not a candidate"
        );
    }

    #[test]
    fn rejects_a_plt_stub() {
        // endbr64; jmp *GOT(%rip) at a 16-aligned site is a PLT stub, not
        // a function entry - neither as a pattern hit nor as a section
        // start.
        let mut text = vec![0xf3, 0x0f, 0x1e, 0xfa, 0xff, 0x25, 0x02, 0x00];
        text.resize(32, 0x90);
        assert!(
            scan_prologue_candidates(&text, 0).is_empty(),
            "PLT stub shape must not become a function"
        );
        let mut bnd = vec![0xf3, 0x0f, 0x1e, 0xfa, 0xf2, 0xff, 0x25, 0x02, 0x00];
        bnd.resize(32, 0x90);
        assert!(
            scan_prologue_candidates(&bnd, 0).is_empty(),
            "bnd jmp form too"
        );
    }

    #[test]
    fn finds_a_push_rbx_prologue_only_aligned() {
        let prologue = [0x53, 0x48, 0x89, 0xfb]; // push rbx; mov rbx,rdi
        let mut aligned = vec![0x06];
        aligned.extend_from_slice(&filler()[1..]);
        aligned.extend_from_slice(&prologue);
        aligned.resize(48, 0x90);
        assert_eq!(
            scan_prologue_candidates(&aligned, 0),
            vec![(16, 32)],
            "aligned push-rbx prologue found"
        );

        let mut unaligned = vec![0x06, 0x90];
        unaligned.extend_from_slice(&prologue);
        unaligned.resize(24, 0x90);
        assert!(
            scan_prologue_candidates(&unaligned, 0).is_empty(),
            "push-rbx prologue is aligned-only"
        );
    }

    #[test]
    fn merges_candidates_closer_than_sixteen_bytes() {
        // A frame-pointer prologue at offset 16 and an endbr64 at 20 (the
        // CET form of the same entry): one function, at the earlier start.
        let mut text = vec![0x06];
        text.extend_from_slice(&filler()[1..]);
        text.extend_from_slice(&[0x55, 0x48, 0x89, 0xe5]); // @16
        text.extend_from_slice(&[0xf3, 0x0f, 0x1e, 0xfa]); // @20 endbr64
        text.extend_from_slice(&[0x48, 0x83, 0xec, 0x08]); // sub rsp,8
        text.resize(64, 0x90);
        let found = scan_prologue_candidates(&text, 0);
        assert_eq!(found, vec![(16, 48)]);
    }

    #[test]
    fn drops_a_candidate_that_fails_to_decode() {
        // The prologue bytes are present, but 0xff 0xff (an invalid FF /7
        // opcode) inside the first 16 bytes kills the candidate.
        let mut text = vec![0x06];
        text.extend_from_slice(&filler()[1..]);
        text.extend_from_slice(&[0x55, 0x48, 0x89, 0xe5]);
        text.extend_from_slice(&[0xff; 12]);
        assert!(scan_prologue_candidates(&text, 0).is_empty());
    }

    #[test]
    fn drops_a_candidate_with_under_sixteen_bytes_to_the_end() {
        let mut text = vec![0x06];
        text.extend_from_slice(&filler()[1..]);
        text.extend_from_slice(&[0x55, 0x48, 0x89, 0xe5, 0xc3, 0x90]);
        // 22 bytes total: the candidate at 16 holds only 6 bytes.
        assert!(scan_prologue_candidates(&text, 0).is_empty());
    }

    #[test]
    fn consecutive_candidates_split_the_range() {
        // Two aligned frame-pointer functions back to back: each runs to
        // the next candidate.
        let mut text = vec![0x06];
        text.extend_from_slice(&filler()[1..]);
        text.extend_from_slice(&[0x55, 0x48, 0x89, 0xe5]);
        text.resize(48, 0x90);
        text.extend_from_slice(&[0x55, 0x48, 0x89, 0xe5]); // @48
        text.resize(80, 0x90);
        let found = scan_prologue_candidates(&text, 0);
        assert_eq!(found, vec![(16, 32), (48, 32)]);
    }

    // ---- call-target collector, on synthetic .text blobs ----

    /// 16 bytes of plausible filler prefixed by one invalid byte, so the
    /// section-start candidate never survives: the sweep starts decoding at
    /// offset 1 and reaches offset 16 on instruction boundaries.
    fn sweepable_prefix() -> Vec<u8> {
        let mut text = vec![0x06]; // push es: invalid in 64-bit mode
        text.extend_from_slice(&filler()[1..]);
        text
    }

    #[test]
    fn a_direct_call_yields_its_target_as_a_start() {
        let mut text = sweepable_prefix();
        // call +11 at offset 16: next instruction at 21, target 21 + 11 = 32.
        text.extend_from_slice(&[0xe8, 0x0b, 0x00, 0x00, 0x00]);
        text.resize(32, 0x90);
        // The target opens with bytes matching no prologue pattern, so only
        // call-target analysis can propose it.
        text.extend_from_slice(&[0x31, 0xc0, 0xb8, 0x2a, 0x00, 0x00, 0x00, 0xc3]); // xor eax,eax; mov eax,0x2a; ret
        text.resize(64, 0x90);

        assert_eq!(
            collect_direct_branch_targets(&text, false),
            vec![32],
            "the E8 at offset 16 targets offset 32"
        );
        assert!(
            scan_prologue_candidates(&text, 0).is_empty(),
            "no prologue pattern fires on this blob"
        );
        assert_eq!(
            scan_heuristic_candidates(&text, 0),
            vec![(32, 32)],
            "the call target becomes the only function start"
        );
    }

    #[test]
    fn a_target_that_fails_the_decode_check_yields_nothing() {
        let mut text = sweepable_prefix();
        // call +11 at offset 16: target 32, which holds a jump-table-looking
        // island (ff-ff 00-00 entries) that cannot disassemble.
        text.extend_from_slice(&[0xe8, 0x0b, 0x00, 0x00, 0x00]);
        text.resize(32, 0x90);
        text.extend_from_slice(&[0xff, 0xff, 0x00, 0x00]); // x4: jump-table-like
        text.extend_from_slice(&[0xff, 0xff, 0x00, 0x00]);
        text.extend_from_slice(&[0xff, 0xff, 0x00, 0x00]);
        text.extend_from_slice(&[0xff, 0xff, 0x00, 0x00]);
        text.resize(64, 0x90);

        assert_eq!(
            collect_direct_branch_targets(&text, false),
            vec![32],
            "the raw E8 target is collected at offset 32"
        );
        assert!(
            scan_heuristic_candidates(&text, 0).is_empty(),
            "undecodable target bytes are filtered before the merge"
        );
    }

    #[test]
    fn a_call_target_at_a_prologue_address_collapses_onto_it() {
        let mut text = sweepable_prefix();
        // call +11 at offset 16: target 32, which also carries an aligned
        // frame-pointer prologue - both sources propose the same start.
        text.extend_from_slice(&[0xe8, 0x0b, 0x00, 0x00, 0x00]);
        text.resize(32, 0x90);
        text.extend_from_slice(&[0x55, 0x48, 0x89, 0xe5]); // push rbp; mov rbp,rsp @32
        text.extend_from_slice(&[0xb8, 0x2a, 0x00, 0x00, 0x00, 0xc3]); // mov eax,0x2a; ret
        text.resize(64, 0x90);

        assert_eq!(
            scan_prologue_candidates(&text, 0),
            vec![(32, 32)],
            "the prologue scan alone finds offset 32"
        );
        assert_eq!(
            scan_heuristic_candidates(&text, 0),
            vec![(32, 32)],
            "the union collapses the duplicate into one start"
        );
    }

    #[test]
    fn a_call_target_inside_a_prologue_start_merges_into_it() {
        // A call whose target sits 4 bytes after an aligned prologue start,
        // with no terminator between: the target is part of the prologue
        // start's code, so the merge keeps only the earlier start.
        let mut text = vec![0x06];
        text.extend_from_slice(&[0xb8, 0x00, 0x00, 0x00, 0x00]); // mov eax,0 @1 (len 5)
        text.extend_from_slice(&[0x90, 0x90]); // @6
        text.extend_from_slice(&[0xe8, 0x07, 0x00, 0x00, 0x00]); // call +7 @8: next 13, target 20
        text.extend_from_slice(&[0x90, 0x90, 0x90]); // @13
        text.extend_from_slice(&[0x55, 0x48, 0x89, 0xe5]); // push rbp; mov rbp,rsp @16
        text.extend_from_slice(&[0xb8, 0x2a, 0x00, 0x00, 0x00, 0xc3]); // @20: mov eax,0x2a; ret
        text.resize(64, 0x90);

        assert_eq!(
            collect_direct_branch_targets(&text, false),
            vec![20],
            "the raw call target is offset 20"
        );
        assert_eq!(
            scan_heuristic_candidates(&text, 0),
            vec![(16, 48)],
            "the target merges into the prologue start at 16; one function"
        );
    }

    #[test]
    fn branch_target_collection_follows_the_jmp_switch() {
        // The same call/jmp opcode placement with the include-jmp switch on
        // and off: E8 targets are always collected, E9 targets only when
        // asked (the shipped mode is decided by the sweep and pinned in
        // CALL_TARGETS_INCLUDE_JMP).
        let mut text = sweepable_prefix();
        text.extend_from_slice(&[0xe8, 0x0b, 0x00, 0x00, 0x00]); // call +11 @16
        text.extend_from_slice(&[0xe9, 0x0b, 0x00, 0x00, 0x00]); // jmp +11 @21
        text.resize(64, 0x90);
        // 0x06-style fillers end; make the tail decode cleanly.
        text.extend_from_slice(&[0x31, 0xc0, 0xc3]); // xor eax,eax; ret
        text.resize(96, 0x90);

        assert_eq!(
            collect_direct_branch_targets(&text, false),
            vec![32],
            "E8-only: one target"
        );
        // jmp @21: next 26, +11 -> target 37. Both targets collected in the
        // one set, sorted.
        assert_eq!(
            collect_direct_branch_targets(&text, true),
            vec![32, 37],
            "E8+E9: both direct rel32 targets"
        );
    }

    #[test]
    fn start_collection_is_deterministic() {
        // The same blob through the union scan twice, and through the raw
        // collector twice: identical start sets, no order leakage from the
        // hit sources (prologue hits land after call targets in the union's
        // input order; the merge sorts).
        let mut text = sweepable_prefix();
        text.extend_from_slice(&[0xe8, 0x0b, 0x00, 0x00, 0x00]); // call @16 -> 32
        text.resize(32, 0x90);
        text.extend_from_slice(&[0x31, 0xc0, 0xb8, 0x2a, 0x00, 0x00, 0x00, 0xc3]);
        text.resize(64, 0x90);
        text.extend_from_slice(&[0x55, 0x48, 0x89, 0xe5]); // aligned prologue @64
        text.resize(96, 0x90);

        let first = scan_heuristic_candidates(&text, 0);
        let second = scan_heuristic_candidates(&text, 0);
        assert_eq!(first, second, "same input, same start set");
        assert_eq!(first, vec![(32, 32), (64, 32)]);
        assert_eq!(
            collect_direct_branch_targets(&text, false),
            collect_direct_branch_targets(&text, false),
        );
    }

    #[test]
    fn measure_counts_matches_within_tolerance() {
        let truth = [100u64, 200, 1000];
        let heur = [104u64, 500, 995, 1100];
        // 104 ~ 100 (delta 4), 995 ~ 1000 (delta 5); 500 and 1100 unmatched.
        assert_eq!(count_within(&heur, &truth, 8), 2);
        assert_eq!(count_within(&truth, &heur, 8), 2);
        assert_eq!(count_within(&heur, &truth, 0), 0);
    }
}
