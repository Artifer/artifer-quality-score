//! Terminal detection and JSON emission.

use crate::explain::{OBSERVATION_DOCS, RULE_DOCS};
use crate::models::ScanReport;
use crate::profile::Profile;
use crate::render::Style;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::io::IsTerminal;

fn terminal_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100)
}

pub fn style() -> Style {
    Style::new(std::io::stdout().is_terminal(), terminal_width())
}

pub fn print_json(value: &Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("report is serialisable")
    );
}

/// The scan report as JSON, minus the internal clone fingerprints.
///
/// Fingerprints exist to be joined and cached, not read: one per function would multiply
/// the size of `--json` on a large repository, and everything a consumer can act on is
/// already in `observations`. Stripping them here keeps the published report shape a
/// description of the code rather than of the analysis.
pub fn report_as_json(report: &ScanReport) -> Value {
    let mut value = serde_json::to_value(report).expect("report is serialisable");
    let files = value.get_mut("files").and_then(Value::as_array_mut);
    for file in files.into_iter().flatten() {
        if let Some(object) = file.as_object_mut() {
            object.remove("fingerprints");
        }
    }
    value
}

/// Fix guidance for just the rules and observations that actually fired.
pub fn guidance_for(report: &ScanReport) -> Value {
    let mut present: BTreeSet<&str> = report
        .files
        .iter()
        .flat_map(|file| file.all_findings())
        .map(|finding| finding.category.as_str())
        .collect();
    // An observation is only actionable if the reader knows what to do about it, and it
    // is the half of the output with no number attached to explain itself.
    present.extend(
        report
            .observations
            .iter()
            .filter(|o| o.is_open())
            .map(|o| o.category.as_str()),
    );

    let mut map = Map::new();
    for doc in RULE_DOCS.iter().chain(OBSERVATION_DOCS.iter()) {
        if present.contains(doc.category) {
            map.insert(
                doc.category.to_string(),
                json!({ "summary": doc.summary, "why": doc.why, "fix": doc.fix }),
            );
        }
    }
    Value::Object(map)
}

/// The thresholds governing one rule, as JSON.
pub fn thresholds_as_json(profile: &Profile, keys: &[&str]) -> Value {
    let mut map = Map::new();
    for key in keys {
        map.insert((*key).to_string(), json!(profile.threshold(key)));
    }
    Value::Object(map)
}

/// Every rule and observation as JSON, for `--list --json`.
///
/// `scored` is explicit rather than inferred from the presence of `weight`: a consumer
/// deciding what to act on should not have to know that a missing key means unscored.
pub fn rules_as_json(profile: &Profile) -> Value {
    let mut map = Map::new();
    for doc in RULE_DOCS {
        map.insert(
            doc.category.to_string(),
            json!({
                "summary": doc.summary,
                "why": doc.why,
                "fix": doc.fix,
                "scored": true,
                "weight": profile.rule(doc.category).weight,
                "thresholds": thresholds_as_json(profile, doc.thresholds),
            }),
        );
    }
    for doc in OBSERVATION_DOCS {
        map.insert(
            doc.category.to_string(),
            json!({
                "summary": doc.summary,
                "why": doc.why,
                "fix": doc.fix,
                "scored": false,
                "thresholds": thresholds_as_json(profile, doc.thresholds),
            }),
        );
    }
    Value::Object(map)
}

/// One rule as JSON, for `explain <rule> --json`.
pub fn one_rule_as_json(profile: &Profile, doc: &crate::explain::RuleDoc) -> Value {
    json!({
        "category": doc.category,
        "summary": doc.summary,
        "why": doc.why,
        "fix": doc.fix,
        "thresholds": thresholds_as_json(profile, doc.thresholds),
    })
}
