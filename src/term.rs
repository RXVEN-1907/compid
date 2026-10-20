//! Minimal terminal color engine: three deliberate colors plus bold and
//! dim, no external crates.
//!
//! The palette follows the project's design language - lavender accents on
//! a dark-first terminal, green for the winning class, amber for warnings,
//! everything else typography rather than decoration. Plain mode (color
//! off) returns every input unchanged, so the historical byte-for-byte
//! output is preserved exactly; color mode wraps strings in ANSI codes that
//! are always appended *after* any width formatting, so column alignment
//! never shifts.
//!
//! `NO_COLOR` is honored per the no-color.org convention: when the variable
//! is present and non-empty, `Auto` mode never emits codes. An explicit
//! `--color always` is a direct user command and overrides it, which is the
//! convention the standard itself describes.

use std::io::IsTerminal;

/// Lavender accent: bars, feature names, secondary wins.
pub const LAVENDER: &str = "\x1b[38;2;167;139;250m";
/// Green: the winning class.
pub const GREEN: &str = "\x1b[38;2;74;222;128m";
/// Amber: the low-confidence marker and boundary cautions.
pub const AMBER: &str = "\x1b[38;2;217;180;80m";
/// Dim: metadata lines and probabilities.
pub const DIM: &str = "\x1b[2m";
/// Bold: headers.
pub const BOLD: &str = "\x1b[1m";
/// Reset back to the terminal defaults.
pub const RESET: &str = "\x1b[0m";

/// When to emit ANSI color codes, as the `--color` flag spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum ColorChoice {
    /// Color only when stdout is a terminal and NO_COLOR is unset.
    #[default]
    Auto,
    /// Always color, even when piped; an explicit force beats NO_COLOR.
    Always,
    /// Never color; output is byte-identical to the plain historical form.
    Never,
}

impl ColorChoice {
    /// Resolve against this process's stdout and environment.
    pub fn resolves(self) -> bool {
        let stdout_is_tty = std::io::stdout().is_terminal();
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        let colored = self.resolves_given(stdout_is_tty, no_color);
        // Classic conhost renders the escapes only once the stdout console
        // mode carries the VT flag; the decision itself is unchanged.
        #[cfg(windows)]
        if colored && stdout_is_tty {
            enable_vt();
        }
        colored
    }

    /// The pure decision core, testable without touching the environment:
    /// `always` forces color on, `never` forces it off, and `auto` colors
    /// only a terminal that has not asked for plainness through NO_COLOR.
    pub fn resolves_given(self, stdout_is_tty: bool, no_color_is_set: bool) -> bool {
        match self {
            ColorChoice::Auto => stdout_is_tty && !no_color_is_set,
            ColorChoice::Always => true,
            ColorChoice::Never => false,
        }
    }
}

/// Turn on `ENABLE_VIRTUAL_TERMINAL_PROCESSING` for the stdout console so
/// the escapes render on classic conhost; Windows Terminal ships with the
/// flag already set. Every other mode bit is preserved, and the answer is
/// false when stdout is not a console (pipe, file, detached) or the API
/// refuses - the caller keeps its color decision either way.
#[cfg(windows)]
fn enable_vt() -> bool {
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, CONSOLE_MODE,
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_OUTPUT_HANDLE,
    };

    let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return false;
    }
    let mut mode: CONSOLE_MODE = 0;
    unsafe {
        // Read the mode before writing it: a console that cannot be read
        // (stdout is a pipe or file) must not have its mode overwritten.
        GetConsoleMode(handle, &mut mode) != 0
            && SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

/// A resolved color setting plus the string-wrapping helpers built on it.
/// Construct once per command run and pass it down through the render
/// functions; the zero-sized decision is made in exactly one place.
#[derive(Debug, Clone, Copy)]
pub struct Painter {
    color: bool,
}

impl Painter {
    /// Resolve a `--color` choice against the real stdout and environment.
    pub fn for_choice(choice: ColorChoice) -> Self {
        Painter {
            color: choice.resolves(),
        }
    }

    /// Plain mode: never emits codes.
    pub fn plain() -> Self {
        Painter { color: false }
    }

    /// Forced-color mode: always emits codes (tests, and `--color always`
    /// plumbing).
    pub fn colored() -> Self {
        Painter { color: true }
    }

    /// Whether codes are emitted at all.
    pub fn is_colored(&self) -> bool {
        self.color
    }

    fn wrap(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("{code}{s}{RESET}")
        } else {
            s.to_string()
        }
    }

    /// Lavender accent - bars, feature names, secondary wins.
    pub fn accent(&self, s: &str) -> String {
        self.wrap(LAVENDER, s)
    }

    /// Green - the winning class.
    pub fn good(&self, s: &str) -> String {
        self.wrap(GREEN, s)
    }

    /// Amber - the low-confidence marker and boundary cautions.
    pub fn warn(&self, s: &str) -> String {
        self.wrap(AMBER, s)
    }

    /// Dim - metadata lines and probabilities.
    pub fn dim(&self, s: &str) -> String {
        self.wrap(DIM, s)
    }

    /// Bold - headers.
    pub fn bold(&self, s: &str) -> String {
        self.wrap(BOLD, s)
    }

    /// The rank bar: lavender block characters when colored, the historical
    /// ASCII `#` run when not. A zero-length bar is empty in both modes -
    /// colored mode emits no codes for the nothing it would wrap.
    pub fn bar(&self, len: usize) -> String {
        if len == 0 {
            return String::new();
        }
        if self.color {
            self.wrap(LAVENDER, &"\u{2588}".repeat(len))
        } else {
            "#".repeat(len)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_forces_color_on() {
        assert!(ColorChoice::Always.resolves_given(false, true));
        assert!(ColorChoice::Always.resolves_given(false, false));
        assert!(ColorChoice::Always.resolves_given(true, true));
    }

    #[test]
    fn never_forces_color_off() {
        assert!(!ColorChoice::Never.resolves_given(true, false));
        assert!(!ColorChoice::Never.resolves_given(true, true));
    }

    #[test]
    fn auto_needs_a_terminal() {
        // Test harnesses capture stdout through a pipe: auto stays plain.
        assert!(!ColorChoice::Auto.resolves_given(false, false));
        assert!(ColorChoice::Auto.resolves_given(true, false));
    }

    #[test]
    fn no_color_suppresses_auto_only() {
        assert!(!ColorChoice::Auto.resolves_given(true, true));
        // An explicit force is a direct user command and wins.
        assert!(ColorChoice::Always.resolves_given(true, true));
    }

    #[test]
    fn colored_painter_wraps_with_codes() {
        let p = Painter::colored();
        assert_eq!(p.good("gcc"), format!("{GREEN}gcc{RESET}"));
        assert_eq!(p.accent("x"), format!("{LAVENDER}x{RESET}"));
        assert_eq!(
            p.warn("[low confidence]"),
            format!("{AMBER}[low confidence]{RESET}")
        );
        assert_eq!(p.dim("99.90%"), format!("{DIM}99.90%{RESET}"));
        assert_eq!(p.bold("abi"), format!("{BOLD}abi{RESET}"));
        assert!(p.is_colored());
    }

    #[test]
    fn colored_bar_is_lavender_blocks() {
        let p = Painter::colored();
        assert_eq!(
            p.bar(3),
            format!("{LAVENDER}\u{2588}\u{2588}\u{2588}{RESET}")
        );
        // A zero-length bar wraps nothing, so it emits nothing.
        assert_eq!(p.bar(0), "");
    }

    #[test]
    fn plain_painter_is_the_identity_on_every_helper() {
        let p = Painter::plain();
        assert_eq!(p.accent("x"), "x");
        assert_eq!(p.good("gcc"), "gcc");
        assert_eq!(p.warn("w"), "w");
        assert_eq!(p.dim("1.23"), "1.23");
        assert_eq!(p.bold("h"), "h");
        assert_eq!(p.bar(4), "####");
        assert_eq!(p.bar(0), "");
        assert!(!p.is_colored());
    }

    #[test]
    fn color_choice_flag_values_parse() {
        // The clap ValueEnum derive spells the flag values kebab-case; pin
        // them so a rename cannot silently break scripts and completions.
        for (word, expected) in [
            ("auto", ColorChoice::Auto),
            ("always", ColorChoice::Always),
            ("never", ColorChoice::Never),
        ] {
            let found = <ColorChoice as clap::ValueEnum>::value_variants()
                .iter()
                .copied()
                .find(|v| {
                    <ColorChoice as clap::ValueEnum>::to_possible_value(v)
                        .expect("variant has a name")
                        .get_name()
                        == word
                });
            assert_eq!(found, Some(expected), "flag value {word:?} stays available");
        }
    }

    // The harness's stdout is a pipe, so the helper answers false here; in
    // an inherited console it flips the flag on. Either way the call must
    // not panic, must not lose mode bits, and must be idempotent.
    #[cfg(windows)]
    #[test]
    fn vt_enabling_is_idempotent_on_any_stdout_kind() {
        let first = enable_vt();
        assert_eq!(enable_vt(), first, "the call is idempotent");
    }
}
