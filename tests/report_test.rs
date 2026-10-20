//! End-to-end color behavior over the real binary: `--color` on classify,
//! explain, and functions; the `report` screen in both modes; and the
//! NO_COLOR / tty auto-detection contract.
//!
//! The test harness captures stdout through a pipe, so `auto` resolves to
//! plain here - which is exactly the "auto off in tests" case.

use std::path::{Path, PathBuf};
use std::process::Command;

const LAVENDER: &str = "\x1b[38;2;167;139;250m";
const RESET: &str = "\x1b[0m";

/// One committed gcc* fixture, or None when the fixtures are absent.
fn any_fixture() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("gcc") && n.contains("_O") && n.contains("_x86_64_"))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

fn compid() -> Command {
    Command::new(env!("CARGO_BIN_EXE_compid"))
}

fn run_compid(args: &[&str], no_color: Option<&str>) -> std::process::Output {
    let fixture = any_fixture().expect("committed fixtures are present");
    let model = Path::new(env!("CARGO_MANIFEST_DIR")).join("model/model.json");
    let mut cmd = compid();
    cmd.args(args)
        .arg(&fixture)
        .arg("--model")
        .arg(&model)
        .current_dir(env!("CARGO_MANIFEST_DIR"));
    if let Some(value) = no_color {
        cmd.env("NO_COLOR", value);
    }
    cmd.output().expect("compid runs")
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn auto_stays_plain_when_stdout_is_not_a_terminal() {
    let out = run_compid(&["classify"], None);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        !text.contains('\x1b'),
        "auto mode emits no codes through a pipe"
    );
    assert!(text.contains("compiler_family"), "the table still renders");
}

#[test]
fn force_on_emits_codes() {
    let out = run_compid(&["classify", "--color", "always"], None);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains(LAVENDER),
        "forced color paints the bars lavender"
    );
    assert!(text.contains(RESET));
}

#[test]
fn no_color_suppresses_auto() {
    let out = run_compid(&["classify"], Some("1"));
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(!text.contains('\x1b'), "NO_COLOR=1 keeps auto mode plain");
    assert!(text.contains("compiler_family"), "the table still renders");
}

#[test]
fn color_never_is_plain_for_every_table_command() {
    for args in [
        vec!["classify", "--color", "never"],
        vec!["explain", "--color", "never"],
        vec!["functions", "--color", "never"],
        vec!["report", "--color", "never"],
    ] {
        let out = run_compid(&args, None);
        assert!(out.status.success(), "{args:?} exits successfully");
        let text = stdout(&out);
        assert!(
            !text.contains('\x1b'),
            "{args:?} with --color never emits no codes"
        );
    }
}

#[test]
fn json_output_stays_plain_even_when_color_is_forced() {
    for args in [
        vec!["classify", "--json", "--color", "always"],
        vec!["report", "--json", "--color", "always"],
    ] {
        let out = run_compid(&args, None);
        assert!(out.status.success(), "{args:?} exits successfully");
        let text = stdout(&out);
        assert!(!text.contains('\x1b'), "{args:?} JSON stays plain");
        let value: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{args:?} emits valid JSON: {e}"));
        assert!(value.get("fields").is_some() || value.get("classification").is_some());
    }
}

#[test]
fn report_screen_renders_both_modes() {
    let plain = run_compid(&["report", "--color", "never"], None);
    assert!(plain.status.success());
    let text = stdout(&plain);
    assert!(!text.contains('\x1b'));
    // The three sections of the screen, in order.
    let table = text.find("compiler_family").expect("classification table");
    let why = text.find("why - top contributors").expect("why section");
    let prov = text.find("provenance").expect("provenance block");
    assert!(table < why && why < prov);
    assert!(
        text.contains("manifest sha256:"),
        "the provenance block lists the manifest digest"
    );

    let colored = run_compid(&["report", "--color", "always"], None);
    assert!(colored.status.success());
    let text = stdout(&colored);
    assert!(text.contains(LAVENDER), "colored report carries the accent");
    // The visible text survives the codes: strip them and the same three
    // sections must be there.
    let visible = strip_sgr(&text);
    assert!(visible.contains("why - top contributors"));
    assert!(visible.contains("provenance"));
}

#[test]
fn report_json_combines_classification_and_provenance() {
    let out = run_compid(&["report", "--json"], None);
    assert!(out.status.success());
    let text = stdout(&out);
    let value: serde_json::Value =
        serde_json::from_str(&text).expect("report --json emits valid JSON");
    assert!(value["classification"]["fields"].is_array());
    assert!(value["provenance"]["manifest_rows"].is_number());
    assert!(value["binary"].is_string());
}

#[test]
fn functions_colored_keeps_its_columns_aligned() {
    // The prediction cells are colored after padding, so stripping the
    // codes must reproduce the plain rendering exactly.
    let plain = run_compid(&["functions", "--color", "never"], None);
    let colored = run_compid(&["functions", "--color", "always"], None);
    assert!(plain.status.success() && colored.status.success());
    let plain = strip_sgr(&stdout(&plain));
    let colored = strip_sgr(&stdout(&colored));
    assert_eq!(plain, colored, "color never shifts the functions table");
}

/// Remove SGR sequences so colored and plain renderings compare by visible
/// text alone.
fn strip_sgr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for e in chars.by_ref() {
                if e == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
