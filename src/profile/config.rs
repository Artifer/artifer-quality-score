//! Finding and applying an `.aqs.json`.
//!
//! Unknown rule *and* section names are hard errors. A typo that quietly disables a rule,
//! or a `"exclude_pattern"` that silently does nothing, is the worst possible failure mode
//! for a gate.

use super::{PatternSet, Profile, RuleConfig};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub const CONFIG_FILENAMES: &[&str] = &[".aqs.json", ".aqs/config.json"];

/// An empty path is not a directory and has no parent, so upward searches starting from
/// one silently find nothing. Treat it as the current directory.
fn normalise_start(start: &Path) -> PathBuf {
    if start.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        start.to_path_buf()
    }
}

/// The shadcn/ui component directory declared in `components.json`, as a repo-relative
/// path like `shared/ui`.
///
/// shadcn is configurable: across the Artifer repositories three of twelve put it in
/// `@/shared/ui` rather than the more common `@/components/ui`. Hardcoding one layout
/// silently scored the other three, reporting their thin `forwardRef` wrappers as
/// duplication — which is the library's intended shape, not a defect. `components.json`
/// states where it lives, so read it rather than guess.
pub fn detect_shadcn_ui(start: &Path) -> Option<String> {
    let start = normalise_start(start);
    let start = start.as_path();
    let mut directory: Option<&Path> = if start.is_dir() { Some(start) } else { start.parent() };
    while let Some(current) = directory {
        let candidate = current.join("components.json");
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).ok()?;
            let data: serde_json::Value = serde_json::from_str(&text).ok()?;
            let alias = data.get("aliases")?.get("ui")?.as_str()?;
            // "@/shared/ui" and "~/shared/ui" both mean "shared/ui" from the root.
            let path = alias
                .trim()
                .trim_start_matches('@')
                .trim_start_matches('~')
                .trim_start_matches('/')
                .trim_end_matches('/');
            return if path.is_empty() { None } else { Some(path.to_string()) };
        }
        directory = current.parent();
    }
    None
}

/// Nearest config file at or above `start`.
pub fn find_config(start: &Path) -> Option<PathBuf> {
    let start = normalise_start(start);
    let start = start.as_path();
    let mut directory: Option<&Path> = if start.is_dir() { Some(start) } else { start.parent() };
    while let Some(current) = directory {
        for filename in CONFIG_FILENAMES {
            let candidate = current.join(filename);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        directory = current.parent();
    }
    None
}

/// Fold in the shadcn directory a repository declares.
///
/// A declared component directory is a fact about the repository, not a preference, so
/// it applies whether or not there is an `.aqs.json`.
fn apply_shadcn(profile: &mut Profile, root: &Path) {
    let Some(ui) = detect_shadcn_ui(root) else { return };
    for pattern in [format!("{ui}/*"), format!("*/{ui}/*")] {
        profile.exclude_patterns.push_if_absent(pattern);
    }
}

fn apply_thresholds(profile: &mut Profile, thresholds: &serde_json::Value) -> Result<()> {
    let Some(map) = thresholds.as_object() else {
        bail!("thresholds must be an object");
    };
    for (key, value) in map {
        if !profile.thresholds.contains_key(key) {
            bail!("unknown threshold {key:?}. Run `aqs explain --thresholds` for valid names.");
        }
        let Some(number) = value.as_f64() else {
            bail!("threshold {key:?} must be a number, got {value}");
        };
        profile.thresholds.insert(key.clone(), number);
    }
    Ok(())
}

/// Apply one rule override, which is either a bare weight or a field object.
fn override_rule(name: &str, value: &serde_json::Value, rule: &mut RuleConfig) -> Result<()> {
    if let Some(weight) = value.as_f64() {
        rule.weight = weight;
        return Ok(());
    }
    let Some(fields) = value.as_object() else {
        bail!("rule {name:?} must be a number or an object");
    };
    for (field, raw) in fields {
        let Some(number) = raw.as_f64() else {
            bail!("rule {name:?} field {field:?} must be a number");
        };
        match field.as_str() {
            "base" => rule.base = number,
            "scale" => rule.scale = number,
            "cap" => rule.cap = number,
            "file_cap" => rule.file_cap = number,
            "weight" => rule.weight = number,
            other => bail!("rule {name:?} has unknown field(s): {other}"),
        }
    }
    Ok(())
}

fn apply_rules(profile: &mut Profile, rules: &serde_json::Value) -> Result<()> {
    let Some(map) = rules.as_object() else {
        bail!("rules must be an object");
    };
    for (name, value) in map {
        let Some(mut rule) = profile.rules.get(name).copied() else {
            bail!("unknown rule {name:?}. Run `aqs explain --list` for valid names.");
        };
        override_rule(name, value, &mut rule)?;
        profile.rules.insert(name.clone(), rule);
    }
    Ok(())
}

fn string_list(object: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<Vec<String>> {
    let array = object.get(key)?.as_array()?;
    Some(array.iter().filter_map(|v| v.as_str().map(String::from)).collect())
}

fn read_config(path: &Path) -> Result<serde_json::Map<String, serde_json::Value>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("{}: cannot read config", path.display()))?;
    let data: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("{}: invalid JSON", path.display()))?;
    match data {
        serde_json::Value::Object(object) => Ok(object),
        _ => bail!("{}: expected a JSON object at the top level", path.display()),
    }
}

/// Every section a config file may contain. Anything else is a typo.
const CONFIG_SECTIONS: &[&str] = &[
    "name",
    "thresholds",
    "rules",
    "test_patterns",
    "exclude_patterns",
    "test_exempt_rules",
];

/// Reject unrecognised top-level keys.
///
/// An unknown *rule* name is already a hard error; an unknown *section* name was not,
/// so `"exclude_pattern"` or `"rule"` silently did nothing. For a gate, a config that
/// quietly fails to apply is worse than one that refuses to load. Keys beginning with
/// `_` are reserved for comments, since JSON has none.
fn reject_unknown_sections(object: &serde_json::Map<String, serde_json::Value>) -> Result<()> {
    for key in object.keys() {
        if key.starts_with('_') || CONFIG_SECTIONS.contains(&key.as_str()) {
            continue;
        }
        bail!(
            "unknown config section {key:?}. Valid sections: {}. \
             Keys starting with `_` are treated as comments.",
            CONFIG_SECTIONS.join(", ")
        );
    }
    Ok(())
}

/// The default profile with any config file's overrides applied.
///
/// Unknown rule or threshold names are a hard error, never silently ignored: a typo
/// that quietly disables a rule is the worst possible failure mode for a gate.
pub fn load_profile(root: &Path, explicit: Option<&Path>) -> Result<Profile> {
    let mut profile = Profile::default();

    let config = match explicit {
        Some(path) => Some(path.to_path_buf()),
        None => find_config(root),
    };

    if let Some(config) = config {
        let object = read_config(&config)?;
        reject_unknown_sections(&object)?;
        if let Some(name) = object.get("name").and_then(|v| v.as_str()) {
            profile.name = name.to_string();
        }
        if let Some(thresholds) = object.get("thresholds") {
            apply_thresholds(&mut profile, thresholds)?;
        }
        if let Some(rules) = object.get("rules") {
            apply_rules(&mut profile, rules)?;
        }
        if let Some(patterns) = string_list(&object, "test_patterns") {
            profile.test_patterns = PatternSet::new(patterns);
        }
        if let Some(patterns) = string_list(&object, "exclude_patterns") {
            profile.exclude_patterns = PatternSet::new(patterns);
        }
        if let Some(rules) = string_list(&object, "test_exempt_rules") {
            profile.test_exempt_rules = PatternSet::new(rules);
        }
    }

    apply_shadcn(&mut profile, root);
    Ok(profile)
}
