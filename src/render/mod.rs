//! Terminal rendering.
//!
//! Worst-first everywhere. A report that opens with the files you should look at is
//! useful in a TDD loop; one that opens with an alphabetical list is a wall to scroll
//! past.
//!
//! | module | renders |
//! |---|---|
//! | [`style`] | colour, score bands, text fitting |
//! | [`layout`] | column widths and one finding row |
//! | [`scan`] | the `scan` report |
//! | [`delta`] | the `delta` report and the shared changed-file rows |
//! | [`gate`] | the `gate` report |
//! | [`explain`] | `explain`, `--list`, `--thresholds` |
//! | [`check`] | lint-style lines |

use crate::explain::{RuleDoc, OBSERVATION_DOCS, RULE_DOCS};
use crate::models::{DeltaReport, FileDelta, FileReport, Finding, GateReport, ScanReport};
use crate::profile::Profile;

pub mod check;
pub mod delta;
pub mod explain;
pub mod gate;
pub mod layout;
pub mod scan;
pub mod style;

pub use check::render_check;
pub use delta::render_delta;
pub use explain::{render_explain, render_rule_list, render_thresholds};
pub use gate::render_gate;
pub use scan::render_scan;
pub use style::Style;

// Shared between the report modules; callers outside `render` never need these.
use delta::{delta_heading, delta_rows};
use layout::{file_findings, finding_line, Layout, GAP, INDENT, PENALTY_WIDTH};
use style::{
    fit, number, print_title, score_colour, thousands, wrap, BOLD, CYAN, DIM, GREEN, MAGENTA, RED,
    YELLOW,
};
