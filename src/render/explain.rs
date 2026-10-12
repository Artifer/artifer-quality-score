//! `aqs explain`, `--list` and `--thresholds`.

use super::*;

pub fn render_explain(doc: &RuleDoc, profile: &Profile, style: &Style) {
    println!();
    println!("  {}", style.paint(doc.category, &format!("{BOLD}{CYAN}")));
    println!();
    for chunk in wrap(doc.summary, 76) {
        println!("  {chunk}");
    }
    println!();
    for (label, body) in [("Why it matters", doc.why), ("How to fix it", doc.fix)] {
        println!("  {}", style.paint(label, BOLD));
        for chunk in wrap(body, 76) {
            println!("    {chunk}");
        }
        println!();
    }

    render_rule_config(doc, profile, style);
}

/// The thresholds and weight that govern one rule.
///
/// An observation has thresholds but no penalty config, so the weight line is replaced by
/// a statement that it is not scored. `profile.rule` would panic on the lookup, and the
/// reader needs to be told the difference regardless.
pub(crate) fn render_rule_config(doc: &RuleDoc, profile: &Profile, style: &Style) {
    println!("  {}", style.paint("Configuration", BOLD));
    for key in doc.thresholds {
        let value = profile.threshold(key);
        println!(
            "{}",
            style.paint(&format!("    {:<52} {}", key, number(value)), DIM)
        );
    }
    let summary = match profile.rules.get(doc.category) {
        Some(rule) => format!(
            "    {:<52} {}  (base {}, cap {}, file cap {})",
            "weight", rule.weight, rule.base, rule.cap, rule.file_cap
        ),
        None => format!(
            "    {:<52} {}",
            "weight", "not scored — reported as an observation"
        ),
    };
    println!("{}", style.paint(&summary, DIM));
    println!();
}

pub fn render_rule_list(profile: &Profile, style: &Style) {
    println!();
    println!(
        "  {:<38}{:>7}{:>7}  {}",
        style.paint("Rule", BOLD),
        style.paint("Weight", BOLD),
        style.paint("Max", BOLD),
        style.paint("What it measures", BOLD)
    );
    let available = style.width.saturating_sub(58).max(20);
    for doc in RULE_DOCS {
        let rule = profile.rule(doc.category);
        println!(
            "  {}{:>7.2}{:>7.2}  {}",
            style.paint(&fit(doc.category, 38), CYAN),
            rule.weight,
            rule.cap,
            fit(doc.summary, available)
        );
    }
    // Listed here too, because the directive parser accepts these names and its error
    // message sends people to `--list` to find valid ones. Under their own heading,
    // because the Weight and Max columns above do not apply to them.
    println!();
    println!(
        "  {}{}",
        style.paint(&fit("Observation", 38), BOLD),
        style.paint(
            "not scored — reported, resolved by fixing or dismissing",
            BOLD
        )
    );
    for doc in OBSERVATION_DOCS {
        println!(
            "  {}{}",
            style.paint(&fit(doc.category, 38), MAGENTA),
            fit(doc.summary, available)
        );
    }

    println!();
    println!(
        "{}",
        style.paint(
            "  Run `aqs explain <rule>` for why it matters and how to fix it.",
            DIM
        )
    );
    println!();
}

pub fn render_thresholds(profile: &Profile, style: &Style) {
    println!();
    println!(
        "  {:<54}{}",
        style.paint("Threshold", BOLD),
        style.paint("Value", BOLD)
    );
    for (key, value) in &profile.thresholds {
        println!("  {}{}", style.paint(&fit(key, 54), CYAN), number(*value));
    }
    println!();
}
