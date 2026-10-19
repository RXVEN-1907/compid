//! Feature extraction tests against a synthetic ELF built byte-by-byte in
//! the test, so the extractor is exercised on every `cargo test` run even
//! when `tests/fixtures/` has no committed binaries yet.

use compid::{elfinfo, features};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Minimum viable ELF64 executable: no program headers, four section
/// headers (null, .text, .comment, .shstrtab). goblin parses this happily.
fn tiny_elf_exec(text: &[u8], comment: &[u8]) -> Vec<u8> {
    let shstrtab: &[u8] = b"\0.text\0.comment\0.shstrtab\0";
    let text_off = 64u64;
    let comment_off = text_off + text.len() as u64;
    let strtab_off = comment_off + comment.len() as u64;
    let shoff = strtab_off + shstrtab.len() as u64;

    let mut out = Vec::with_capacity(shoff as usize + 4 * 64);

    // ELF header.
    out.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&2u16.to_le_bytes()); // e_type = ET_EXEC
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
    out.extend_from_slice(&4u16.to_le_bytes()); // e_shnum
    out.extend_from_slice(&3u16.to_le_bytes()); // e_shstrndx
    assert_eq!(out.len(), 64);

    // Section contents.
    out.extend_from_slice(text);
    out.extend_from_slice(comment);
    out.extend_from_slice(shstrtab);
    assert_eq!(out.len() as u64, shoff);

    // Section headers: null, .text, .comment, .shstrtab.
    section_header(&mut out, 0, 0, 0, 0, 0, 0, 0);
    section_header(
        &mut out,
        1,
        1,
        0x6,
        0x401000,
        text_off,
        text.len() as u64,
        16,
    );
    section_header(
        &mut out,
        7,
        1,
        0x30,
        0,
        comment_off,
        comment.len() as u64,
        1,
    );
    section_header(&mut out, 16, 3, 0, 0, strtab_off, shstrtab.len() as u64, 1);
    out
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
    align: u64,
) {
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&typ.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&addr.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // sh_link
    out.extend_from_slice(&0u32.to_le_bytes()); // sh_info
    out.extend_from_slice(&align.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // sh_entsize
}

/// Two clean 64-byte windows of known x86-64 code plus nop padding.
fn known_text() -> Vec<u8> {
    let mut text: Vec<u8> = Vec::new();
    // Window 1.
    text.push(0x55); // push rbp
    text.extend_from_slice(&[0x48, 0x89, 0xe5]); // mov rbp, rsp
    text.extend_from_slice(&[0x48, 0x83, 0xec, 0x10]); // sub rsp, 0x10
    text.extend_from_slice(&[0xb8, 0x2a, 0x00, 0x00, 0x00]); // mov eax, 0x2a
    text.push(0x5d); // pop rbp
    text.push(0xc3); // ret
    while text.len() < 64 {
        text.push(0x90); // nop
    }
    // Window 2.
    text.extend_from_slice(&[0x31, 0xc0]); // xor eax, eax
    while text.len() < 128 {
        text.push(0x90); // nop
    }
    text
}

fn synth(comment: &[u8]) -> elfinfo::ElfInfo {
    let data = tiny_elf_exec(&known_text(), comment);
    elfinfo::parse(Arc::new(data)).expect("synthetic ELF parses")
}

#[test]
fn parses_labels_off_the_synthetic_binary() {
    let info = synth(b"GCC: (compid synthetic) 9.4.0\0");
    let l = &info.labels;
    assert_eq!(l.arch, "x86-64");
    assert_eq!(l.pie, "no");
    assert_eq!(l.static_linking, "static");
    assert_eq!(l.compiler_family, "gcc");
    assert_eq!(l.compiler_version, "9.4.0");
    assert_eq!(l.os, "unknown");
    assert_eq!(l.abi, "unknown");
    assert_eq!(l.lto, "no");
    assert_eq!(l.opt_level, "unknown");
}

#[test]
fn counts_mnemonics_registers_immediates_exactly() {
    let info = synth(b"GCC: (compid synthetic) 9.4.0\0");
    let ef = features::extract(&info).expect("extract succeeds");
    let f = &ef.features;

    assert_eq!(
        f.get("meta:windows"),
        Some(&2),
        "both windows decode cleanly"
    );
    assert_eq!(f.get("mn:push"), Some(&1));
    assert_eq!(f.get("mn:mov"), Some(&2));
    assert_eq!(f.get("mn:sub"), Some(&1));
    assert_eq!(f.get("mn:pop"), Some(&1));
    assert_eq!(f.get("mn:ret"), Some(&1));
    assert_eq!(f.get("mn:xor"), Some(&1));
    assert_eq!(f.get("mn:nop"), Some(&111), "49 + 62 nop bytes");

    // push rbp, mov rbp,rsp (x2), pop rbp -> rbp three times;
    // mov rbp,rsp, sub rsp,0x10 -> rsp twice;
    // mov eax,0x2a, xor eax,eax (x2) -> eax three times.
    assert_eq!(f.get("rg:rbp"), Some(&3));
    assert_eq!(f.get("rg:rsp"), Some(&2));
    assert_eq!(f.get("rg:eax"), Some(&3));

    assert_eq!(f.get("cn:10"), Some(&1), "sub rsp, 0x10");
    assert_eq!(f.get("cn:2a"), Some(&1), "mov eax, 0x2a");
}

#[test]
fn counts_byte_ngrams_over_the_raw_text() {
    let info = synth(b"GCC: (compid synthetic) 9.4.0\0");
    let ef = features::extract(&info).expect("extract succeeds");
    let f = &ef.features;

    // 128 bytes of text -> 126 3-grams in total.
    let total: u64 = f
        .iter()
        .filter(|(k, _)| k.starts_with("ng:"))
        .map(|(_, v)| v)
        .sum();
    assert_eq!(total, 126);

    // push rbp; mov ...
    assert_eq!(f.get("ng:554889"), Some(&1));
    // 49-nop run then 62-nop run: (49 - 2) + (62 - 2)
    assert_eq!(f.get("ng:909090"), Some(&107));
}

#[test]
fn section_features_reflect_geometry_and_entropy() {
    let info = synth(b"GCC: (compid synthetic) 9.4.0\0");
    let ef = features::extract(&info).expect("extract succeeds");
    let f = &ef.features;

    assert_eq!(f.get("sec:.text"), Some(&7), "log2(128) = 7");
    // The comment is 30 bytes: log2(30) floors to 4.
    assert_eq!(f.get("sec:.comment"), Some(&4));

    let ent_text = f.get("ent:.text").copied().expect("entropy bucket present");
    assert!(ent_text <= 8);
    // Mostly 0x90 bytes: low-ish but nonzero entropy.
    assert!((1..=5).contains(&ent_text), "ent:.text = {ent_text}");

    let ent_comment = f
        .get("ent:.comment")
        .copied()
        .expect("comment entropy present");
    assert!(ent_comment <= 8);
}

#[test]
fn comment_tokens_become_cmt_features() {
    let info = synth(b"GCC: (compid synthetic) 9.4.0\0");
    let ef = features::extract(&info).expect("extract succeeds");
    let f = &ef.features;
    assert_eq!(f.get("cmt:gcc:"), Some(&1));
    assert_eq!(f.get("cmt:9.4.0"), Some(&1));
    assert_eq!(f.get("cmt:(compid"), Some(&1));
}

#[test]
fn unclean_window_contributes_nothing() {
    // 0x06 is an invalid opcode in 64-bit mode, so this window never
    // disassembles cleanly.
    let mut text = vec![0x06u8; 64];
    text.extend(vec![0x90u8; 64]);
    let data = tiny_elf_exec(&text, b"x\0");
    let info = elfinfo::parse(Arc::new(data)).expect("parses");
    let ef = features::extract(&info).expect("extract succeeds");
    assert_eq!(
        ef.features.get("meta:windows"),
        Some(&1),
        "only the nop window"
    );
    assert_eq!(ef.features.get("mn:nop"), Some(&64));
    assert!(!ef.features.contains_key("mn:mov"));
}

#[test]
fn empty_text_is_a_clean_error_not_a_panic() {
    // .text section declared but zero-size -> extraction errors with a
    // helpful message.
    let data = tiny_elf_exec(&[], b"x\0");
    let info = elfinfo::parse(Arc::new(data)).expect("parses");
    let err = features::extract(&info).expect_err("empty text must error");
    let msg = format!("{err:#}");
    assert!(msg.contains(".text"), "message mentions .text: {msg}");
}

#[test]
fn cosine_similarity_known_values() {
    let mut a = BTreeMap::new();
    a.insert("mn:push".to_string(), 2u64);
    a.insert("mn:pop".to_string(), 1u64);
    let mut parallel = BTreeMap::new();
    parallel.insert("mn:push".to_string(), 4u64);
    parallel.insert("mn:pop".to_string(), 2u64);
    assert!(
        (features::cosine(&a, &parallel) - 1.0).abs() < 1e-12,
        "parallel vectors = 1.0"
    );

    let mut b = BTreeMap::new();
    b.insert("mn:push".to_string(), 1u64);
    // dot = 2, |a| = sqrt(5), |b| = 1 -> 2/sqrt(5)
    let want = 2.0 / 5.0f64.sqrt();
    assert!((features::cosine(&a, &b) - want).abs() < 1e-12);

    let mut disjoint = BTreeMap::new();
    disjoint.insert("mn:ret".to_string(), 7u64);
    assert_eq!(features::cosine(&a, &disjoint), 0.0, "disjoint = 0.0");

    let empty = BTreeMap::new();
    assert_eq!(features::cosine(&a, &empty), 0.0, "empty vector = 0.0");
}

#[test]
fn two_builds_of_the_same_code_have_different_fingerprints_only_by_padding() {
    // Two synth binaries whose code differs only in one immediate: cosine
    // close to 1 but not exactly 1.
    let mut text2 = known_text();
    text2[7] = 0x20; // sub rsp, 0x20 instead of 0x10
    let info_a = elfinfo::parse(Arc::new(tiny_elf_exec(&known_text(), b"x\0"))).unwrap();
    let info_b = elfinfo::parse(Arc::new(tiny_elf_exec(&text2, b"x\0"))).unwrap();
    let fa = features::extract(&info_a).unwrap();
    let fb = features::extract(&info_b).unwrap();
    let sim = features::cosine(&fa.features, &fb.features);
    assert!(sim > 0.99, "one immediate apart: {sim}");
    assert!(sim < 1.0, "not identical: {sim}");
}
