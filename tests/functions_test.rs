//! Function-level extraction tests against synthetic ELFs built byte-by-byte:
//! symbol-table enumeration (linked-executable and relocatable-object
//! layouts), feature scoping to one function's bytes, and the model reader's
//! acceptance of a functions.json payload.

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

/// A stripped ELF (no symtab section) yields an empty list with the reason,
/// and no features are manufactured.
#[test]
fn stripped_elf_reports_a_reason_instead_of_functions() {
    // Reuse the four-section fixture shape: .text plus .comment only.
    let shstrtab: &[u8] = b"\0.text\0.comment\0.shstrtab\0";
    let text = two_functions();
    let comment: &[u8] = b"x\0";
    let text_off = 64u64;
    let comment_off = text_off + text.len() as u64;
    let strtab_off = comment_off + comment.len() as u64;
    let shoff = strtab_off + shstrtab.len() as u64;

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&62u16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&shoff.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&64u16.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&text);
    out.extend_from_slice(comment);
    out.extend_from_slice(shstrtab);
    // null, .text, .comment, .shstrtab
    section_header(&mut out, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    section_header(
        &mut out,
        1,
        1,
        0x6,
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
        0x30,
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

    let info = elfinfo::parse(Arc::new(out)).expect("parses");
    assert!(!info.has_symtab);
    assert!(info.functions.is_empty());
    let ex = functions::extract_elf(&info);
    assert!(ex.functions.is_empty());
    let reason = ex.reason.expect("reason present");
    assert!(
        reason.contains("stripped"),
        "reason mentions stripped: {reason}"
    );
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
