//! Stripped-binary heuristic tests for PE and Mach-O: the prologue-plus-
//! call-target scan in `src/functions.rs` extended past ELF. The heuristic
//! owns the boundaries when the container carries no symbol source at all -
//! a PE with no export directory, a Mach-O with no `LC_SYMTAB` - and stays
//! out of the way otherwise. No stripped PE or Mach-O ships as a fixture
//! (the committed PE fixture is a DLL with exports; the Mach-O fixtures
//! keep their symbol tables), so the stripped views are synthesized here
//! byte-by-byte: minimum images goblin parses, minus any symbol source.
//! The committed x86-64 Mach-O fixture backs the address-convention and
//! section-filter checks with real bytes.

use compid::{functions, machoinfo, peinfo};
use std::path::Path;
use std::sync::Arc;

const FILE_ALIGN: u32 = 0x200;
const SECT_ALIGN: u32 = 0x1000;
const HEADERS_SIZE: u32 = 0x400;
const TEXT_RAW_PTR: u32 = 0x400;
const TEXT_VA: u32 = 0x1000;

/// The `.text` raw size for `text_len` code bytes: padded to the file
/// alignment, the same padding the section's raw data carries in the file.
fn pe_raw_size(text_len: usize) -> u32 {
    (text_len as u32).div_ceil(FILE_ALIGN) * FILE_ALIGN
}

/// Minimal PE32+ with one `.text` section and all sixteen data directories
/// zero: no export table, no imports - the plain-executable view the
/// heuristic path exists for. Layout mirrors the synthetic-PE feature tests
/// (DOS header, PE signature, COFF header, PE32+ optional header, section
/// table, raw data at 0x400).
fn stripped_pe(machine: u16, text: &[u8]) -> Vec<u8> {
    let raw = pe_raw_size(text.len());
    let image_size = (TEXT_VA + raw).div_ceil(SECT_ALIGN) * SECT_ALIGN;

    let mut out = Vec::new();

    // DOS header: magic at 0, e_lfanew at 0x3c.
    out.extend_from_slice(b"MZ");
    out.extend_from_slice(&[0u8; 58]);
    out.extend_from_slice(&0x40u32.to_le_bytes());
    assert_eq!(out.len(), 64);

    // PE signature + COFF header.
    out.extend_from_slice(b"PE\0\0");
    out.extend_from_slice(&machine.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // one section
    out.extend_from_slice(&0u32.to_le_bytes()); // time/date stamp
    out.extend_from_slice(&0u32.to_le_bytes()); // pointer to symbol table
    out.extend_from_slice(&0u32.to_le_bytes()); // number of symbols
    out.extend_from_slice(&240u16.to_le_bytes()); // size of optional header
    out.extend_from_slice(&0x0022u16.to_le_bytes()); // executable image, large address aware

    // Optional header, PE32+ flavor.
    out.extend_from_slice(&0x020bu16.to_le_bytes()); // PE32+ magic
    out.push(14); // major linker version
    out.push(0); // minor linker version
    out.extend_from_slice(&raw.to_le_bytes()); // size of code
    out.extend_from_slice(&0u32.to_le_bytes()); // size of initialized data
    out.extend_from_slice(&0u32.to_le_bytes()); // size of uninitialized data
    out.extend_from_slice(&TEXT_VA.to_le_bytes()); // entry point RVA
    out.extend_from_slice(&TEXT_VA.to_le_bytes()); // base of code
    out.extend_from_slice(&0x140000000u64.to_le_bytes()); // image base
    out.extend_from_slice(&SECT_ALIGN.to_le_bytes());
    out.extend_from_slice(&FILE_ALIGN.to_le_bytes());
    out.extend_from_slice(&6u16.to_le_bytes()); // major OS version
    out.extend_from_slice(&0u16.to_le_bytes()); // minor OS version
    out.extend_from_slice(&0u16.to_le_bytes()); // major image version
    out.extend_from_slice(&0u16.to_le_bytes()); // minor image version
    out.extend_from_slice(&6u16.to_le_bytes()); // major subsystem version
    out.extend_from_slice(&0u16.to_le_bytes()); // minor subsystem version
    out.extend_from_slice(&0u32.to_le_bytes()); // win32 version value
    out.extend_from_slice(&image_size.to_le_bytes());
    out.extend_from_slice(&HEADERS_SIZE.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // checksum
    out.extend_from_slice(&3u16.to_le_bytes()); // subsystem: console
    out.extend_from_slice(&0u16.to_le_bytes()); // DLL characteristics
    out.extend_from_slice(&0x10_0000u64.to_le_bytes()); // stack reserve
    out.extend_from_slice(&0x1000u64.to_le_bytes()); // stack commit
    out.extend_from_slice(&0x10_0000u64.to_le_bytes()); // heap reserve
    out.extend_from_slice(&0x1000u64.to_le_bytes()); // heap commit
    out.extend_from_slice(&0u32.to_le_bytes()); // loader flags
    out.extend_from_slice(&16u32.to_le_bytes()); // number of RVA and sizes
    for _ in 0..16 {
        out.extend_from_slice(&0u64.to_le_bytes()); // no export table, no imports
    }
    assert_eq!(out.len(), 64 + 4 + 20 + 240);

    // One section header: .text, code / execute / read.
    out.extend_from_slice(b".text\0\0\0");
    out.extend_from_slice(&(text.len() as u32).to_le_bytes()); // virtual size
    out.extend_from_slice(&TEXT_VA.to_le_bytes()); // virtual address (RVA)
    out.extend_from_slice(&raw.to_le_bytes()); // raw size
    out.extend_from_slice(&TEXT_RAW_PTR.to_le_bytes()); // pointer to raw data
    out.extend_from_slice(&0u32.to_le_bytes()); // pointer to relocations
    out.extend_from_slice(&0u32.to_le_bytes()); // pointer to line numbers
    out.extend_from_slice(&0u16.to_le_bytes()); // number of relocations
    out.extend_from_slice(&0u16.to_le_bytes()); // number of line numbers
    out.extend_from_slice(&0x6000_0020u32.to_le_bytes()); // code, execute, read

    // Pad the headers, then the section's raw data.
    out.resize(TEXT_RAW_PTR as usize, 0);
    out.extend_from_slice(text);
    out.resize((TEXT_RAW_PTR + raw) as usize, 0);
    out
}

const MH_MAGIC_64: u32 = 0xfeed_facf;
const CPU_TYPE_X86_64: u32 = 0x0100_0007;
const CPU_TYPE_ARM64: u32 = 0x0100_000c;
const MH_EXECUTE: u32 = 2;
const LC_SEGMENT_64: u32 = 0x19;
const MACHO_TEXT_OFF: u32 = 0x1000; // where __text's bytes sit in the file
const MACHO_TEXT_ADDR: u64 = 0x1_0000_1000; // where __text maps in memory

/// Minimal 64-bit Mach-O: one `LC_SEGMENT_64` naming a `__TEXT` segment
/// with a single `__text` section and no `LC_SYMTAB` - the stripped view
/// the heuristic path exists for. Field layout mirrors the committed
/// fixtures (see tests/fixtures/macho_x86_64_sample), at miniature scale.
fn stripped_macho(cputype: u32, cpusubtype: u32, text: &[u8]) -> Vec<u8> {
    let segment_extent = MACHO_TEXT_OFF as u64 + text.len() as u64;
    let mut segname = [0u8; 16];
    segname[..6].copy_from_slice(b"__TEXT");

    let mut out = Vec::new();

    // Mach-O 64 header.
    out.extend_from_slice(&MH_MAGIC_64.to_le_bytes());
    out.extend_from_slice(&cputype.to_le_bytes());
    out.extend_from_slice(&cpusubtype.to_le_bytes());
    out.extend_from_slice(&MH_EXECUTE.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // ncmds
    out.extend_from_slice(&(72u32 + 80).to_le_bytes()); // sizeofcmds
    out.extend_from_slice(&0u32.to_le_bytes()); // flags
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    assert_eq!(out.len(), 32);

    // LC_SEGMENT_64: __TEXT, one section.
    out.extend_from_slice(&LC_SEGMENT_64.to_le_bytes());
    out.extend_from_slice(&(72u32 + 80).to_le_bytes()); // cmdsize
    out.extend_from_slice(&segname); // segname
    out.extend_from_slice(&0x1_0000_0000u64.to_le_bytes()); // vmaddr
    out.extend_from_slice(&segment_extent.to_le_bytes()); // vmsize
    out.extend_from_slice(&0u64.to_le_bytes()); // fileoff
    out.extend_from_slice(&segment_extent.to_le_bytes()); // filesize
    out.extend_from_slice(&7u32.to_le_bytes()); // maxprot
    out.extend_from_slice(&5u32.to_le_bytes()); // initprot
    out.extend_from_slice(&1u32.to_le_bytes()); // nsects
    out.extend_from_slice(&0u32.to_le_bytes()); // flags

    // section_64: __TEXT,__text at a 16-aligned virtual address.
    let mut sectname = [0u8; 16];
    sectname[..6].copy_from_slice(b"__text");
    out.extend_from_slice(&sectname);
    out.extend_from_slice(&segname);
    out.extend_from_slice(&MACHO_TEXT_ADDR.to_le_bytes()); // addr
    out.extend_from_slice(&(text.len() as u64).to_le_bytes()); // size
    out.extend_from_slice(&MACHO_TEXT_OFF.to_le_bytes()); // offset
    out.extend_from_slice(&4u32.to_le_bytes()); // align: 2^4 = 16
    out.extend_from_slice(&0u32.to_le_bytes()); // reloff
    out.extend_from_slice(&0u32.to_le_bytes()); // nreloc
    out.extend_from_slice(&0x8000_0400u32.to_le_bytes()); // S_REGULAR, code attrs
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved1
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved2
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved3
    assert_eq!(out.len(), 32 + 72 + 80);

    // The section's bytes at their declared file offset.
    out.resize(MACHO_TEXT_OFF as usize, 0);
    out.extend_from_slice(text);
    out
}

/// Two 64-byte functions: `alpha` opens with the aligned push rbp/mov rbp,
/// rsp prologue and a 0x2a immediate, `beta` with an aligned endbr64 and a
/// 0x1f immediate; nop padding between, so the boundary-straddling n-grams
/// belong to neither.
fn two_functions() -> Vec<u8> {
    let mut text = vec![
        0x55, 0x48, 0x89, 0xe5, 0xb8, 0x2a, 0x00, 0x00, 0x00, 0x5d, 0xc3,
    ];
    while text.len() < 64 {
        text.push(0x90);
    }
    text.extend_from_slice(&[
        0xf3, 0x0f, 0x1e, 0xfa, 0x55, 0x48, 0x89, 0xe5, 0xb8, 0x1f, 0x00, 0x00, 0x00, 0x5d, 0xc3,
    ]);
    while text.len() < 128 {
        text.push(0x90);
    }
    text
}

/// Every heuristic function's n-gram budget matches its own byte count:
/// `size - 2` 3-grams out of `size` bytes, so no outside bytes leaked in.
fn assert_ngrams_scoped(f: &functions::FunctionFeatures) {
    let total: u64 = f
        .features
        .iter()
        .filter(|(k, _)| k.starts_with("ng:"))
        .map(|(_, v)| v)
        .sum();
    assert_eq!(total, f.size - 2, "{}: n-gram total = {total}", f.name);
}

// ---- PE: no export table, x86-64 -> the heuristic owns the boundaries ----

/// A plain x86-64 executable (no export table) falls back to the prologue
/// heuristic: both prologue-marked functions are found with synthesized
/// RVA names, correct boundaries, and strictly scoped features. The RVA
/// assertion is load-bearing: a scan keyed on `PointerToRawData` instead of
/// `VirtualAddress` would have named them `func_410`/`func_450`.
#[test]
fn stripped_pe_yields_heuristic_functions() {
    let text = two_functions();
    let raw = pe_raw_size(text.len());
    let info = peinfo::parse(Arc::new(stripped_pe(0x8664, &text))).expect("synthetic PE parses");
    assert_eq!(info.labels.arch, "x86_64");
    assert!(!info.has_exports);
    assert!(
        info.functions.is_empty(),
        "no export table, no export symbols"
    );

    let ex = functions::extract_pe(&info);
    assert_eq!(ex.source, functions::FunctionSource::Heuristic);
    assert!(ex.reason.is_none(), "reason: {:?}", ex.reason);
    assert_eq!(ex.functions.len(), 2, "got: {:?}", ex.functions);

    let alpha = &ex.functions[0];
    let beta = &ex.functions[1];
    assert_eq!(
        alpha.name, "func_1000",
        "addresses are RVAs, not file offsets"
    );
    assert_eq!(alpha.addr, TEXT_VA as u64);
    assert_eq!(alpha.size, 64);
    assert_eq!(beta.name, "func_1040");
    assert_eq!(beta.addr, 0x1040);
    // beta runs to the end of the section's raw data, file padding included -
    // the same section-end rule the export-table source applies.
    assert_eq!(beta.size, u64::from(raw) - 64);

    // Feature scoping survives the heuristic: alpha's 64 bytes hold 62
    // 3-grams and its own immediate, beta's bytes hold its own, and neither
    // leaks into the other.
    assert_ngrams_scoped(alpha);
    assert_ngrams_scoped(beta);
    assert_eq!(alpha.features.get("cn:2a"), Some(&1));
    assert_eq!(beta.features.get("cn:1f"), Some(&1));
    assert!(!alpha.features.contains_key("cn:1f"));
    assert!(!beta.features.contains_key("cn:2a"));
    assert_eq!(beta.features.get("ng:f30f1e"), Some(&1));
    assert!(!alpha.features.contains_key("ng:f30f1e"));
    // The scoped namespaces: a size bucket and the container marker, no
    // container-only families.
    assert_eq!(alpha.features.get("fn:6"), Some(&1), "log2(64) = 6");
    assert!(!alpha.features.keys().any(|k| k.starts_with("sec:")));
    assert!(!alpha.features.keys().any(|k| k.starts_with("ent:")));
    assert!(!alpha.features.keys().any(|k| k.starts_with("cmt:")));
}

/// A plain x86-64 executable whose executable bytes hold no prologue at all
/// yields an empty list with an explicit heuristic-shaped reason.
#[test]
fn stripped_pe_without_prologues_reports_a_reason() {
    // `push es` is invalid in 64-bit mode, so not even the section-start
    // candidate survives the decode check.
    let text = vec![0x06u8; 24];
    let info = peinfo::parse(Arc::new(stripped_pe(0x8664, &text))).expect("parses");
    assert!(info.functions.is_empty());
    let ex = functions::extract_pe(&info);
    assert_eq!(ex.source, functions::FunctionSource::Heuristic);
    assert!(ex.functions.is_empty());
    let reason = ex.reason.expect("reason present");
    assert!(
        reason.contains("no export table") && reason.contains("heuristic found no candidate"),
        "reason: {reason}"
    );
}

/// A non-x86-64 PE keeps the explicit export-requirement reason: the
/// heuristic reads x86-64 code only, so a stripped arm64 image is never
/// guessed at.
#[test]
fn aarch64_pe_without_exports_keeps_the_export_requirement() {
    let info = peinfo::parse(Arc::new(stripped_pe(0xaa64, &two_functions()))).expect("parses");
    assert_eq!(info.labels.arch, "aarch64");
    assert!(!info.has_exports);
    let ex = functions::extract_pe(&info);
    assert_eq!(ex.source, functions::FunctionSource::SymbolTable);
    assert!(ex.functions.is_empty());
    let reason = ex.reason.expect("reason present");
    assert!(
        reason.contains("no export table") && reason.contains("x86-64"),
        "reason: {reason}"
    );
}

/// End to end through the magic-byte dispatcher: a stripped PE on disk
/// extracts with the heuristic source.
#[test]
fn dispatcher_routes_a_stripped_pe_to_the_heuristic() {
    let path = std::env::temp_dir().join("compid-stripped-pe-heuristic-test.exe");
    std::fs::write(&path, stripped_pe(0x8664, &two_functions())).expect("write temp PE");
    let ex = compid::extract_binary_functions(&path).expect("dispatches to the PE reader");
    std::fs::remove_file(&path).ok();
    assert_eq!(ex.source, functions::FunctionSource::Heuristic);
    assert_eq!(ex.functions.len(), 2);
    assert!(ex.reason.is_none());
}

// ---- Mach-O: no LC_SYMTAB, x86-64 -> the heuristic owns the boundaries ----

/// A stripped x86-64 Mach-O falls back to the prologue heuristic: both
/// prologue-marked functions are found with synthesized virtual-address
/// names and scoped features. The address assertion is load-bearing: a scan
/// keyed on the section's file offset instead of `sect.addr` would have
/// named them `func_1000`/`func_1040`.
#[test]
fn stripped_macho_yields_heuristic_functions() {
    let text = two_functions();
    let info =
        machoinfo::parse(Arc::new(stripped_macho(CPU_TYPE_X86_64, 3, &text))).expect("parses");
    assert_eq!(info.labels.arch, "x86_64");
    assert!(!info.has_symtab);
    assert!(info.functions.is_empty(), "no symtab, no symbols");

    let ex = functions::extract_macho(&info);
    assert_eq!(ex.source, functions::FunctionSource::Heuristic);
    assert!(ex.reason.is_none(), "reason: {:?}", ex.reason);
    assert_eq!(ex.functions.len(), 2, "got: {:?}", ex.functions);

    let alpha = &ex.functions[0];
    let beta = &ex.functions[1];
    assert_eq!(
        alpha.name, "func_100001000",
        "addresses are virtual addresses, not file offsets"
    );
    assert_eq!(alpha.addr, MACHO_TEXT_ADDR);
    assert_eq!(alpha.size, 64);
    assert_eq!(beta.name, "func_100001040");
    assert_eq!(beta.addr, MACHO_TEXT_ADDR + 64);
    assert_eq!(beta.size, 64);

    // Feature scoping: the same strict per-function bytes as the ELF and PE
    // heuristic paths.
    assert_ngrams_scoped(alpha);
    assert_ngrams_scoped(beta);
    assert_eq!(alpha.features.get("cn:2a"), Some(&1));
    assert_eq!(beta.features.get("cn:1f"), Some(&1));
    assert!(!alpha.features.contains_key("cn:1f"));
    assert!(!beta.features.contains_key("cn:2a"));
    assert_eq!(alpha.features.get("fn:6"), Some(&1));
    assert!(!alpha.features.keys().any(|k| k.starts_with("sec:")));
    assert!(!alpha.features.keys().any(|k| k.starts_with("ent:")));
    assert!(!alpha.features.keys().any(|k| k.starts_with("cmt:")));
}

/// A stripped x86-64 Mach-O whose code bytes hold no prologue at all yields
/// an empty list with an explicit heuristic-shaped reason.
#[test]
fn stripped_macho_without_prologues_reports_a_reason() {
    // `push es` is invalid in 64-bit mode, so not even the section-start
    // candidate survives the decode check.
    let text = vec![0x06u8; 24];
    let info =
        machoinfo::parse(Arc::new(stripped_macho(CPU_TYPE_X86_64, 3, &text))).expect("parses");
    assert!(info.functions.is_empty());
    let ex = functions::extract_macho(&info);
    assert_eq!(ex.source, functions::FunctionSource::Heuristic);
    assert!(ex.functions.is_empty());
    let reason = ex.reason.expect("reason present");
    assert!(
        reason.contains("no symbol table") && reason.contains("heuristic found no candidate"),
        "reason: {reason}"
    );
}

/// A stripped arm64 Mach-O keeps the explicit symtab requirement: the
/// heuristic reads x86-64 code only and never pretends otherwise.
#[test]
fn stripped_arm64_macho_keeps_the_symbol_table_requirement() {
    let info = machoinfo::parse(Arc::new(stripped_macho(
        CPU_TYPE_ARM64,
        0,
        &two_functions(),
    )))
    .expect("parses");
    assert_eq!(info.labels.arch, "aarch64");
    assert!(!info.has_symtab);
    let ex = functions::extract_macho(&info);
    assert_eq!(ex.source, functions::FunctionSource::SymbolTable);
    assert!(ex.functions.is_empty());
    let reason = ex.reason.expect("reason present");
    assert!(
        reason.contains("stripped") && reason.contains("x86-64"),
        "reason: {reason}"
    );
}

/// End to end through the magic-byte dispatcher: a stripped Mach-O on disk
/// extracts with the heuristic source.
#[test]
fn dispatcher_routes_a_stripped_macho_to_the_heuristic() {
    let path = std::env::temp_dir().join("compid-stripped-macho-heuristic-test");
    std::fs::write(&path, stripped_macho(CPU_TYPE_X86_64, 3, &two_functions()))
        .expect("write temp Mach-O");
    let ex = compid::extract_binary_functions(&path).expect("dispatches to the Mach-O reader");
    std::fs::remove_file(&path).ok();
    assert_eq!(ex.source, functions::FunctionSource::Heuristic);
    assert_eq!(ex.functions.len(), 2);
    assert!(ex.reason.is_none());
}

// ---- the committed Mach-O fixtures, real bytes ----

/// The heuristic against the committed x86-64 fixture's symtab as ground
/// truth: the image's one symbol (`_main`, an aligned push rbp/mov rbp,rsp
/// prologue) must be proposed by the scan at the same virtual address, and
/// the `__TEXT` metadata sections (`__unwind_info`, `__eh_frame`,
/// `__cstring`) must contribute no candidates - the section filter keeping
/// structured data out of the boundary set.
#[test]
fn macho_x86_64_fixture_heuristic_covers_the_symbol_starts() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/macho_x86_64_sample");
    if !path.exists() {
        eprintln!("skipping: fixture {path:?} not present");
        return;
    }
    let info = machoinfo::load(&path).expect("fixture parses");
    assert!(info.has_symtab);
    assert!(!info.functions.is_empty(), "fixture has symbol functions");

    let heur = functions::heuristic_functions_macho(&info);
    for sym in &info.functions {
        assert!(
            heur.iter()
                .any(|f| f.addr.abs_diff(sym.addr) <= functions::MEASURE_TOLERANCE),
            "heuristic starts {heur:?} must cover symbol {} at {:#x}",
            sym.name,
            sym.addr
        );
    }

    // No candidate inside the metadata sections the filter excludes.
    for f in &heur {
        for meta in [
            "__TEXT,__unwind_info",
            "__TEXT,__eh_frame",
            "__TEXT,__cstring",
        ] {
            let Some(s) = info.sections.iter().find(|s| s.name == meta) else {
                continue;
            };
            let hi = s.addr.saturating_add(s.size);
            assert!(
                !(f.addr >= s.addr && f.addr < hi),
                "candidate {:#x} sits inside {meta}",
                f.addr
            );
        }
    }
}

/// The arm64 fixture's code is opaque to the x86-64 heuristic: an empty
/// candidate set, never guessed arm64 boundaries.
#[test]
fn macho_aarch64_fixture_heuristic_is_empty() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/macho_aarch64_sample");
    if !path.exists() {
        eprintln!("skipping: fixture {path:?} not present");
        return;
    }
    let info = machoinfo::load(&path).expect("fixture parses");
    assert_eq!(info.labels.arch, "aarch64");
    assert!(
        functions::heuristic_functions_macho(&info).is_empty(),
        "arm64 code gets no x86-64 heuristic candidates"
    );
}
