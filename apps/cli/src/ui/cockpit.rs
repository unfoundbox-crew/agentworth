//! The cockpit: the CLI grammar with a cursor.
//!
//! `docs/specs/cli-grammar.md` section 3. Six screens, all of them strings that
//! `apps/cli/src/ui/views.rs` (or a command module that itself renders through it) already
//! produces for a printed command. What this module adds is a viewport, a cursor and key
//! handling — nothing else. The binding rule from the spec: **no view function may exist
//! that only the cockpit calls.** `apps/cli/tests/cockpit.rs` enforces it by walking every
//! `pub fn` in `views.rs` and requiring a reference from outside this module.
//!
//! Read-only, permanently. Nothing here writes, scans, changes config, or calls a model.
//! The index is opened for reading; a missing one shows the same line the CLI shows and
//! quits on any key.

use std::io::IsTerminal;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::{truncate, Ui};

/// Rows a list screen asks the index for. The same bound `session list` and `repo list`
/// default to, so opening the cockpit costs one bounded query per screen and never a scan.
pub const LIST_LIMIT: usize = 50;

/// Windows the windows screen asks for, and the width of each.
pub const WINDOW_COUNT: usize = 12;
pub const WINDOW_HOURS: i64 = 5;

/// How much index work the cockpit is allowed before its first screen, over and above what
/// the same binary costs to start at all. Asserted against a fixture index in
/// `apps/cli/tests/cockpit.rs`, the same shape as the Tab budget.
pub const OPEN_BUDGET_MS: u128 = 900;

// -----------------------------------------------------------------------------
// state
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Overview,
    Sessions,
    Session,
    Agents,
    Repos,
    Windows,
}

impl Screen {
    /// The `1`-`6` order, which is also the order of the spec's table.
    pub const ORDER: [Screen; 6] = [
        Screen::Overview,
        Screen::Sessions,
        Screen::Session,
        Screen::Agents,
        Screen::Repos,
        Screen::Windows,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Screen::Overview => "overview",
            Screen::Sessions => "sessions",
            Screen::Session => "session",
            Screen::Agents => "agents",
            Screen::Repos => "repos",
            Screen::Windows => "windows",
        }
    }

    /// True where a cursor picks a row rather than scrolling a body.
    pub fn is_list(self) -> bool {
        matches!(self, Screen::Sessions | Screen::Repos)
    }
}

/// Which reading of one session is on screen. Each is a printed command in its own right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Show,
    Handoff,
    Asks,
    Forgotten,
    Receipt,
}

impl Tab {
    pub fn name(self) -> &'static str {
        match self {
            Tab::Show => "show",
            Tab::Handoff => "handoff",
            Tab::Asks => "asks",
            Tab::Forgotten => "forgotten",
            Tab::Receipt => "receipt",
        }
    }
}

/// A key, as the state machine sees one. Deliberately not crossterm's own type: the key
/// handling is unit-tested without a terminal (`apps/cli/src/ui/cockpit.rs`'s own tests),
/// and `from_crossterm` below is the only place the two vocabularies meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Esc,
    Backspace,
    Char(char),
}

impl Key {
    pub fn from_crossterm(k: KeyEvent) -> Option<Key> {
        // A key that is only *released* would otherwise move the cursor twice on Windows,
        // where crossterm reports both edges.
        if k.kind == KeyEventKind::Release {
            return None;
        }
        // Ctrl-C is a quit whatever the screen is doing, filter entry included.
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('c')) {
            return Some(Key::Char('q'));
        }
        Some(match k.code {
            KeyCode::Up => Key::Up,
            KeyCode::Down => Key::Down,
            KeyCode::PageUp => Key::PageUp,
            KeyCode::PageDown => Key::PageDown,
            KeyCode::Home => Key::Home,
            KeyCode::End => Key::End,
            KeyCode::Enter => Key::Enter,
            KeyCode::Esc => Key::Esc,
            KeyCode::Backspace => Key::Backspace,
            KeyCode::Char(c) => Key::Char(c),
            _ => return None,
        })
    }
}

/// What the state machine wants the runner to do next. Every variant is a read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    None,
    /// Load the screen's data again — a screen changed, or a filter did.
    Reload,
    /// Drill into the session at this row of the sessions list.
    OpenRow(usize),
    Quit,
}

/// Everything the cockpit knows that is not data: which screen, where the cursor is, what
/// is typed into the filter. Pure — no terminal, no storage, no I/O — so the whole key
/// contract is unit-testable.
#[derive(Debug, Clone)]
pub struct App {
    pub screen: Screen,
    pub tab: Tab,
    /// Selected row on a list screen.
    pub cursor: usize,
    /// First body line drawn, on a screen that scrolls.
    pub scroll: usize,
    pub filter: String,
    /// True while `/` has the keyboard.
    pub filtering: bool,
    pub help: bool,
    /// The session the `session` screen is reading, if one has been opened.
    pub open_session: Option<String>,
    /// Rows the current list holds, set by the runner after each load.
    pub rows: usize,
    /// Body lines the current screen holds, set by the runner after each render.
    pub lines: usize,
    /// Body height in rows, set by the runner from the terminal size.
    pub viewport: usize,
    /// Where the current screen's selectable rows sit among its body lines, set by the
    /// runner after each render. `None` on a screen with no rows.
    pub row_span: Option<(usize, usize)>,
}

impl Default for App {
    fn default() -> Self {
        App {
            screen: Screen::Overview,
            tab: Tab::Show,
            cursor: 0,
            scroll: 0,
            filter: String::new(),
            filtering: false,
            help: false,
            open_session: None,
            rows: 0,
            lines: 0,
            viewport: 20,
            row_span: None,
        }
    }
}

impl App {
    /// The whole key contract, in one place.
    ///
    /// Order matters and is the reason this reads as a ladder rather than a match: filter
    /// entry swallows printable keys, and help swallows everything, so both are tested
    /// before the screen's own bindings get a look.
    pub fn on_key(&mut self, key: Key) -> Cmd {
        if self.filtering {
            return self.filter_key(key);
        }
        if self.help {
            // Any key closes help. `?` toggling it back on would make the key feel stuck.
            self.help = false;
            return Cmd::None;
        }

        match key {
            Key::Char('q') => Cmd::Quit,
            Key::Char('?') => {
                self.help = true;
                Cmd::None
            }
            Key::Char('/') if self.screen.is_list() => {
                self.filtering = true;
                Cmd::None
            }
            Key::Char(c @ '1'..='6') => {
                let want = Screen::ORDER[(c as u8 - b'1') as usize];
                self.go(want)
            }
            Key::Char('h') => self.open_tab(Tab::Handoff),
            Key::Char('a') => self.open_tab(Tab::Asks),
            Key::Char('f') => self.open_tab(Tab::Forgotten),
            Key::Char('r') => self.open_tab(Tab::Receipt),
            Key::Char('j') | Key::Down => self.move_by(1),
            Key::Char('k') | Key::Up => self.move_by(-1),
            Key::PageDown => self.move_by(self.viewport.max(1) as isize),
            Key::PageUp => self.move_by(-(self.viewport.max(1) as isize)),
            Key::Home => {
                self.cursor = 0;
                self.scroll = 0;
                Cmd::None
            }
            Key::End => {
                if self.screen.is_list() {
                    self.cursor = self.rows.saturating_sub(1);
                } else {
                    self.scroll = self.lines.saturating_sub(self.viewport);
                }
                Cmd::None
            }
            Key::Enter if self.screen == Screen::Sessions && self.rows > 0 => {
                self.tab = Tab::Show;
                Cmd::OpenRow(self.cursor)
            }
            Key::Esc => self.back(),
            _ => Cmd::None,
        }
    }

    fn filter_key(&mut self, key: Key) -> Cmd {
        match key {
            Key::Esc => {
                self.filtering = false;
                let had = !self.filter.is_empty();
                self.filter.clear();
                self.cursor = 0;
                if had {
                    Cmd::Reload
                } else {
                    Cmd::None
                }
            }
            Key::Enter => {
                self.filtering = false;
                Cmd::None
            }
            Key::Backspace => {
                self.filter.pop();
                self.cursor = 0;
                Cmd::Reload
            }
            Key::Char(c) => {
                self.filter.push(c);
                self.cursor = 0;
                Cmd::Reload
            }
            _ => Cmd::None,
        }
    }

    /// `Esc`: out of a reading back to the session, out of the session back to the list,
    /// out of a filter, then back to the overview. Never straight to a quit — a key that
    /// both backs up and exits eventually exits by accident.
    ///
    /// The first step is why `show` needs no key of its own: `h`/`a`/`f`/`r` go in, and
    /// `Esc` is the way back, which is the same thing it means everywhere else here.
    fn back(&mut self) -> Cmd {
        if self.screen == Screen::Session {
            if self.tab != Tab::Show {
                self.tab = Tab::Show;
                self.scroll = 0;
                return Cmd::Reload;
            }
            return self.go(Screen::Sessions);
        }
        if !self.filter.is_empty() {
            self.filter.clear();
            self.cursor = 0;
            return Cmd::Reload;
        }
        if self.screen != Screen::Overview {
            return self.go(Screen::Overview);
        }
        Cmd::None
    }

    fn go(&mut self, want: Screen) -> Cmd {
        // `3` with nothing opened yet means "the row the cursor is on", which is the only
        // reading that does not need a session the cockpit has not been told about.
        if want == Screen::Session && self.open_session.is_none() {
            if self.screen == Screen::Sessions && self.rows > 0 {
                self.tab = Tab::Show;
                return Cmd::OpenRow(self.cursor);
            }
            return Cmd::None;
        }
        if self.screen == want {
            return Cmd::None;
        }
        self.screen = want;
        self.scroll = 0;
        self.filtering = false;
        if want != Screen::Session {
            self.cursor = 0;
            self.filter.clear();
        }
        Cmd::Reload
    }

    /// `h`/`a`/`f`/`r`. On the sessions list they open the highlighted session straight
    /// onto that reading, so a person never has to press Enter first.
    fn open_tab(&mut self, tab: Tab) -> Cmd {
        self.tab = tab;
        self.scroll = 0;
        match self.screen {
            Screen::Session => Cmd::Reload,
            Screen::Sessions if self.rows > 0 => Cmd::OpenRow(self.cursor),
            _ if self.open_session.is_some() => {
                self.screen = Screen::Session;
                Cmd::Reload
            }
            _ => Cmd::None,
        }
    }

    fn move_by(&mut self, delta: isize) -> Cmd {
        if self.screen.is_list() && self.rows > 0 {
            let last = self.rows - 1;
            let next = (self.cursor as isize + delta).clamp(0, last as isize) as usize;
            self.cursor = next;
            self.follow_cursor();
        } else {
            let max = self.lines.saturating_sub(self.viewport);
            let next = (self.scroll as isize + delta).clamp(0, max as isize) as usize;
            self.scroll = next;
        }
        Cmd::None
    }

    /// Keep the selected row inside the viewport. The cursor leads; the scroll follows.
    fn follow_cursor(&mut self) {
        let Some((start, _)) = self.row_span else {
            return;
        };
        let line = start + self.cursor;
        if line < self.scroll {
            self.scroll = line;
        } else if self.viewport > 0 && line >= self.scroll + self.viewport {
            self.scroll = line + 1 - self.viewport;
        }
    }
}

// -----------------------------------------------------------------------------
// chrome
// -----------------------------------------------------------------------------

/// The one-line key legend under every screen.
pub const KEY_LEGEND: &str =
    "j/k move  Enter open  Esc back  / filter  1-6 screens  h a f r  ? help  q quit";

/// The help overlay, line by line. Every line here is also a row of README.md's key table.
pub const HELP_LINES: &[(&str, &str)] = &[
    ("j / k, down / up", "move the cursor"),
    ("Enter", "drill into the highlighted session"),
    ("Esc", "back: out of a reading, a session, then a filter"),
    ("/", "filter the current list; Esc clears it"),
    ("1 - 6", "overview, sessions, session, agents, repos, windows"),
    ("h", "this session's handoff"),
    ("a", "the questions it asked, and where the answers are"),
    ("f", "what compaction dropped"),
    ("r", "its Flight Receipt"),
    ("?", "this screen"),
    ("q", "quit"),
];

/// The headline the cockpit shows against an index with nothing in it -- the same one
/// `archie session list` prints, so the two surfaces say one thing.
pub const NO_INDEX_LINE: &str = "No sessions found in index.";

/// Every fixed string the cockpit draws that no view function produced. The glyph test in
/// `ui/mod.rs` walks this so the cockpit's own chrome is held to the same set as every
/// printed screen.
pub fn chrome_samples(ui: &Ui) -> Vec<String> {
    let mut out = vec![
        KEY_LEGEND.to_string(),
        NO_INDEX_LINE.to_string(),
        "any key to quit".to_string(),
    ];
    for (k, v) in HELP_LINES {
        out.push(format!("  {}   {}", k, v));
    }
    for screen in Screen::ORDER {
        out.push(status_line(ui, screen, Tab::Show, "", 0, 0));
        out.push(tab_bar(ui, Tab::Handoff, "session_a1b2"));
    }
    out.push(status_line(ui, Screen::Sessions, Tab::Show, "vector", 3, 12));
    out
}

/// The top line: which screen, where in it, and the filter if one is on.
pub fn status_line(
    ui: &Ui,
    screen: Screen,
    tab: Tab,
    filter: &str,
    cursor: usize,
    rows: usize,
) -> String {
    let mut left = String::from("archie ");
    for s in Screen::ORDER {
        let mark = if s == screen { "[" } else { " " };
        let close = if s == screen { "]" } else { " " };
        left.push_str(&format!("{}{}{}", mark, s.name(), close));
    }
    if screen == Screen::Session {
        left.push_str(&format!("  {} {}", ui.dot(), tab.name()));
    }
    let right = if !filter.is_empty() {
        format!("/{}  {} of {}", filter, cursor + 1, rows.max(1))
    } else if rows > 0 {
        format!("{} of {}", cursor + 1, rows)
    } else {
        String::new()
    };
    let gap = ui
        .width()
        .saturating_sub(super::display_width(&left) + super::display_width(&right));
    format!("{}{}{}", left, " ".repeat(gap.max(1)), right)
}

/// The session screen's reading selector.
///
/// `show` carries no key letter because it has no key: it is where the screen opens, and
/// `Esc` is the way back to it. Naming a key that does nothing is worse than naming none.
pub fn tab_bar(ui: &Ui, tab: Tab, session_id: &str) -> String {
    let mut out = format!("{}  {} ", truncate(session_id, 20), ui.dot());
    for (key, t) in [
        (None, Tab::Show),
        (Some('h'), Tab::Handoff),
        (Some('a'), Tab::Asks),
        (Some('f'), Tab::Forgotten),
        (Some('r'), Tab::Receipt),
    ] {
        let on = t == tab;
        let label = match key {
            Some(k) => format!("{}:{}", k, t.name()),
            None => t.name().to_string(),
        };
        out.push_str(&format!(
            " {}{}{}",
            if on { "[" } else { " " },
            label,
            if on { "]" } else { " " }
        ));
    }
    out
}

// -----------------------------------------------------------------------------
// row spans
// -----------------------------------------------------------------------------

/// Where a list view's selectable rows sit among its rendered lines.
///
/// Every table in `views.rs` draws the same way: a header, a blank, a column head, a rule,
/// then exactly one line per row. So the rows begin one line after the first rule, and run
/// for as many lines as there are rows. Finding the rule rather than counting header lines
/// is what keeps this from breaking when a header gains a line;
/// `row_span_lands_on_the_session_ids` in the tests below is what keeps it honest if a
/// table ever stops drawing that way.
pub fn row_span(lines: &[String], rows: usize) -> Option<(usize, usize)> {
    if rows == 0 {
        return None;
    }
    let first_rule = lines.iter().position(|l| is_rule(l))?;
    let start = first_rule + 1;
    if start + rows > lines.len() {
        return None;
    }
    Some((start, rows))
}

/// A rule is the indented run of `─` (or `-` in the ASCII set) that `Ui::rule_of` draws,
/// and nothing else on the line.
fn is_rule(line: &str) -> bool {
    let bare = console::strip_ansi_codes(line);
    let bare = bare.trim();
    bare.len() >= 8 && bare.chars().all(|c| c == '─' || c == '-')
}

// -----------------------------------------------------------------------------
// ANSI -> ratatui
// -----------------------------------------------------------------------------

/// Turn one already-rendered line into a styled `Line`.
///
/// `Ui::paint` is the only thing in this binary that adds escapes, and it emits exactly
/// one shape: `ESC [ params m` ... `ESC [ 0 m`, with params drawn from `Role::codes`. So
/// this parses that shape rather than pulling in a general terminal emulator. Anything it
/// does not recognise is dropped, which loses a colour and never a column.
pub fn to_line(raw: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default();
    let mut text = String::new();
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            text.push(c);
            continue;
        }
        if chars.peek() != Some(&'[') {
            continue;
        }
        chars.next();
        let mut params = String::new();
        for c in chars.by_ref() {
            if c == 'm' {
                break;
            }
            params.push(c);
        }
        if !text.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut text), style));
        }
        style = sgr_style(&params);
    }
    if !text.is_empty() {
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

/// `38;2;r;g;b`, `1`, and the ANSI-16 params `Role::codes` uses. `0` resets.
fn sgr_style(params: &str) -> Style {
    let mut style = Style::default();
    let parts: Vec<&str> = params.split(';').collect();
    let mut i = 0;
    while i < parts.len() {
        match parts[i] {
            "0" | "" => style = Style::default(),
            "1" => style = style.add_modifier(Modifier::BOLD),
            "38" if parts.get(i + 1) == Some(&"2") && parts.len() >= i + 5 => {
                let rgb: Vec<u8> = parts[i + 2..i + 5]
                    .iter()
                    .map(|p| p.parse::<u8>().unwrap_or(0))
                    .collect();
                style = style.fg(Color::Rgb(rgb[0], rgb[1], rgb[2]));
                i += 4;
            }
            "30" => style = style.fg(Color::Black),
            "31" => style = style.fg(Color::Red),
            "32" => style = style.fg(Color::Green),
            "33" => style = style.fg(Color::Yellow),
            "34" => style = style.fg(Color::Blue),
            "35" => style = style.fg(Color::Magenta),
            "36" => style = style.fg(Color::Cyan),
            "37" => style = style.fg(Color::Gray),
            "39" => style = style.fg(Color::Reset),
            "90" => style = style.fg(Color::DarkGray),
            _ => {}
        }
        i += 1;
    }
    style
}

// -----------------------------------------------------------------------------
// the runner
// -----------------------------------------------------------------------------

/// Should a bare `archie` take the terminal over?
///
/// The spec's answer: only on a real terminal, and never under `--plain`, `TERM=dumb`, or
/// a JSON default. Everything else prints the overview and exits 0.
pub fn should_open(ui: &Ui, json: bool) -> bool {
    if json || ui.ascii() {
        return false;
    }
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return false;
    }
    !matches!(std::env::var("TERM").as_deref(), Ok("dumb") | Ok(""))
}

/// Everything the runner needs to fetch a screen. Implemented in `app.rs`, where the
/// commands' own builders live, so the cockpit never learns a query of its own.
pub trait Screens {
    fn overview(&self) -> Result<String>;
    /// The rendered list, and the session id behind each row, in row order.
    fn sessions(&self, filter: &str) -> Result<(String, Vec<String>)>;
    fn session(&self, id: &str, tab: Tab) -> Result<String>;
    fn agents(&self) -> Result<String>;
    /// The rendered list, and the repository behind each row.
    fn repos(&self, filter: &str) -> Result<(String, Vec<String>)>;
    fn windows(&self) -> Result<String>;
}

/// One screen, loaded.
struct Loaded {
    lines: Vec<String>,
    ids: Vec<String>,
}

fn load(screens: &dyn Screens, app: &App) -> Loaded {
    let (text, ids) = match app.screen {
        Screen::Overview => (screens.overview(), Vec::new()),
        Screen::Sessions => match screens.sessions(&app.filter) {
            Ok((t, ids)) => (Ok(t), ids),
            Err(e) => (Err(e), Vec::new()),
        },
        Screen::Session => (
            match app.open_session.as_deref() {
                Some(id) => screens.session(id, app.tab),
                None => Ok(String::new()),
            },
            Vec::new(),
        ),
        Screen::Agents => (screens.agents(), Vec::new()),
        Screen::Repos => match screens.repos(&app.filter) {
            Ok((t, ids)) => (Ok(t), ids),
            Err(e) => (Err(e), Vec::new()),
        },
        Screen::Windows => (screens.windows(), Vec::new()),
    };
    let lines = match text {
        Ok(t) => t.lines().map(|l| l.to_string()).collect(),
        // A screen that cannot be read says so in place rather than tearing the terminal
        // down; every other screen still works.
        Err(e) => vec![String::new(), format!("  {}", e)],
    };
    Loaded { lines, ids }
}

/// Open the cockpit. Returns when the reader quits.
pub fn run(ui: &Ui, screens: &dyn Screens) -> Result<()> {
    // The status spinner draws straight to stdout, which inside the alternate screen is a
    // stray line over whatever was there. Off for as long as the cockpit owns the terminal.
    super::set_status_quiet(true);
    install_panic_hook();

    let mut terminal = ratatui::try_init()?;
    let result = event_loop(&mut terminal, ui, screens);
    ratatui::restore();
    super::set_status_quiet(false);
    result
}

/// A panic inside the alternate screen leaves the terminal in raw mode with no echo, which
/// looks to the reader like their shell died. Restore first, then let the original hook
/// print the panic exactly as it would have.
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        original(info);
    }));
}

/// The cockpit with one thing to say and nothing to read: the missing-index screen. Any
/// key quits, so a reader is never trapped in a full-screen dead end.
pub fn run_message(text: &str) -> Result<()> {
    install_panic_hook();
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let mut terminal = ratatui::try_init()?;
    let result = (|| -> Result<()> {
        loop {
            terminal.draw(|frame| {
                let area = frame.area();
                let mut rows: Vec<Line<'static>> = lines.iter().map(|l| to_line(l)).collect();
                rows.push(Line::from(""));
                rows.push(Line::from("  any key to quit"));
                Paragraph::new(rows).render(area, frame.buffer_mut());
            })?;
            if let Event::Key(k) = event::read()? {
                if Key::from_crossterm(k).is_some() {
                    return Ok(());
                }
            }
        }
    })();
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    ui: &Ui,
    screens: &dyn Screens,
) -> Result<()> {
    let mut app = App::default();
    let mut loaded = load(screens, &app);
    app.rows = loaded.ids.len();

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            app.viewport = area.height.saturating_sub(2) as usize;
            app.lines = loaded.lines.len();
            app.row_span = row_span(&loaded.lines, app.rows);
            draw(frame.buffer_mut(), area, ui, &app, &loaded.lines);
        })?;

        // Blocking read: the cockpit repaints on a key or a resize and never on a timer,
        // so an open cockpit costs nothing while it sits there.
        let ev = event::read()?;
        let key = match ev {
            Event::Key(k) => match Key::from_crossterm(k) {
                Some(k) => k,
                None => continue,
            },
            Event::Resize(..) => continue,
            _ => continue,
        };

        match app.on_key(key) {
            Cmd::Quit => return Ok(()),
            Cmd::None => {}
            Cmd::Reload => {
                loaded = load(screens, &app);
                app.rows = loaded.ids.len();
                app.cursor = app.cursor.min(app.rows.saturating_sub(1));
            }
            Cmd::OpenRow(row) => {
                if let Some(id) = loaded.ids.get(row).cloned() {
                    app.open_session = Some(id);
                    app.screen = Screen::Session;
                    app.scroll = 0;
                    loaded = load(screens, &app);
                    app.rows = 0;
                }
            }
        }
    }
}

fn draw(
    buf: &mut ratatui::buffer::Buffer,
    area: Rect,
    ui: &Ui,
    app: &App,
    lines: &[String],
) {
    if area.height < 3 || area.width < 20 {
        return;
    }
    let status = if app.filtering {
        status_line(ui, app.screen, app.tab, &format!("{}_", app.filter), app.cursor, app.rows)
    } else {
        status_line(ui, app.screen, app.tab, &app.filter, app.cursor, app.rows)
    };
    let head = Rect::new(area.x, area.y, area.width, 1);
    Paragraph::new(Line::from(Span::styled(
        truncate(&status, area.width as usize),
        Style::default().add_modifier(Modifier::REVERSED),
    )))
    .render(head, buf);

    let body_h = area.height.saturating_sub(2);
    let body = Rect::new(area.x, area.y + 1, area.width, body_h);

    if app.help {
        let mut rows: Vec<Line<'static>> = vec![Line::from(""), Line::from("  Keys")];
        for (k, v) in HELP_LINES {
            rows.push(Line::from(format!("    {:<18}{}", k, v)));
        }
        rows.push(Line::from(""));
        rows.push(Line::from("  any key to close"));
        Paragraph::new(rows).render(body, buf);
    } else {
        let mut rows: Vec<Line<'static>> = Vec::new();
        if app.screen == Screen::Session {
            if let Some(id) = app.open_session.as_deref() {
                rows.push(Line::from(Span::styled(
                    tab_bar(ui, app.tab, id),
                    Style::default().add_modifier(Modifier::BOLD),
                )));
            }
        }
        let take = body_h as usize - rows.len().min(body_h as usize);
        let selected = app.row_span.map(|(start, _)| start + app.cursor);
        for (i, raw) in lines.iter().skip(app.scroll).take(take).enumerate() {
            let absolute = app.scroll + i;
            let mut line = to_line(raw);
            if Some(absolute) == selected {
                line = line.style(Style::default().add_modifier(Modifier::REVERSED));
            }
            rows.push(line);
        }
        Paragraph::new(rows).render(body, buf);
    }

    let foot = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    Paragraph::new(Line::from(Span::styled(
        truncate(KEY_LEGEND, area.width as usize),
        Style::default().add_modifier(Modifier::DIM),
    )))
    .render(foot, buf);
}

// -----------------------------------------------------------------------------
// tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::ColorMode;

    fn app_with(screen: Screen, rows: usize) -> App {
        App {
            screen,
            rows,
            lines: 100,
            viewport: 10,
            row_span: Some((5, rows)),
            ..App::default()
        }
    }

    #[test]
    fn j_and_k_and_the_arrows_are_the_same_key() {
        let mut a = app_with(Screen::Sessions, 5);
        a.on_key(Key::Char('j'));
        assert_eq!(a.cursor, 1);
        a.on_key(Key::Down);
        assert_eq!(a.cursor, 2);
        a.on_key(Key::Char('k'));
        assert_eq!(a.cursor, 1);
        a.on_key(Key::Up);
        assert_eq!(a.cursor, 0);
    }

    #[test]
    fn the_cursor_stops_at_both_ends() {
        let mut a = app_with(Screen::Sessions, 3);
        for _ in 0..10 {
            a.on_key(Key::Char('j'));
        }
        assert_eq!(a.cursor, 2);
        for _ in 0..10 {
            a.on_key(Key::Char('k'));
        }
        assert_eq!(a.cursor, 0);
    }

    #[test]
    fn the_scroll_follows_the_cursor_out_of_the_viewport() {
        let mut a = app_with(Screen::Sessions, 40);
        for _ in 0..20 {
            a.on_key(Key::Char('j'));
        }
        assert_eq!(a.cursor, 20);
        // Row 20 sits on body line 25; a 10-row viewport has to have moved.
        assert!(a.scroll > 0, "the scroll never followed the cursor");
        assert!(a.scroll <= 25 && 25 < a.scroll + a.viewport);
    }

    #[test]
    fn a_body_screen_scrolls_instead_of_moving_a_cursor() {
        let mut a = App {
            screen: Screen::Overview,
            lines: 60,
            viewport: 10,
            ..App::default()
        };
        a.on_key(Key::Char('j'));
        assert_eq!((a.cursor, a.scroll), (0, 1));
        a.on_key(Key::End);
        assert_eq!(a.scroll, 50);
        a.on_key(Key::Home);
        assert_eq!(a.scroll, 0);
    }

    #[test]
    fn enter_drills_into_the_highlighted_row() {
        let mut a = app_with(Screen::Sessions, 5);
        a.on_key(Key::Char('j'));
        a.on_key(Key::Char('j'));
        assert_eq!(a.on_key(Key::Enter), Cmd::OpenRow(2));
    }

    #[test]
    fn enter_on_an_empty_list_does_nothing() {
        let mut a = app_with(Screen::Sessions, 0);
        assert_eq!(a.on_key(Key::Enter), Cmd::None);
    }

    #[test]
    fn the_number_keys_reach_every_screen_in_the_spec_order() {
        let mut a = App::default();
        for (n, want) in [
            ('2', Screen::Sessions),
            ('4', Screen::Agents),
            ('5', Screen::Repos),
            ('6', Screen::Windows),
            ('1', Screen::Overview),
        ] {
            a.on_key(Key::Char(n));
            assert_eq!(a.screen, want, "key {}", n);
        }
    }

    #[test]
    fn three_needs_a_session_and_takes_the_highlighted_one() {
        let mut a = App::default();
        assert_eq!(a.on_key(Key::Char('3')), Cmd::None);
        assert_eq!(a.screen, Screen::Overview);

        let mut b = app_with(Screen::Sessions, 4);
        b.cursor = 1;
        assert_eq!(b.on_key(Key::Char('3')), Cmd::OpenRow(1));
    }

    #[test]
    fn h_a_f_r_pick_a_reading_of_the_open_session() {
        let mut a = App {
            screen: Screen::Session,
            open_session: Some("s1".into()),
            ..App::default()
        };
        for (key, want) in [
            ('h', Tab::Handoff),
            ('a', Tab::Asks),
            ('f', Tab::Forgotten),
            ('r', Tab::Receipt),
        ] {
            assert_eq!(a.on_key(Key::Char(key)), Cmd::Reload);
            assert_eq!(a.tab, want, "key {}", key);
        }
    }

    #[test]
    fn h_from_the_list_opens_the_highlighted_session_on_its_handoff() {
        let mut a = app_with(Screen::Sessions, 3);
        a.cursor = 2;
        assert_eq!(a.on_key(Key::Char('h')), Cmd::OpenRow(2));
        assert_eq!(a.tab, Tab::Handoff);
    }

    #[test]
    fn slash_takes_the_keyboard_and_esc_gives_it_back() {
        let mut a = app_with(Screen::Sessions, 5);
        a.on_key(Key::Char('/'));
        assert!(a.filtering);
        // 'q' is text while filtering, not a quit.
        for c in "qjk".chars() {
            assert_eq!(a.on_key(Key::Char(c)), Cmd::Reload);
        }
        assert_eq!(a.filter, "qjk");
        a.on_key(Key::Backspace);
        assert_eq!(a.filter, "qj");
        assert_eq!(a.on_key(Key::Esc), Cmd::Reload);
        assert!(!a.filtering);
        assert!(a.filter.is_empty());
    }

    #[test]
    fn enter_leaves_filter_entry_without_clearing_it() {
        let mut a = app_with(Screen::Sessions, 5);
        a.on_key(Key::Char('/'));
        a.on_key(Key::Char('v'));
        a.on_key(Key::Enter);
        assert!(!a.filtering);
        assert_eq!(a.filter, "v");
    }

    #[test]
    fn esc_walks_back_out_of_a_session_then_a_filter_then_to_the_overview() {
        let mut a = App {
            screen: Screen::Session,
            open_session: Some("s1".into()),
            ..App::default()
        };
        // A reading first: `show` has no key of its own, so Esc is how you get back to it.
        a.on_key(Key::Char('f'));
        assert_eq!(a.tab, Tab::Forgotten);
        assert_eq!(a.on_key(Key::Esc), Cmd::Reload);
        assert_eq!(a.tab, Tab::Show);
        assert_eq!(a.screen, Screen::Session);

        assert_eq!(a.on_key(Key::Esc), Cmd::Reload);
        assert_eq!(a.screen, Screen::Sessions);

        a.filter = "vector".into();
        assert_eq!(a.on_key(Key::Esc), Cmd::Reload);
        assert!(a.filter.is_empty());

        assert_eq!(a.on_key(Key::Esc), Cmd::Reload);
        assert_eq!(a.screen, Screen::Overview);
        assert_eq!(a.on_key(Key::Esc), Cmd::None);
    }

    #[test]
    fn question_opens_help_and_the_next_key_closes_it() {
        let mut a = App::default();
        a.on_key(Key::Char('?'));
        assert!(a.help);
        a.on_key(Key::Char('j'));
        assert!(!a.help);
        // The key that closed help is swallowed, not also acted on.
        assert_eq!(a.scroll, 0);
    }

    #[test]
    fn q_quits_and_ctrl_c_is_the_same_key() {
        let mut a = App::default();
        assert_eq!(a.on_key(Key::Char('q')), Cmd::Quit);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(Key::from_crossterm(ctrl_c), Some(Key::Char('q')));
    }

    #[test]
    fn a_released_key_is_not_a_second_press() {
        let mut k = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        k.kind = KeyEventKind::Release;
        assert_eq!(Key::from_crossterm(k), None);
    }

    #[test]
    fn row_span_lands_on_the_session_ids() {
        let ui = Ui::new(80, ColorMode::None, true);
        let rows: Vec<crate::ui::views::TraceRow> = ["alpha_one", "beta_two", "gamma_three"]
            .iter()
            .map(|id| crate::ui::views::TraceRow {
                session_id: (*id).to_string(),
                adapter: "claude_code".into(),
                model: "sonnet".into(),
                score: 42.0,
                rung: 3,
                duration_seconds: Some(90.0),
                total_tokens: 1234,
            })
            .collect();
        let text = crate::ui::views::traces(&ui, "archie session list", 3, &rows);
        let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        let (start, n) = row_span(&lines, rows.len()).expect("no row span");
        assert_eq!(n, 3);
        for (i, r) in rows.iter().enumerate() {
            let head: String = r.session_id.chars().take(4).collect();
            assert!(
                lines[start + i].contains(&head),
                "row {} is not on line {}: {:?}",
                i,
                start + i,
                lines[start + i]
            );
        }
    }

    #[test]
    fn row_span_is_none_when_there_are_no_rows() {
        assert_eq!(row_span(&["a".to_string()], 0), None);
    }

    #[test]
    fn a_painted_line_keeps_every_printing_character() {
        let ui = Ui::new(80, ColorMode::True, false);
        let raw = ui.leaders("cache hit ratio", "91.2%", 60, crate::ui::Role::Verified);
        let line = to_line(&raw);
        let flat: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(flat, console::strip_ansi_codes(&raw));
        assert!(line.spans.len() > 1, "the colours collapsed into one span");
    }

    #[test]
    fn a_plain_line_survives_untouched() {
        let line = to_line("  sessions active .... 12");
        let flat: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(flat, "  sessions active .... 12");
    }

    #[test]
    fn the_status_line_fills_the_content_width() {
        let ui = Ui::new(80, ColorMode::None, true);
        for screen in Screen::ORDER {
            let line = status_line(&ui, screen, Tab::Show, "", 0, 0);
            assert!(
                super::super::display_width(&line) >= ui.width(),
                "{:?}: {:?}",
                screen,
                line
            );
        }
    }
}
