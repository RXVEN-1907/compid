//! The hidden `completions` subcommand emits a shell completion script for
//! every shell it advertises, and nothing else leaks through as a shell
//! name.

use std::process::Command;

const SHELLS: [&str; 4] = ["bash", "zsh", "fish", "powershell"];

fn completions_script(shell: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_compid"))
        .args(["completions", shell])
        .output()
        .unwrap_or_else(|e| panic!("compid completions {shell} runs: {e}"))
}

#[test]
fn bash_completions_name_the_binary() {
    let out = completions_script("bash");
    assert!(out.status.success(), "completions bash exits successfully");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("compid"),
        "bash completion script mentions compid"
    );
}

#[test]
fn every_supported_shell_emits_a_named_script() {
    for shell in SHELLS {
        let out = completions_script(shell);
        assert!(
            out.status.success(),
            "completions {shell} exits successfully"
        );
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            !text.trim().is_empty(),
            "completions {shell} prints a non-empty script"
        );
        assert!(
            text.contains("compid"),
            "completions {shell} script names compid"
        );
    }
}

#[test]
fn unknown_shell_is_a_usage_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_compid"))
        .args(["completions", "tcsh"])
        .output()
        .expect("compid runs");
    assert_eq!(
        out.status.code(),
        Some(2),
        "an unsupported shell is rejected as a usage error"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("tcsh"),
        "the usage error names the rejected shell"
    );
}
