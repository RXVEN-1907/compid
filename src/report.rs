//! Human-readable report rendering for classify, explain, functions, and
//! the one-line interpretation for compare.

use crate::features::ExtractedFeatures;
use crate::model::Prediction;
use std::collections::BTreeMap;

/// Render a full classification report (human table or JSON).
pub fn classification(
    bin: &std::path::Path,
    model_path: &std::path::Path,
    model_created: &str,
    feature_count: usize,
    ef: &ExtractedFeatures,
    pred: &Prediction,
    json: bool,
) -> String {
    if json {
        return serde_json::to_string_pretty(pred)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
    }

    let mut out = String::new();
    out.push_str(&format!("binary: {}\n", bin.display()));
    out.push_str(&format!(
        "model:   {} (trained {created}, {n} features)\n",
        model_path.display(),
        created = model_created,
        n = feature_count,
    ));
    if ef.build_id != "none" {
        out.push_str(&format!("build-id: {}\n", ef.build_id));
    }
    for f in &pred.fields {
        out.push('\n');
        out.push_str(&field_block(f));
    }
    out
}

/// One field's top-3 table block.
fn field_block(f: &crate::model::FieldPrediction) -> String {
    let mut out = String::new();
    out.push_str(&f.field);
    out.push('\n');
    let width = f
        .top3
        .iter()
        .map(|r| r.class.len())
        .max()
        .unwrap_or(0)
        .max(f.predicted.len())
        .max(4);
    for (rank, r) in f.top3.iter().enumerate() {
        let pct = r.probability * 100.0;
        let bar_len = (r.probability * 20.0).round() as usize;
        let bar: String = "#".repeat(bar_len);
        out.push_str(&format!(
            "  {rank}. {class:<width$} {pct:6.2}%  {bar}\n",
            class = r.class,
            pct = pct,
            bar = bar,
            width = width,
        ));
    }
    out
}

/// Render explain output: for every field, the winning class and the
/// features that pushed it there.
pub fn explain(bin: &std::path::Path, pred: &Prediction, json: bool) -> String {
    if json {
        return serde_json::to_string_pretty(pred)
            .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
    }
    let mut out = String::new();
    out.push_str(&format!("binary: {}\n\n", bin.display()));
    for f in &pred.fields {
        let top_pct = f.top3.first().map(|r| r.probability * 100.0).unwrap_or(0.0);
        out.push_str(&format!(
            "{}: {} ({top_pct:.1}% confidence)\n",
            f.field, f.predicted,
        ));
        if f.contributions.is_empty() {
            out.push_str("  (no positive contributions; prediction driven by the intercept)\n");
            continue;
        }
        out.push_str("  top contributing features:\n");
        let width = f
            .contributions
            .iter()
            .map(|c| c.feature.len())
            .max()
            .unwrap_or(0);
        for c in &f.contributions {
            out.push_str(&format!(
                "    {feature:<width$}  {sign}{value:.4}  (tf={tf})\n",
                feature = c.feature,
                sign = if c.contribution >= 0.0 { "+" } else { "" },
                value = c.contribution,
                tf = c.tf,
                width = width,
            ));
        }
        out.push('\n');
    }
    out
}

/// Plain-language reading of a cosine similarity.
pub fn interpret_similarity(sim: f64) -> &'static str {
    if sim >= 0.99 {
        "near-identical feature vectors; these binaries look like the same code built the same way"
    } else if sim >= 0.90 {
        "very similar; likely the same toolchain and optimization settings"
    } else if sim >= 0.70 {
        "similar; plausibly the same compiler family with different settings"
    } else if sim >= 0.40 {
        "quite different; different toolchains, settings, or mostly different code"
    } else {
        "very different; these binaries share almost no code shape"
    }
}

/// One function's row in the `compid functions` table. `family` and
/// `opt_level` carry the predicted class and its confidence when a
/// function-level model is loaded, and are `None` for a plain listing.
#[derive(Debug, Clone)]
pub struct FunctionRow {
    pub name: String,
    /// Preformatted hexadecimal address (as the symbol table records it).
    pub addr: String,
    pub size: u64,
    pub family: Option<(String, f64)>,
    pub opt_level: Option<(String, f64)>,
}

/// Display form of a raw symbol name: capped to 48 characters (counted in
/// chars, not bytes, so multi-byte names stay valid UTF-8) with an ASCII
/// ellipsis. The JSON output keeps the full name.
fn display_name(name: &str) -> String {
    const NAME_CAP: usize = 48;
    if name.chars().count() > NAME_CAP {
        let keep: String = name.chars().take(NAME_CAP - 3).collect();
        format!("{keep}...")
    } else {
        name.to_string()
    }
}

/// Render the per-function provenance table. When `model` is `Some`, rows
/// carry prediction columns and the footer counts predicted families; when
/// `reason` is `Some` there is nothing to list and the reason stands alone.
pub fn function_provenance(
    bin: &std::path::Path,
    build_id: &str,
    reason: Option<&str>,
    model: Option<(&std::path::Path, &str, usize)>,
    rows: &[FunctionRow],
) -> String {
    let mut out = String::new();
    out.push_str(&format!("binary: {}\n", bin.display()));
    if build_id != "none" {
        out.push_str(&format!("build-id: {build_id}\n"));
    }
    if let Some((path, created, n)) = model {
        out.push_str(&format!(
            "model:   {} (trained {created}, {n} features)\n",
            path.display()
        ));
    }
    if rows.is_empty() {
        out.push_str(&match reason {
            Some(r) => format!("\nno functions: {r}\n"),
            None => "\nno functions\n".to_string(),
        });
        return out;
    }

    let predicted = rows.iter().any(|r| r.family.is_some());
    // Raw symbol-table names can run hundreds of characters (mangled Zig or
    // C++ template names); the table truncates for display, the JSON output
    // never does.
    let display: Vec<String> = rows.iter().map(|r| display_name(&r.name)).collect();
    let name_w = display.iter().map(|s| s.len()).max().unwrap_or(0).max(4);
    let addr_w = rows.iter().map(|r| r.addr.len()).max().unwrap_or(0).max(3);
    let size_w = rows
        .iter()
        .map(|r| r.size.to_string().len())
        .max()
        .unwrap_or(0)
        .max(4);

    out.push('\n');
    if predicted {
        out.push_str(&format!(
            "{:<name_w$}  {:>addr_w$}  {:>size_w$}  compiler_family        opt_level\n",
            "name", "addr", "size",
        ));
    } else {
        out.push_str(&format!(
            "{:<name_w$}  {:>addr_w$}  {:>size_w$}\n",
            "name", "addr", "size",
        ));
    }
    for (i, r) in rows.iter().enumerate() {
        if !predicted {
            out.push_str(&format!(
                "{:<name_w$}  {:>addr_w$}  {:>size_w$}\n",
                display[i], r.addr, r.size,
            ));
            continue;
        }
        // Each prediction column renders independently: a model may carry
        // only one of the two fields.
        let cell = |pred: &Option<(String, f64)>| match pred {
            Some((c, p)) => format!("{:<width$} {:<5.2}", c, p, width = 12),
            None => " ".repeat(18),
        };
        out.push_str(&format!(
            "{:<name_w$}  {:>addr_w$}  {:>size_w$}  {}  {}\n",
            display[i],
            r.addr,
            r.size,
            cell(&r.family),
            cell(&r.opt_level),
        ));
    }

    if predicted {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        let mut with_family = 0usize;
        for r in rows {
            if let Some((f, _)) = &r.family {
                *counts.entry(f.as_str()).or_insert(0) += 1;
                with_family += 1;
            }
        }
        let tally = counts
            .iter()
            .map(|(f, n)| format!("{f}={n}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "\n{} functions; predicted families: {tally}\n",
            rows.len()
        ));
        if with_family != rows.len() {
            out.push_str(&format!(
                "note: {} of {} functions carry no compiler_family prediction\n",
                rows.len() - with_family,
                rows.len()
            ));
        }
    } else {
        out.push_str(&format!("\n{} functions\n", rows.len()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn similarity_words_match_the_scale() {
        assert!(interpret_similarity(1.0).contains("near-identical"));
        assert!(interpret_similarity(0.95).contains("very similar"));
        assert!(interpret_similarity(0.75).contains("similar"));
        assert!(interpret_similarity(0.5).contains("quite different"));
        assert!(interpret_similarity(0.1).contains("very different"));
    }

    fn demo_rows() -> Vec<FunctionRow> {
        vec![
            FunctionRow {
                name: "handle".to_string(),
                addr: "0x401160".to_string(),
                size: 148,
                family: Some(("gcc".to_string(), 0.97)),
                opt_level: Some(("O2".to_string(), 0.61)),
            },
            FunctionRow {
                name: "emit".to_string(),
                addr: "0x401200".to_string(),
                size: 96,
                family: Some(("clang".to_string(), 0.88)),
                opt_level: Some(("O2".to_string(), 0.55)),
            },
        ]
    }

    #[test]
    fn function_table_with_predictions_has_tally_footer() {
        let text = function_provenance(
            Path::new("mixed-demo"),
            "d1294f",
            None,
            Some((Path::new("model/functions.json"), "2026-10-20", 3121)),
            &demo_rows(),
        );
        assert!(text.contains("binary: mixed-demo"));
        assert!(text.contains("build-id: d1294f"));
        assert!(text.contains("model:   model/functions.json"));
        assert!(text.contains("handle"));
        assert!(text.contains("gcc"));
        assert!(text.contains("clang"));
        assert!(text.contains("2 functions; predicted families: clang=1, gcc=1"));
    }

    #[test]
    fn function_table_without_model_lists_plainly() {
        let rows: Vec<FunctionRow> = demo_rows()
            .into_iter()
            .map(|mut r| {
                r.family = None;
                r.opt_level = None;
                r
            })
            .collect();
        let text = function_provenance(Path::new("b"), "none", None, None, &rows);
        assert!(!text.contains("compiler_family"));
        assert!(!text.contains("model:"));
        assert!(text.contains("2 functions\n"));
        assert!(!text.contains("predicted families"));
    }

    #[test]
    fn function_table_reports_the_reason_when_empty() {
        let text = function_provenance(
            Path::new("stripped"),
            "none",
            Some("no symbol table: binary looks stripped"),
            None,
            &[],
        );
        assert!(text.contains("no functions: no symbol table: binary looks stripped"));
    }
}
