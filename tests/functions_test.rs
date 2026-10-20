//! Function-level extraction tests against synthetic ELFs built byte-by-byte:
//! symbol-table enumeration (linked-executable and relocatable-object
//! layouts), the stripped-binary prologue heuristic (synthetic .text blobs
//! and a real fixture stripped at test time), feature scoping to one
//! function's bytes, and the model reader's acceptance of a functions.json
//! payload.

use compid::{elfinfo, functions, model};
use std::sync::Arc;

/// Two known 64-byte functions back to back. `alpha` opens with the
/// push/rbp/mov prologue and a 0x2a immediate; `beta` opens with
/// `xor eax, eax`; both end in nop padding, so the n-grams that straddle the
/// boundary (90 90 31 and 90 31 c0) belong to neither function.
fn two_functions() -> Vec<u8> {
    let mut text: Vec<u8> = Vec::new();
    // alpha
    text.extend_from_slice(&[
        0x55, // push rbp
        0x48, 0x89, 0xe5, // mov rbp, rsp
        0xb8, 0x2a, 0x00, 0x00, 0x00, // mov eax, 0x2a
        0x5d, // pop rbp
        0xc3, // ret
    ]);
    while text.len() < 64 {
        text.push(0x90); // nop
    }
    // beta
    text.extend_from_slice(&[0x31, 0xc0]); // xor eax, eax
    while text.len() < 128 {
        text.push(0x90); // nop
    }
    text
}

/// One 24-byte ELF64 symbol table entry, little-endian.
#[allow(clippy::too_many_arguments)]
fn sym_entry(
    out: &mut Vec<u8>,
    st_name: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
    st_value: u64,
    st_size: u64,
) {
    out.extend_from_slice(&st_name.to_le_bytes());
    out.push(st_info);
    out.push(st_other);
    out.extend_from_slice(&st_shndx.to_le_bytes());
    out.extend_from_slice(&st_value.to_le_bytes());
    out.extend_from_slice(&st_size.to_le_bytes());
}

/// One 64-byte ELF64 section header, little-endian.
#[allow(clippy::too_many_arguments)]
fn section_header(
    out: &mut Vec<u8>,
    name: u32,
    typ: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    align: u64,
    entsize: u64,
) {
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&typ.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&addr.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&link.to_le_bytes());
    out.extend_from_slice(&info.to_le_bytes());
    out.extend_from_slice(&align.to_le_bytes());
    out.extend_from_slice(&entsize.to_le_bytes());
}

/// Minimal ELF with five sections (null, .text, .symtab, .strtab,
/// .shstrtab) and a five-entry symbol table: the two functions under test
/// plus an STT_OBJECT symbol and a zero-size function that must both be
/// filtered out. `e_type` selects the linked-executable layout (ET_EXEC,
/// symbol values are virtual addresses) or the relocatable-object layout
/// (ET_REL, symbol values are offsets within .text).
fn elf_with_functions(e_type: u16) -> Vec<u8> {
    let shstrtab: &[u8] = b"\0.text\0.symtab\0.strtab\0.shstrtab\0";
    let strtab: &[u8] = b"\0alpha\0beta\0an_object\0tiny\0";

    let text = two_functions();
    let text_addr: u64 = if e_type == 1 { 0 } else { 0x401000 };
    let alpha_value: u64 = if e_type == 1 { 0 } else { 0x401000 };
    let beta_value: u64 = if e_type == 1 { 64 } else { 0x401040 };

    let text_off = 64u64;
    let symtab_off = text_off + text.len() as u64;
    let strtab_off = symtab_off + 5 * 24;
    let shstrtab_off = strtab_off + strtab.len() as u64;
    let shoff = (shstrtab_off + shstrtab.len() as u64).div_ceil(8) * 8;

    let mut out = Vec::with_capacity(shoff as usize + 5 * 64);

    // ELF header.
    out.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&e_type.to_le_bytes());
    out.extend_from_slice(&62u16.to_le_bytes()); // e_machine = EM_X86_64
    out.extend_from_slice(&1u32.to_le_bytes()); // e_version
    out.extend_from_slice(&0u64.to_le_bytes()); // e_entry
    out.extend_from_slice(&0u64.to_le_bytes()); // e_phoff
    out.extend_from_slice(&shoff.to_le_bytes()); // e_shoff
    out.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    out.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
    out.extend_from_slice(&0u16.to_le_bytes()); // e_phentsize
    out.extend_from_slice(&0u16.to_le_bytes()); // e_phnum
    out.extend_from_slice(&64u16.to_le_bytes()); // e_shentsize
    out.extend_from_slice(&5u16.to_le_bytes()); // e_shnum
    out.extend_from_slice(&4u16.to_le_bytes()); // e_shstrndx
    assert_eq!(out.len(), 64);

    // Section contents.
    out.extend_from_slice(&text);
    {
        let mut syms = Vec::new();
        // 0: the null symbol.
        sym_entry(&mut syms, 0, 0, 0, 0, 0, 0);
        // 1: alpha, a local function spanning the first half of .text
        //    (bind LOCAL << 4 | type FUNC).
        sym_entry(&mut syms, 1, 0x02, 0, 1, alpha_value, 64);
        // 2: beta, a global function spanning the second half.
        sym_entry(&mut syms, 7, 0x12, 0, 1, beta_value, 64);
        // 3: an object symbol that must never reach the function list.
        sym_entry(&mut syms, 12, 0x11, 0, 1, text_addr, 8);
        // 4: a zero-size function that must never reach the function list.
        sym_entry(&mut syms, 22, 0x12, 0, 1, beta_value, 0);
        out.extend_from_slice(&syms);
    }
    out.extend_from_slice(strtab);
    out.extend_from_slice(shstrtab);
    while (out.len() as u64) < shoff {
        out.push(0);
    }
    assert_eq!(out.len() as u64, shoff);

    // Section headers: null, .text, .symtab, .strtab, .shstrtab.
    section_header(&mut out, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    section_header(
        &mut out,
        1,
        1,
        0x6, // ALLOC | EXECINSTR
        text_addr,
        text_off,
        text.len() as u64,
        0,
        0,
        16,
        0,
    );
    section_header(
        &mut out,
        7, // ".symtab"
        2, // SHT_SYMTAB
        0,
        0,
        symtab_off,
        5 * 24,
        3, // sh_link -> .strtab
        2, // sh_info -> first non-local index
        8,
        24,
    );
    section_header(
        &mut out,
        15,
        3,
        0,
        0,
        strtab_off,
        strtab.len() as u64,
        0,
        0,
        1,
        0,
    );
    section_header(
        &mut out,
        23,
        3,
        0,
        0,
        shstrtab_off,
        shstrtab.len() as u64,
        0,
        0,
        1,
        0,
    );
    out
}

fn parse_functions(e_type: u16) -> elfinfo::ElfInfo {
    let data = elf_with_functions(e_type);
    elfinfo::parse(Arc::new(data)).expect("synthetic ELF with symbols parses")
}

/// alpha and beta with correct raw names, addresses, sizes; the object
/// symbol and the zero-size function are filtered; deduplication keeps both
/// distinct ranges.
#[test]
fn enumerates_the_two_known_functions_from_a_linked_elf() {
    let info = parse_functions(2); // ET_EXEC
    let fs = &info.functions;
    assert!(info.has_symtab);
    assert_eq!(fs.len(), 2, "got: {fs:?}");
    assert_eq!(fs[0].name, "alpha");
    assert_eq!(fs[0].addr, 0x401000);
    assert_eq!(fs[0].size, 64);
    assert_eq!(fs[1].name, "beta");
    assert_eq!(fs[1].addr, 0x401040);
    assert_eq!(fs[1].size, 64);
}

#[test]
fn enumerates_functions_from_a_relocatable_object_layout() {
    let info = parse_functions(1); // ET_REL: st_value is a section offset
    let fs = &info.functions;
    assert_eq!(fs.len(), 2, "got: {fs:?}");
    assert_eq!(fs[0].name, "alpha");
    assert_eq!(fs[0].addr, 0);
    assert_eq!(fs[0].size, 64);
    assert_eq!(fs[1].name, "beta");
    assert_eq!(fs[1].addr, 64);
    assert_eq!(fs[1].size, 64);
}

/// A function's feature vector contains ONLY its own bytes: each half keeps
/// its distinctive n-gram and immediate, neither sees the other half's, and
/// the n-grams straddling the boundary appear in neither.
#[test]
fn function_features_scope_to_the_function_bytes_only() {
    let info = parse_functions(2);
    let ex = functions::extract_elf(&info);
    assert!(ex.reason.is_none());
    assert_eq!(ex.functions.len(), 2);
    let alpha = &ex.functions[0];
    let beta = &ex.functions[1];

    assert_eq!(alpha.name, "alpha");
    assert_eq!(beta.name, "beta");

    // Each function's n-gram budget matches its byte count exactly: 62
    // 3-grams out of 64 bytes, so no outside bytes leaked in.
    for f in [alpha, beta] {
        let total: u64 = f
            .features
            .iter()
            .filter(|(k, _)| k.starts_with("ng:"))
            .map(|(_, v)| v)
            .sum();
        assert_eq!(total, 62, "{}: n-gram total = {total}", f.name);
    }

    // alpha's prologue and immediate stay in alpha.
    assert_eq!(alpha.features.get("ng:554889"), Some(&1));
    assert_eq!(alpha.features.get("cn:2a"), Some(&1));
    assert_eq!(alpha.features.get("mn:push"), Some(&1));
    // beta's opening xor stays in beta.
    assert_eq!(beta.features.get("ng:31c090"), Some(&1));
    assert_eq!(beta.features.get("mn:xor"), Some(&1));
    // Cross-contamination checks, in both directions.
    assert!(
        !alpha.features.contains_key("ng:31c090"),
        "beta's xor leaked into alpha"
    );
    assert!(
        !beta.features.contains_key("ng:554889"),
        "alpha's prologue leaked into beta"
    );
    assert!(
        !beta.features.contains_key("cn:2a"),
        "alpha's immediate leaked into beta"
    );
    // Boundary n-grams (nop, nop, xor) and (nop, xor, xor-operand) belong to
    // neither function.
    assert!(!alpha.features.contains_key("ng:909031"));
    assert!(!alpha.features.contains_key("ng:9031c0"));
    assert!(!beta.features.contains_key("ng:909031"));
    assert!(!beta.features.contains_key("ng:9031c0"));

    // The scoped namespaces: size bucket and container marker, no geometry,
    // entropy, or comment residue.
    for f in [alpha, beta] {
        assert_eq!(f.features.get("fn:6"), Some(&1), "{}: log2(64) = 6", f.name);
        assert_eq!(
            f.features.get("meta:windows"),
            Some(&1),
            "{}: one clean window",
            f.name
        );
        assert!(!f.features.keys().any(|k| k.starts_with("sec:")));
        assert!(!f.features.keys().any(|k| k.starts_with("ent:")));
        assert!(!f.features.keys().any(|k| k.starts_with("cmt:")));
    }
}

/// A minimal stripped ELF (no symtab): ELF header, a `.text` section
/// mapped at 0x401000, a `.comment` section, and the section-name string
/// table. `machine` picks the architecture (62 = EM_X86_64, 183 =
/// EM_AARCH64).
fn stripped_elf(text: &[u8], machine: u16) -> Vec<u8> {
    let shstrtab: &[u8] = b"\0.text\0.comment\0.shstrtab\0";
    let comment: &[u8] = b"x\0";
    let text_off = 64u64;
    let comment_off = text_off + text.len() as u64;
    let strtab_off = comment_off + comment.len() as u64;
    let shoff = strtab_off + shstrtab.len() as u64;

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    out.extend_from_slice(&machine.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes()); // e_version
    out.extend_from_slice(&0u64.to_le_bytes()); // e_entry
    out.extend_from_slice(&0u64.to_le_bytes()); // e_phoff
    out.extend_from_slice(&shoff.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    out.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
    out.extend_from_slice(&0u16.to_le_bytes()); // e_phentsize
    out.extend_from_slice(&0u16.to_le_bytes()); // e_phnum
    out.extend_from_slice(&64u16.to_le_bytes()); // e_shentsize
    out.extend_from_slice(&4u16.to_le_bytes()); // e_shnum
    out.extend_from_slice(&3u16.to_le_bytes()); // e_shstrndx
    assert_eq!(out.len(), 64);
    out.extend_from_slice(text);
    out.extend_from_slice(comment);
    out.extend_from_slice(shstrtab);
    // null, .text, .comment, .shstrtab
    section_header(&mut out, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    section_header(
        &mut out,
        1,
        1,   // SHT_PROGBITS
        0x6, // ALLOC | EXECINSTR
        0x401000,
        text_off,
        text.len() as u64,
        0,
        0,
        16,
        0,
    );
    section_header(
        &mut out,
        7,
        1,
        0x30, // ALLOC | WRITE
        0,
        comment_off,
        comment.len() as u64,
        0,
        0,
        1,
        0,
    );
    section_header(
        &mut out,
        16,
        3,
        0,
        0,
        strtab_off,
        shstrtab.len() as u64,
        0,
        0,
        1,
        0,
    );
    out
}

/// A stripped x86-64 ELF falls back to the prologue heuristic: both
/// prologue-marked functions are found with synthesized names, correct
/// boundaries, and strictly scoped features.
#[test]
fn stripped_elf_yields_heuristic_functions() {
    let mut text: Vec<u8> = Vec::new();
    // alpha @0x401000 (16-aligned): push rbp; mov rbp,rsp; mov eax,0x2a; ...
    text.extend_from_slice(&[
        0x55, 0x48, 0x89, 0xe5, 0xb8, 0x2a, 0x00, 0x00, 0x00, 0x5d, 0xc3,
    ]);
    while text.len() < 64 {
        text.push(0x90);
    }
    // beta @0x401040 (16-aligned): endbr64; push rbp; mov rbp,rsp; mov eax,0x1f; ...
    text.extend_from_slice(&[
        0xf3, 0x0f, 0x1e, 0xfa, 0x55, 0x48, 0x89, 0xe5, 0xb8, 0x1f, 0x00, 0x00, 0x00, 0x5d, 0xc3,
    ]);
    while text.len() < 128 {
        text.push(0x90);
    }

    let info = elfinfo::parse(Arc::new(stripped_elf(&text, 62))).expect("parses");
    assert!(!info.has_symtab);
    assert!(info.functions.is_empty(), "no symbol table, no symbols");
    let ex = functions::extract_elf(&info);
    assert_eq!(ex.source, functions::FunctionSource::Heuristic);
    assert!(ex.reason.is_none(), "reason: {:?}", ex.reason);
    assert_eq!(ex.functions.len(), 2, "got: {:?}", ex.functions);

    let alpha = &ex.functions[0];
    let beta = &ex.functions[1];
    assert_eq!(alpha.name, "func_401000");
    assert_eq!(alpha.addr, 0x401000);
    assert_eq!(alpha.size, 64);
    assert_eq!(beta.name, "func_401040");
    assert_eq!(beta.addr, 0x401040);
    assert_eq!(beta.size, 64);

    // Feature scoping survives the heuristic: 62 3-grams per 64-byte
    // function, no boundary straddling, no cross-leak.
    for f in [alpha, beta] {
        let total: u64 = f
            .features
            .iter()
            .filter(|(k, _)| k.starts_with("ng:"))
            .map(|(_, v)| v)
            .sum();
        assert_eq!(total, 62, "{}: n-gram total = {total}", f.name);
    }
    assert_eq!(alpha.features.get("cn:2a"), Some(&1));
    assert_eq!(beta.features.get("cn:1f"), Some(&1));
    assert!(!alpha.features.contains_key("cn:1f"));
    assert!(!beta.features.contains_key("cn:2a"));
    // beta's CET prologue is beta's own n-gram, never alpha's.
    assert_eq!(beta.features.get("ng:f30f1e"), Some(&1));
    assert!(!alpha.features.contains_key("ng:f30f1e"));
}

/// A stripped ELF whose executable bytes hold no prologue at all yields an
/// empty list with an explicit heuristic-shaped reason.
#[test]
fn stripped_elf_without_prologues_reports_a_reason() {
    // `push es` is invalid in 64-bit mode, so not even the section-start
    // candidate survives the decode check.
    let text = vec![0x06; 24];
    let info = elfinfo::parse(Arc::new(stripped_elf(&text, 62))).expect("parses");
    let ex = functions::extract_elf(&info);
    assert_eq!(ex.source, functions::FunctionSource::Heuristic);
    assert!(ex.functions.is_empty());
    let reason = ex.reason.expect("reason present");
    assert!(
        reason.contains("heuristic found no candidate"),
        "reason: {reason}"
    );
}

/// A stripped non-x86-64 ELF keeps the "needs an unstripped ELF" reason -
/// boundaries are never guessed for architectures the scanner cannot read.
#[test]
fn stripped_non_x86_64_elf_keeps_the_symbol_table_requirement() {
    let info = elfinfo::parse(Arc::new(stripped_elf(&two_functions(), 183))).expect("parses"); // EM_AARCH64
    assert_eq!(info.labels.arch, "aarch64");
    let ex = functions::extract_elf(&info);
    assert_eq!(ex.source, functions::FunctionSource::SymbolTable);
    assert!(ex.functions.is_empty());
    let reason = ex.reason.expect("reason present");
    assert!(
        reason.contains("stripped") && reason.contains("unstripped ELF"),
        "reason: {reason}"
    );
}

/// The prologue heuristic against a real binary: strip a copy of the
/// committed unstripped fixture at test time and compare the heuristic's
/// starts with the original's symbol-table starts. Skipped when `strip` is
/// unavailable or the fixture is absent.
// Linux-only: it shells out to binutils `strip` on an ELF fixture, which
// Apple's strip cannot parse and Windows has no ELF-capable strip for.
#[cfg(target_os = "linux")]
#[test]
fn stripped_fixture_heuristic_matches_symbol_ground_truth() {
    use std::path::PathBuf;

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let unstripped_path = dir.join("gcc13_O2_x86_64_pie_dynamic");
    if !unstripped_path.exists() {
        eprintln!("skipping: fixture {unstripped_path:?} not present");
        return;
    }
    if std::process::Command::new("strip")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: strip tool not available");
        return;
    }

    let tmp = std::env::temp_dir().join(format!(
        "compid_stripped_fixture_{}_{}.elf",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::copy(&unstripped_path, &tmp).expect("copy fixture");
    let stripped_ok = std::process::Command::new("strip")
        .arg(&tmp)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(stripped_ok, "strip ran");

        let stripped_info = elfinfo::load(&tmp).expect("stripped copy parses");
        assert!(!stripped_info.has_symtab);
        let heur = functions::heuristic_functions(&stripped_info);
        assert!(
            !heur.is_empty(),
            "heuristic found functions in a real binary"
        );
        for f in &heur {
            assert!(f.name.starts_with("func_"), "synthesized name: {}", f.name);
            assert!(f.size >= 16, "minimum size holds: {}", f.name);
        }
        for pair in heur.windows(2) {
            assert!(pair[0].addr < pair[1].addr, "sorted by address");
        }

        // Ground truth: the unstripped sibling's symbol starts, ±8 bytes.
        let truth_info = elfinfo::load(&unstripped_path).expect("unstripped fixture parses");
        let tol = functions::MEASURE_TOLERANCE;
        let matched_truth = truth_info
            .function_starts
            .iter()
            .filter(|t| heur.iter().any(|h| h.addr.abs_diff(**t) <= tol))
            .count();
        let matched_heur = heur
            .iter()
            .filter(|h| {
                truth_info
                    .function_starts
                    .iter()
                    .any(|t| h.addr.abs_diff(*t) <= tol)
            })
            .count();
        let recall = matched_truth as f64 / truth_info.function_starts.len() as f64;
        let precision = matched_heur as f64 / heur.len() as f64;
        eprintln!(
            "stripped fixture: {} heuristic starts vs {} symbol starts, \
             precision {precision:.4}, recall {recall:.4}",
            heur.len(),
            truth_info.function_starts.len()
        );
        assert!(
            precision >= 0.9,
            "precision {precision} below 0.9 on the stripped fixture"
        );
        assert!(
            recall >= 0.6,
            "recall {recall} below 0.6 on the stripped fixture"
        );

        // And the extraction reports the heuristic as its source.
        let ex = functions::extract_elf(&stripped_info);
        assert_eq!(ex.source, functions::FunctionSource::Heuristic);
        assert!(ex.reason.is_none());
    }));
    let _ = std::fs::remove_file(&tmp);
    result.expect("stripped fixture assertions hold");
}

/// A functions.json payload - the same compid-model/v1 shape with `fn:` and
/// `meta:windows` features and only the two per-function fields - loads
/// through the unchanged reader.
#[test]
fn model_reader_accepts_a_functions_model() {
    let json = r#"{
        "format": "compid-model/v1",
        "feature_version": "1",
        "created": "2026-10-20",
        "features": ["fn:6", "meta:windows", "ng:554889", "mn:push"],
        "fields": {
            "compiler_family": {
                "classes": ["gcc", "clang"],
                "intercept": [0.1, -0.1],
                "coef": [[0.9, 0.1, 0.5, 0.4], [-0.8, -0.1, -0.5, -0.4]]
            },
            "opt_level": {
                "classes": ["O0", "O2"],
                "intercept": [0.0, 0.0],
                "coef": [[0.2, 0.1, -0.3, -0.2], [-0.2, -0.1, 0.3, 0.2]]
            }
        }
    }"#;
    let m = model::Model::parse(json).expect("functions.json shape parses");
    assert_eq!(m.fields.len(), 2, "compiler_family and opt_level only");

    let mut feats = std::collections::BTreeMap::new();
    feats.insert("mn:push".to_string(), 1u64);
    feats.insert("fn:6".to_string(), 1u64);
    let pred = m.predict(&feats);
    let family = pred
        .fields
        .iter()
        .find(|f| f.field == "compiler_family")
        .unwrap();
    assert_eq!(family.predicted, "gcc");
}
