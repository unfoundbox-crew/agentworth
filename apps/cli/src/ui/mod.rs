//! The one place the CLI decides how anything looks.
//!
//! Every human-facing screen renders through here: one grid, one rule weight, one accent.
//! `--json` payloads never touch this module.
//!
//! ALLOWED GLYPHS — ASCII, Box Drawing U+2500-257F (`─ │ ┌ ┐ └ ┘ ├ ┤`), Block Elements
//! U+2580-259F (`█ ░ ▂ ▄`), and exactly five more: `● ○` (U+25CF/25CB, the evidence meter),
//! `·` (U+00B7), `—` (U+2014), `→` (U+2192). Nothing else, and no emoji ever. Geist Mono
//! and SF Mono are both missing `▰ ▱ ◉ ✓ ✗`; a missing glyph falls back to another face at
//! a different advance width and silently shifts every column to its right.

use std::fmt::Write;

pub mod views;

/// Widest content the CLI ever draws. A 100-column window shows 22 columns of air; an
/// 80-column window still fits.
pub const MAX_CONTENT: usize = 78;

/// Terminal colour capability, resolved once at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// 24-bit `38;2;r;g;b` sequences.
    True,
    /// The 16-colour fallback. Violet has no ANSI-16 equivalent; magenta is the honest substitute.
    Ansi16,
    /// NO_COLOR, `--no-color`, `--plain`, or a non-TTY stdout.
    None,
}

/// What a piece of text is, not what colour it should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Rules, tracks, dot leaders.
    Chrome,
    /// Column heads and prose.
    Label,
    /// Ordinary numbers and text.
    Value,
    /// Totals and the command echo.
    Emphasis,
    /// Rungs 3-5 and the one primary number per screen. Nothing else.
    Verified,
    /// Rung 0 rows and empty meter cells.
    Unverified,
    Warn,
    Error,
}

impl Role {
    /// `(truecolor rgb, ansi-16 SGR params)`. The light and dark truecolor ramps differ only
    /// in which end of the zinc scale the chrome sits on, and a terminal does not tell us
    /// which it is — so the dark ramp is used, which is legible on both.
    fn codes(self) -> (&'static str, &'static str) {
        match self {
            Role::Chrome => ("38;2;63;63;70", "90"),
            Role::Label => ("38;2;161;161;170", "39"),
            Role::Value => ("38;2;212;212;216", "39"),
            Role::Emphasis => ("1;38;2;255;255;255", "1"),
            Role::Verified => ("38;2;163;150;214", "35"),
            Role::Unverified => ("38;2;113;113;122", "90"),
            Role::Warn => ("38;2;251;191;36", "33"),
            Role::Error => ("38;2;248;113;113", "31"),
        }
    }
}

/// The resolved rendering context: how wide, how colourful, which glyph set.
#[derive(Debug, Clone, Copy)]
pub struct Ui {
    width: usize,
    color: ColorMode,
    ascii: bool,
}

impl Ui {
    pub fn new(term_columns: usize, color: ColorMode, ascii: bool) -> Self {
        // Cap at MAX_CONTENT, leave a two-column right margin below that, and never
        // go so narrow that a table head cannot be read.
        let width = term_columns.saturating_sub(2).clamp(24, MAX_CONTENT);
        Ui {
            width,
            color,
            ascii,
        }
    }

    /// Resolve from the environment plus the two global flags.
    ///
    /// `COLUMNS` wins over the ioctl so tests (and `COLUMNS=60 agentworth stats`) are
    /// deterministic; NO_COLOR is honoured whatever its value, per no-color.org.
    pub fn detect(no_color_flag: bool, plain_flag: bool) -> Self {
        let term = console::Term::stdout();
        let is_tty = term.is_term();

        let columns = std::env::var("COLUMNS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|c| *c > 0)
            .or_else(|| term.size_checked().map(|(_, c)| c as usize))
            .unwrap_or(80);

        let no_color_env = std::env::var_os("NO_COLOR").is_some();
        let forced = matches!(std::env::var("CLICOLOR_FORCE").as_deref(), Ok(v) if v != "0");

        let color = if plain_flag || no_color_flag || no_color_env {
            ColorMode::None
        } else if !is_tty && !forced {
            ColorMode::None
        } else if std::env::var("COLORTERM")
            .map(|v| v == "truecolor" || v == "24bit")
            .unwrap_or(false)
        {
            ColorMode::True
        } else {
            ColorMode::Ansi16
        };

        // ASCII turns on by itself when stdout is not a TTY, so a piped or redirected
        // stream carries no glyph a downstream tool has to guess the width of.
        let ascii = plain_flag || (!is_tty && !forced);

        Ui::new(columns, color, ascii)
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn color(&self) -> ColorMode {
        self.color
    }

    pub fn ascii(&self) -> bool {
        self.ascii
    }

    /// Width available inside the two-space body indent.
    pub fn inner(&self) -> usize {
        self.width.saturating_sub(2)
    }

    /// Wrap `text` in the sequences for `role`. Column positions are byte-identical
    /// whether or not this adds anything.
    pub fn paint(&self, role: Role, text: &str) -> String {
        match self.color {
            ColorMode::None => text.to_string(),
            ColorMode::True => format!("\x1b[{}m{}\x1b[0m", role.codes().0, text),
            ColorMode::Ansi16 => format!("\x1b[{}m{}\x1b[0m", role.codes().1, text),
        }
    }

    // -- glyphs ---------------------------------------------------------------

    fn g(&self, unicode: &'static str, ascii: &'static str) -> &'static str {
        if self.ascii {
            ascii
        } else {
            unicode
        }
    }

    /// A rule at the full content width. One weight, always.
    pub fn rule(&self) -> String {
        self.rule_of(self.width)
    }

    pub fn rule_of(&self, n: usize) -> String {
        self.g("─", "-").repeat(n)
    }

    /// The evidence line: a rule with its name set into the middle of it.
    pub fn titled_rule(&self, n: usize, title: &str) -> String {
        let label = format!(" {} ", title);
        let label_w = label.chars().count();
        if label_w + 4 >= n {
            return self.rule_of(n);
        }
        let left = (n - label_w) / 2;
        let right = n - label_w - left;
        format!("{}{}{}", self.rule_of(left), label, self.rule_of(right))
    }

    /// Five cells, `filled` of them solid. Filled cells first and violet second, so the
    /// ladder still reads under NO_COLOR.
    pub fn meter(&self, filled: usize) -> String {
        let filled = filled.min(5);
        let on = self.g("●", "#");
        let off = self.g("○", ".");
        format!("{}{}", on.repeat(filled), off.repeat(5 - filled))
    }

    /// A magnitude bar. `fraction` is clamped to 0.0..=1.0.
    pub fn bar(&self, fraction: f64, width: usize) -> String {
        let full = self.g("█", "#");
        let track = self.g("░", ".");
        let filled = ((fraction.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
        format!("{}{}", full.repeat(filled), track.repeat(width - filled))
    }

    /// Not-measured. Never `0` — printing a zero for an adapter that keeps no token
    /// counts asserts something false.
    pub fn dash(&self) -> &'static str {
        self.g("—", "-")
    }

    pub fn arrow(&self) -> &'static str {
        self.g("→", "->")
    }

    pub fn dot(&self) -> &'static str {
        self.g("·", "-")
    }

    // -- lines ----------------------------------------------------------------

    /// The screen head: the command echoed at column 0, its context right-aligned.
    pub fn header(&self, command: &str, right: &str) -> String {
        let mut out = String::new();
        let command = truncate(command, self.width.saturating_sub(display_width(right) + 1));
        let gap = self
            .width
            .saturating_sub(display_width(&command) + display_width(right));
        let line = if right.is_empty() {
            self.paint(Role::Emphasis, &command)
        } else {
            format!(
                "{}{}{}",
                self.paint(Role::Emphasis, &command),
                " ".repeat(gap.max(1)),
                self.paint(Role::Label, right)
            )
        };
        let _ = writeln!(out, "{}", line);
        let _ = writeln!(out, "{}", self.paint(Role::Chrome, &self.rule()));
        out
    }

    /// A label, dot leaders, a right-aligned number. Receipt-shaped output only.
    pub fn leaders(&self, label: &str, value: &str, width: usize, value_role: Role) -> String {
        let lw = display_width(label);
        let vw = display_width(value);
        let dots = width.saturating_sub(lw + vw + 2);
        format!(
            "{} {} {}",
            self.paint(Role::Value, label),
            self.paint(Role::Chrome, &".".repeat(dots)),
            self.paint(value_role, value)
        )
    }

    /// An uppercase section head with a rule under it, indented two.
    pub fn section(&self, left: &str, right: &str) -> String {
        let mut out = String::new();
        let gap = self
            .inner()
            .saturating_sub(display_width(left) + display_width(right));
        let _ = writeln!(
            out,
            "  {}{}{}",
            self.paint(Role::Label, left),
            " ".repeat(gap.max(1)),
            self.paint(Role::Label, right)
        );
        let _ = writeln!(
            out,
            "  {}",
            self.paint(Role::Chrome, &self.rule_of(self.inner()))
        );
        out
    }

    /// The closing line every screen ends on. A dead end is a design bug.
    pub fn next(&self, command: &str, why: &str) -> String {
        format!(
            "  {}  {}   {}\n",
            self.paint(Role::Label, "Next"),
            self.paint(Role::Emphasis, command),
            self.paint(Role::Label, why)
        )
    }
}

// -- numbers ------------------------------------------------------------------

/// `1285563` -> `1,285,563`. Every column is the same width whatever the magnitude.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `92837128217` -> `92.8B`. One decimal, unit-suffixed, so the column never moves.
pub fn compact(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "0.0%".to_string();
    }
    format!("{:.1}%", (part as f64 / whole as f64) * 100.0)
}

/// The same share with no decimal, for a column too narrow to carry one.
pub fn percent_round(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "0%".to_string();
    }
    format!("{:.0}%", (part as f64 / whole as f64) * 100.0)
}

/// `40s`, `13m 37s`, `1h 24m`.
pub fn duration(seconds: f64) -> String {
    if seconds >= 3600.0 {
        format!(
            "{}h {}m",
            (seconds / 3600.0).floor(),
            ((seconds % 3600.0) / 60.0).floor()
        )
    } else if seconds >= 60.0 {
        format!(
            "{}m {:02}s",
            (seconds / 60.0).floor(),
            (seconds % 60.0).floor()
        )
    } else {
        format!("{}s", seconds.round())
    }
}

/// Printable width, ignoring any escape sequences already in `s`.
pub fn display_width(s: &str) -> usize {
    console::measure_text_width(s)
}

/// Cut to `max` columns, ending in `..` when anything was dropped. Never mid-escape,
/// because callers truncate before painting.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max <= 2 {
        s.chars().take(max).collect()
    } else {
        let head: String = s.chars().take(max - 2).collect();
        format!("{}..", head)
    }
}

/// Right-align `s` in `w` columns, measuring what prints rather than what is stored.
pub fn rpad(s: &str, w: usize) -> String {
    let pad = w.saturating_sub(display_width(s));
    format!("{}{}", " ".repeat(pad), s)
}

/// Left-align `s` in `w` columns.
pub fn lpad(s: &str, w: usize) -> String {
    let pad = w.saturating_sub(display_width(s));
    format!("{}{}", s, " ".repeat(pad))
}

/// Archie, reduced to the glyphs every mono face actually has. `eyes` is the face's
/// two-character middle: `▄ ▄` digging, `○ ○` done, `× ×` failed.
pub fn archie(ui: &Ui, eyes: &str) -> [String; 3] {
    if ui.ascii() {
        [
            "  _______".to_string(),
            format!(" ( {} )", eyes),
            "  '-+-+-'".to_string(),
        ]
    } else {
        [
            "▂▂▂▂▂▂▂".to_string(),
            format!("( {} )", eyes),
            "╰─┬─┬─╯".to_string(),
        ]
    }
}

/// Archie's eyes, in the current glyph set: digging, done, failed.
pub fn eyes(ui: &Ui, kind: EyeKind) -> String {
    match (kind, ui.ascii()) {
        (EyeKind::Digging, false) => "▄ ▄".to_string(),
        (EyeKind::Digging, true) => "v v".to_string(),
        (EyeKind::Done, false) => "○ ○".to_string(),
        (EyeKind::Done, true) => "o o".to_string(),
        (EyeKind::Failed, _) => "x x".to_string(),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum EyeKind {
    Digging,
    Done,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(w: usize) -> Ui {
        Ui::new(w, ColorMode::None, true)
    }

    #[test]
    fn width_caps_at_the_content_maximum() {
        assert_eq!(plain(80).width(), 78);
        assert_eq!(plain(120).width(), 78);
        assert_eq!(plain(200).width(), 78);
        assert_eq!(plain(60).width(), 58);
        assert_eq!(plain(20).width(), 24);
    }

    #[test]
    fn thousands_separates_every_magnitude() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_285_563), "1,285,563");
        assert_eq!(thousands(92_837_128_217), "92,837,128,217");
    }

    #[test]
    fn compact_holds_one_decimal_and_a_unit() {
        assert_eq!(compact(920), "920");
        assert_eq!(compact(367_300), "367.3K");
        assert_eq!(compact(12_800_000), "12.8M");
        assert_eq!(compact(92_837_128_217), "92.8B");
    }

    #[test]
    fn duration_reads_like_a_clock() {
        assert_eq!(duration(40.3), "40s");
        assert_eq!(duration(817.0), "13m 37s");
        assert_eq!(duration(5040.0), "1h 24m");
    }

    #[test]
    fn meter_is_five_cells_whatever_the_rung() {
        let ui = Ui::new(80, ColorMode::None, false);
        assert_eq!(ui.meter(0), "○○○○○");
        assert_eq!(ui.meter(2), "●●○○○");
        assert_eq!(ui.meter(5), "●●●●●");
        assert_eq!(ui.meter(9), "●●●●●");
        assert_eq!(plain(80).meter(2), "##...");
    }

    #[test]
    fn ascii_mode_holds_the_column_count() {
        let uni = Ui::new(80, ColorMode::None, false);
        let asc = plain(80);
        assert_eq!(display_width(&uni.rule()), display_width(&asc.rule()));
        assert_eq!(display_width(uni.meter(3).as_str()), display_width(asc.meter(3).as_str()));
        assert_eq!(display_width(&uni.bar(0.5, 30)), display_width(&asc.bar(0.5, 30)));
        assert_eq!(display_width(uni.dash()), display_width(asc.dash()));
    }

    #[test]
    fn colour_never_changes_a_column_position() {
        for mode in [ColorMode::True, ColorMode::Ansi16, ColorMode::None] {
            let ui = Ui::new(80, mode, false);
            let line = ui.leaders("CI green", "86", 76, Role::Verified);
            assert_eq!(display_width(&line), 76, "mode {:?}", mode);
        }
    }

    #[test]
    fn titled_rule_is_exactly_as_wide_as_asked() {
        let ui = Ui::new(80, ColorMode::None, false);
        for n in [30usize, 55, 76, 78] {
            assert_eq!(display_width(&ui.titled_rule(n, "the evidence line")), n);
        }
    }

    #[test]
    fn header_fills_the_content_width() {
        let ui = Ui::new(80, ColorMode::None, false);
        let head = ui.header("agentworth stats", "~/.agentworth/agentworth.db");
        for line in head.lines() {
            assert_eq!(display_width(line), 78);
        }
    }

    #[test]
    fn no_output_glyph_falls_outside_the_allowed_set() {
        let ui = Ui::new(80, ColorMode::None, false);
        let mut sample = String::new();
        sample.push_str(&ui.rule());
        sample.push_str(&ui.meter(3));
        sample.push_str(&ui.bar(0.4, 10));
        sample.push_str(ui.dash());
        sample.push_str(ui.arrow());
        sample.push_str(ui.dot());
        sample.push_str(&ui.titled_rule(40, "the evidence line"));
        for part in archie(&ui, &eyes(&ui, EyeKind::Digging)) {
            sample.push_str(&part);
        }
        for c in sample.chars() {
            let ok = c.is_ascii()
                || matches!(c as u32, 0x2500..=0x257F | 0x2580..=0x259F)
                || matches!(c, '●' | '○' | '·' | '—' | '→');
            assert!(ok, "glyph U+{:04X} ({}) is outside the allowed set", c as u32, c);
        }
    }
}
