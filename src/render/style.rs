//! Colour, score bands, and text fitting.
//!
//! Colour is a property of the number, not of the writer, so `score_colour` is a free
//! function. `Style` carries only whether the terminal wants ANSI and how wide it is.

pub(crate) const HEALTHY: f64 = 8.0;

pub(crate) const WARNING: f64 = 4.0;

pub(crate) const RESET: &str = "\x1b[0m";

pub(crate) const BOLD: &str = "\x1b[1m";

pub(crate) const DIM: &str = "\x1b[2m";

pub(crate) const RED: &str = "\x1b[31m";

pub(crate) const GREEN: &str = "\x1b[32m";

pub(crate) const YELLOW: &str = "\x1b[33m";

pub(crate) const CYAN: &str = "\x1b[36m";

pub(crate) const MAGENTA: &str = "\x1b[35m";

/// The colour a score is shown in. A free function: it depends only on the number.
pub(crate) fn score_colour(score: f64) -> &'static str {
    if score >= HEALTHY {
        GREEN
    } else if score >= WARNING {
        YELLOW
    } else {
        RED
    }
}

pub struct Style {
    pub(crate) colour: bool,
    pub width: usize,
}

impl Style {
    pub fn new(colour: bool, width: usize) -> Self {
        Self { colour, width }
    }

    pub(crate) fn paint(&self, text: &str, code: &str) -> String {
        if self.colour {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    }

    pub(crate) fn score(&self, score: f64, width: usize) -> String {
        let text = format!("{score:>width$.2}");
        if self.colour {
            format!("{BOLD}{}{}{RESET}", score_colour(score), text)
        } else {
            text
        }
    }

    pub(crate) fn change(&self, change: f64) -> String {
        if change > 0.0 {
            self.paint(&format!("+{change:.2}"), &format!("{BOLD}{GREEN}"))
        } else if change < 0.0 {
            self.paint(&format!("{change:.2}"), &format!("{BOLD}{RED}"))
        } else {
            self.paint("  0.00", DIM)
        }
    }
}

/// Pad or ellipsise `text` to exactly `width` display columns.
pub(crate) fn fit(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        let mut out: String = text.to_string();
        out.extend(std::iter::repeat(' ').take(width - chars.len()));
        return out;
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut out: String = chars[..width - 1].iter().collect();
    out.push('…');
    out
}

pub(crate) fn number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

pub(crate) fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

pub(crate) fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// The bold title line shared by every report.
pub(crate) fn print_title(title: &str, detail: &str, style: &Style) {
    println!();
    println!("  {}{}", style.paint(title, BOLD), style.paint(detail, DIM));
    println!();
}
