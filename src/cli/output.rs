//! Terminal detection and JSON emission.

use crate::explain::RULE_DOCS;
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
    println!("{}", serde_json::to_string_pretty(value).expect("report is serialisable"));
}

/// Fix guidance for just the rules that actually fired.
pub fn guidance_for(report: &ScanReport) -> Value {
    let present: BTreeSet<&str> = report
        .files
        .iter()
        .flat_map(|file| file.all_findings())
        .map(|finding| finding.category.as_str())
        .collect();
    let mut map = Map::new();
    for doc in RULE_DOCS {
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

/// Every rule as JSON, for `--list --json`.
pub fn rules_as_json(profile: &Profile) -> Value {
    let mut map = Map::new();
    for doc in RULE_DOCS {
        map.insert(
            doc.category.to_string(),
            json!({
                "summary": doc.summary,
                "why": doc.why,
                "fix": doc.fix,
                "weight": profile.rule(doc.category).weight,
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
