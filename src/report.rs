//! Human-readable report rendering for classify and explain, plus the
//! one-line interpretation for compare.

use crate::features::ExtractedFeatures;
use crate::model::Prediction;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_words_match_the_scale() {
        assert!(interpret_similarity(1.0).contains("near-identical"));
        assert!(interpret_similarity(0.95).contains("very similar"));
        assert!(interpret_similarity(0.75).contains("similar"));
        assert!(interpret_similarity(0.5).contains("quite different"));
        assert!(interpret_similarity(0.1).contains("very different"));
    }
}
