//! Minimal terminal emulator: PTY + shell + keyboard → vte parser → crossterm display.
//!
//! Architecture:
//! - Main thread: crossterm raw mode + alternate screen, event loop (keyboard + resize),
//!   writes input to PTY master, redraws from shared screen buffer on timeout or when dirty.
//! - Reader thread: reads from PTY master, feeds bytes into vte::Parser, which calls our
//!   Perform impl to update the shared screen buffer; then signals redraw.
//! - On resize: update PTY size and clear/redraw.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossterm::{
    cursor,
    event::{
        self,
        Event,
        KeyCode,
        KeyEvent,
        KeyModifiers,
        MouseButton,
        MouseEventKind,
    },
    execute, queue,
    terminal::{self, ClearType},
};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use vte::{Params, Perform, Parser};

// -----------------------------------------------------------------------------
// Theme (user-editable via config file)
// -----------------------------------------------------------------------------

/// Parses a hex color string "#rrggbb" or "rrggbb" into (r, g, b). Returns None if invalid.
fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}

const DEFAULT_FG: (u8, u8, u8) = (0xcc, 0xcc, 0xcc);
const DEFAULT_BG: (u8, u8, u8) = (0x1e, 0x1e, 0x1e);

#[derive(Clone, Debug)]
struct Theme {
    default_foreground: (u8, u8, u8),
    default_background: (u8, u8, u8),
    background_opacity: f32,
    font_size: u8,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            default_foreground: DEFAULT_FG,
            default_background: DEFAULT_BG,
            background_opacity: 0.95,
            font_size: 14,
        }
    }
}

#[derive(serde::Deserialize, Default)]
struct ThemeConfigFile {
    #[serde(rename = "theme")]
    theme: Option<ThemeSection>,
}

#[derive(serde::Deserialize)]
struct ThemeSection {
    default_foreground: Option<String>,
    default_background: Option<String>,
    background_opacity: Option<f32>,
    font_size: Option<u8>,
}

fn load_theme() -> Theme {
    let mut theme = Theme::default();
    let config_path = match directories::ProjectDirs::from("", "", "chameleon") {
        Some(dirs) => dirs.config_dir().join("config.toml"),
        None => return theme,
    };
    let contents = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return theme,
    };
    let file_config: ThemeConfigFile = match toml::from_str(&contents) {
        Ok(c) => c,
        Err(_) => return theme,
    };
    let Some(section) = file_config.theme else {
        return theme;
    };
    if let Some(ref s) = section.default_foreground {
        if let Some(rgb) = parse_hex(s) {
            theme.default_foreground = rgb;
        }
    }
    if let Some(ref s) = section.default_background {
        if let Some(rgb) = parse_hex(s) {
            theme.default_background = rgb;
        }
    }
    if let Some(o) = section.background_opacity {
        theme.background_opacity = o.clamp(0.0, 1.0);
    }
    if let Some(f) = section.font_size {
        theme.font_size = f.min(72).max(6);
    }
    theme
}

/// Returns the config file path for the theme (for opening in editor). May be None.
fn theme_config_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("", "", "chameleon")
        .map(|d| d.config_dir().join("config.toml"))
}

/// Default config content written when the file does not exist.
const DEFAULT_CONFIG: &str = r##"# Chameleon theme — edit and save; press Ctrl+Shift+T to reopen this file.
[theme]
default_foreground = "#cccccc"
default_background = "#1e1e1e"
background_opacity = 0.95
font_size = 14
"##;

/// Open the theme config file in $EDITOR, then reload theme into `theme`. Restores terminal state
/// (alternate screen, raw mode) after the editor exits.
fn open_theme_config_and_reload(
    stdout: &mut io::Stdout,
    theme: &Arc<Mutex<Theme>>,
) -> io::Result<()> {
    let config_path = match theme_config_path() {
        Some(p) => p,
        None => return Ok(()),
    };
    // Restore normal terminal so the editor gets a usable TTY
    execute!(
        stdout,
        event::DisableMouseCapture,
        cursor::Show,
        terminal::LeaveAlternateScreen
    )?;
    stdout.flush()?;
    let _ = terminal::disable_raw_mode();

    // Ensure config dir and default file exist
    if let Some(parent) = config_path.parent() {
        let _ = std::fs::create_dir_all(parent);
        if !config_path.exists() {
            let _ = std::fs::write(&config_path, DEFAULT_CONFIG);
        }
    }

    let editor = std::env::var("EDITOR")
        .unwrap_or_else(|_| std::env::var("VISUAL").unwrap_or_else(|_| "nano".to_string()));
    let parts: Vec<&str> = editor.split_whitespace().collect();
    let (bin, args) = parts
        .split_first()
        .map(|(b, rest)| (*b, rest))
        .unwrap_or(("nano", &[][..]));
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args).arg(&config_path);
    let _ = cmd.status();

    let _ = terminal::enable_raw_mode();
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
    let new_theme = load_theme();
    if let Ok(mut t) = theme.lock() {
        *t = new_theme;
    }
    execute!(stdout, terminal::Clear(ClearType::All))?;
    if let Ok(t) = theme.lock() {
        let (r, g, b) = t.default_background;
        execute!(
            stdout,
            crossterm::style::SetBackgroundColor(crossterm::style::Color::Rgb { r, g, b })
        )?;
    }
    execute!(stdout, event::EnableMouseCapture)?;
    stdout.flush()?;
    Ok(())
}

// -----------------------------------------------------------------------------
// Screen buffer (shared between vte Perform and main thread)
// -----------------------------------------------------------------------------

/// Single cell: character and basic attributes (minimal — no truecolor).
#[derive(Clone, Copy, Debug)]
struct Cell {
    ch: char,
    fg: u8, // 0–7 standard colors (we map to crossterm)
    bg: u8,
    bold: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: 7,
            bg: 0,
            bold: false,
        }
    }
}

/// Terminal screen state: grid + cursor + size. Protected by Mutex for reader thread.
struct Screen {
    /// Grid [row][col]. Row 0 = top.
    grid: Vec<Vec<Cell>>,
    rows: usize,
    cols: usize,
    cursor_row: usize,
    cursor_col: usize,
    /// Current attributes for new characters
    cur_fg: u8,
    cur_bg: u8,
    cur_bold: bool,
}

impl Screen {
    fn new(rows: usize, cols: usize) -> Self {
        let mut grid = Vec::with_capacity(rows);
        for _ in 0..rows {
            grid.push(vec![Cell::default(); cols]);
        }
        Self {
            grid,
            rows,
            cols,
            cursor_row: 0,
            cursor_col: 0,
            cur_fg: 7,
            cur_bg: 0,
            cur_bold: false,
        }
    }

    fn resize(&mut self, rows: usize, cols: usize) {
        self.rows = rows;
        self.cols = cols;
        self.grid.resize(rows, Vec::new());
        for row in &mut self.grid {
            row.resize(cols, Cell::default());
        }
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        if self.cursor_row >= self.rows {
            self.cursor_row = self.rows.saturating_sub(1);
        }
        if self.cursor_col >= self.cols {
            self.cursor_col = self.cols.saturating_sub(1);
        }
    }

    fn put_cell(&mut self, row: usize, col: usize, cell: Cell) {
        if row < self.rows && col < self.cols {
            self.grid[row][col] = cell;
        }
    }

    fn put_char(&mut self, c: char) {
        if self.cursor_row >= self.rows || self.cursor_col >= self.cols {
            return;
        }
        self.grid[self.cursor_row][self.cursor_col] = Cell {
            ch: c,
            fg: self.cur_fg,
            bg: self.cur_bg,
            bold: self.cur_bold,
        };
        self.cursor_col += 1;
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.cursor_row += 1;
            if self.cursor_row >= self.rows {
                self.scroll_up();
                self.cursor_row = self.rows.saturating_sub(1);
            }
        }
    }

    fn scroll_up(&mut self) {
        if self.rows == 0 {
            return;
        }
        self.grid.remove(0);
        self.grid.push(vec![Cell::default(); self.cols]);
    }

    fn scroll_down(&mut self) {
        if self.rows == 0 {
            return;
        }
        self.grid.pop();
        self.grid.insert(0, vec![Cell::default(); self.cols]);
    }

    fn erase_from_cursor_to_end_of_screen(&mut self) {
        for r in self.cursor_row..self.rows {
            for c in 0..self.cols {
                let col_start = if r == self.cursor_row { self.cursor_col } else { 0 };
                if c >= col_start {
                    self.put_cell(r, c, Cell::default());
                }
            }
        }
    }

    fn erase_from_start_to_cursor(&mut self) {
        for r in 0..=self.cursor_row {
            let col_end = if r == self.cursor_row {
                self.cursor_col.saturating_add(1)
            } else {
                self.cols
            };
            for c in 0..col_end {
                self.put_cell(r, c, Cell::default());
            }
        }
    }

    fn erase_entire_screen(&mut self) {
        for row in &mut self.grid {
            for c in row.iter_mut() {
                *c = Cell::default();
            }
        }
    }

    fn erase_from_cursor_to_end_of_line(&mut self) {
        if self.cursor_row < self.rows {
            for c in self.cursor_col..self.cols {
                self.put_cell(self.cursor_row, c, Cell::default());
            }
        }
    }

    fn erase_from_start_to_cursor_in_line(&mut self) {
        if self.cursor_row < self.rows {
            for c in 0..=self.cursor_col {
                self.put_cell(self.cursor_row, c, Cell::default());
            }
        }
    }

    fn erase_entire_line(&mut self) {
        if self.cursor_row < self.rows {
            for c in 0..self.cols {
                self.put_cell(self.cursor_row, c, Cell::default());
            }
        }
    }

}

// -----------------------------------------------------------------------------
// Helper: collect CSI parameters as u16s (vte Params iter yields &[u16] subparameters)
// -----------------------------------------------------------------------------

fn params_to_vec(params: &Params) -> Vec<u16> {
    let mut v = Vec::new();
    for p in params.iter() {
        v.extend_from_slice(p);
    }
    v
}

// -----------------------------------------------------------------------------
// Perform implementation (vte → screen buffer)
// -----------------------------------------------------------------------------

struct TerminalPerform {
    screen: Arc<Mutex<Screen>>,
}

impl Perform for TerminalPerform {
    fn print(&mut self, c: char) {
        if let Ok(mut s) = self.screen.lock() {
            s.put_char(c);
        }
    }

    fn execute(&mut self, byte: u8) {
        if let Ok(mut s) = self.screen.lock() {
            match byte {
                0x07 => {} // BEL - ignore or beep
                0x08 => {
                    // BS
                    s.cursor_col = s.cursor_col.saturating_sub(1);
                }
                0x09 => {
                    // TAB - advance to next multiple of 8
                    s.cursor_col = (s.cursor_col + 8) / 8 * 8;
                    if s.cursor_col >= s.cols {
                        s.cursor_col = 0;
                        s.cursor_row += 1;
                        if s.cursor_row >= s.rows {
                            s.scroll_up();
                            s.cursor_row = s.rows.saturating_sub(1);
                        }
                    }
                }
                0x0a | 0x0b | 0x0c => {
                    // LF, VT, FF
                    s.cursor_row += 1;
                    if s.cursor_row >= s.rows {
                        s.scroll_up();
                        s.cursor_row = s.rows.saturating_sub(1);
                    }
                }
                0x0d => {
                    // CR
                    s.cursor_col = 0;
                }
                _ => {}
            }
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        if let Ok(mut s) = self.screen.lock() {
            let p = params_to_vec(params);
            let default = |i: usize| p.get(i).copied().unwrap_or(1);

            match action {
                'H' | 'f' => {
                    // CUP - cursor position (1-based)
                    let row = default(0).saturating_sub(1) as usize;
                    let col = default(1).saturating_sub(1) as usize;
                    s.cursor_row = row.min(s.rows.saturating_sub(1));
                    s.cursor_col = col.min(s.cols.saturating_sub(1));
                }
                'A' => {
                    // CUU - cursor up
                    s.cursor_row = s.cursor_row.saturating_sub(default(0) as usize);
                }
                'B' => {
                    // CUD - cursor down
                    s.cursor_row = (s.cursor_row + default(0) as usize).min(s.rows.saturating_sub(1));
                }
                'C' => {
                    // CUF - cursor forward
                    s.cursor_col = (s.cursor_col + default(0) as usize).min(s.cols.saturating_sub(1));
                }
                'D' => {
                    // CUB - cursor back
                    s.cursor_col = s.cursor_col.saturating_sub(default(0) as usize);
                }
                'G' => {
                    // CHA - cursor horizontal absolute (1-based)
                    let col = default(0).saturating_sub(1) as usize;
                    s.cursor_col = col.min(s.cols.saturating_sub(1));
                }
                'd' => {
                    // VPA - line position absolute (1-based)
                    let row = default(0).saturating_sub(1) as usize;
                    s.cursor_row = row.min(s.rows.saturating_sub(1));
                }
                'J' => {
                    // ED - erase in display
                    match default(0) {
                        0 => s.erase_from_cursor_to_end_of_screen(),
                        1 => s.erase_from_start_to_cursor(),
                        2 => s.erase_entire_screen(),
                        _ => {}
                    }
                }
                'K' => {
                    // EL - erase in line
                    match default(0) {
                        0 => s.erase_from_cursor_to_end_of_line(),
                        1 => s.erase_from_start_to_cursor_in_line(),
                        2 => s.erase_entire_line(),
                        _ => {}
                    }
                }
                'm' => {
                    // SGR
                    let mut i = 0;
                    while i < p.len() {
                        let code = p[i];
                        match code {
                            0 => {
                                s.cur_fg = 7;
                                s.cur_bg = 0;
                                s.cur_bold = false;
                            }
                            1 => s.cur_bold = true,
                            7 => {} // reverse (skip for minimal)
                            27 => {} // not reverse
                            30..=37 => s.cur_fg = (code - 30) as u8,
                            38 => {
                                // set fg (skip 256/24bit for minimal)
                                if i + 1 < p.len() && p[i + 1] == 5 && i + 2 < p.len() {
                                    s.cur_fg = p[i + 2] as u8 % 8;
                                    i += 2;
                                }
                                i += 1;
                            }
                            39 => s.cur_fg = 7,
                            40..=47 => s.cur_bg = (code - 40) as u8,
                            48 => {
                                if i + 1 < p.len() && p[i + 1] == 5 && i + 2 < p.len() {
                                    s.cur_bg = p[i + 2] as u8 % 8;
                                    i += 2;
                                }
                                i += 1;
                            }
                            49 => s.cur_bg = 0,
                            _ => {}
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        if let Ok(mut s) = self.screen.lock() {
            match byte {
                b'D' => {
                    // IND - index, scroll up / move down
                    s.cursor_row += 1;
                    if s.cursor_row >= s.rows {
                        s.scroll_up();
                        s.cursor_row = s.rows.saturating_sub(1);
                    }
                }
                b'M' => {
                    // RI - reverse index
                    if s.cursor_row > 0 {
                        s.cursor_row -= 1;
                    } else {
                        s.scroll_down();
                    }
                }
                b'E' => {
                    // NEL - next line
                    s.cursor_col = 0;
                    s.cursor_row += 1;
                    if s.cursor_row >= s.rows {
                        s.scroll_up();
                        s.cursor_row = s.rows.saturating_sub(1);
                    }
                }
                b'H' => {
                    // HT
                    s.cursor_col = (s.cursor_col + 8) / 8 * 8;
                    if s.cursor_col >= s.cols {
                        s.cursor_col = 0;
                        s.cursor_row += 1;
                        if s.cursor_row >= s.rows {
                            s.scroll_up();
                            s.cursor_row = s.rows.saturating_sub(1);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

// Fix Screen methods that take &mut self and extra arg (ED 1)
impl Screen {
}

// -----------------------------------------------------------------------------
// Selection (for copy)
// -----------------------------------------------------------------------------

/// Rectangular selection: (top_row, left_col) to (bottom_row, right_col) inclusive.
#[derive(Clone, Copy, Debug, Default)]
struct Selection {
    start_row: usize,
    start_col: usize,
    end_row: usize,
    end_col: usize,
}

impl Selection {
    fn is_empty(&self) -> bool {
        self.start_row == self.end_row && self.start_col == self.end_col
    }

    /// Normalize so we have top-left and bottom-right.
    fn normalized(&self) -> (usize, usize, usize, usize) {
        let (r1, c1) = (self.start_row.min(self.end_row), self.start_col.min(self.end_col));
        let (r2, c2) = (self.start_row.max(self.end_row), self.start_col.max(self.end_col));
        (r1, c1, r2, c2)
    }

    /// Extract selected text from screen (rectangle, line-by-line).
    fn extract_from(&self, screen: &Screen) -> String {
        let (r1, c1, r2, c2) = self.normalized();
        let mut lines = Vec::new();
        for r in r1..=r2 {
            let start_c = if r == r1 { c1 } else { 0 };
            let end_c = if r == r2 { c2 } else { screen.cols.saturating_sub(1) };
            let mut line = String::new();
            if r < screen.grid.len() {
                for c in start_c..=end_c.min(screen.grid[r].len().saturating_sub(1)) {
                    line.push(screen.grid[r][c].ch);
                }
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }
}

// -----------------------------------------------------------------------------
// Render screen buffer to crossterm
// -----------------------------------------------------------------------------

fn render(
    screen: &Screen,
    selection: Option<&Selection>,
    art_height: usize,
    theme: &Theme,
    stdout: &mut io::Stdout,
) -> io::Result<()> {
    let (_term_rows, term_cols) = terminal::size()
        .map(|(c, r)| (r as usize, c as usize))
        .unwrap_or((24, 80));
    let art_line_list = art_lines();
    let art_width = art_line_list.iter().map(|l| l.len()).max().unwrap_or(0);
    let start_col = term_cols.saturating_sub(art_width) / 2;

    // Draw art at top (rows 0..art_height)
    for (i, line) in art_line_list.iter().enumerate() {
        if i < art_height {
            queue!(stdout, cursor::MoveTo(start_col as u16, i as u16), crossterm::style::Print(line))?;
        }
    }

    // Draw terminal grid below the art
    let top_offset = art_height;
    queue!(stdout, cursor::MoveTo(0, top_offset as u16))?;
    let mut last_fg = 255;
    let mut last_bg = 255;
    let mut last_bold = false;
    let sel = selection.and_then(|s| (!s.is_empty()).then(|| s.normalized()));

    for (r, row) in screen.grid.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let in_selection = sel.map(|(r1, c1, r2, c2)| r >= r1 && r <= r2 && c >= c1 && c <= c2).unwrap_or(false);
            let draw_r = top_offset + r;
            let draw_c = c;

            if in_selection {
                queue!(
                    stdout,
                    crossterm::style::SetForegroundColor(crossterm::style::Color::Black),
                    crossterm::style::SetBackgroundColor(crossterm::style::Color::Grey),
                    crossterm::style::SetAttribute(crossterm::style::Attribute::NormalIntensity)
                )?;
                last_fg = 255;
                last_bg = 255;
                last_bold = false;
            } else if cell.fg != last_fg || cell.bg != last_bg || cell.bold != last_bold {
                let fg = match cell.fg {
                    0 => crossterm::style::Color::Black,
                    1 => crossterm::style::Color::DarkRed,
                    2 => crossterm::style::Color::DarkGreen,
                    3 => crossterm::style::Color::DarkYellow,
                    4 => crossterm::style::Color::DarkBlue,
                    5 => crossterm::style::Color::DarkMagenta,
                    6 => crossterm::style::Color::DarkCyan,
                    7 => crossterm::style::Color::Rgb {
                        r: theme.default_foreground.0,
                        g: theme.default_foreground.1,
                        b: theme.default_foreground.2,
                    },
                    _ => crossterm::style::Color::Rgb {
                        r: theme.default_foreground.0,
                        g: theme.default_foreground.1,
                        b: theme.default_foreground.2,
                    },
                };
                let bg = match cell.bg {
                    0 => crossterm::style::Color::Rgb {
                        r: theme.default_background.0,
                        g: theme.default_background.1,
                        b: theme.default_background.2,
                    },
                    1 => crossterm::style::Color::DarkRed,
                    2 => crossterm::style::Color::DarkGreen,
                    3 => crossterm::style::Color::DarkYellow,
                    4 => crossterm::style::Color::DarkBlue,
                    5 => crossterm::style::Color::DarkMagenta,
                    6 => crossterm::style::Color::DarkCyan,
                    7 => crossterm::style::Color::Grey,
                    _ => crossterm::style::Color::Rgb {
                        r: theme.default_background.0,
                        g: theme.default_background.1,
                        b: theme.default_background.2,
                    },
                };
                queue!(
                    stdout,
                    crossterm::style::SetForegroundColor(fg),
                    crossterm::style::SetBackgroundColor(bg)
                )?;
                if cell.bold {
                    queue!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::Bold))?;
                } else if last_bold {
                    queue!(stdout, crossterm::style::SetAttribute(crossterm::style::Attribute::NormalIntensity))?;
                }
                last_fg = cell.fg;
                last_bg = cell.bg;
                last_bold = cell.bold;
            }
            queue!(stdout, cursor::MoveTo(draw_c as u16, draw_r as u16), crossterm::style::Print(cell.ch))?;
        }
    }
    queue!(
        stdout,
        cursor::MoveTo(
            screen.cursor_col as u16,
            (top_offset + screen.cursor_row) as u16
        ),
        cursor::Show
    )?;
    stdout.flush()?;
    Ok(())
}

// -----------------------------------------------------------------------------
// Welcome screen
// -----------------------------------------------------------------------------

const WELCOME_ART: &str = r#"
  ██████╗ ██╗  ██╗ █████╗ ███╗   ███╗███████╗██╗     ███████╗ ██████╗ ███╗   ██╗
 ██╔════╝ ██║  ██║██╔══██╗████╗ ████║██╔════╝██║     ██╔════╝██╔═══██╗████╗  ██║
 ██║  ███╗███████║███████║██╔████╔██║█████╗  ██║     █████╗  ██║   ██║██╔██╗ ██║
 ██║   ██║██╔══██║██╔══██║██║╚██╔╝██║██╔══╝  ██║     ██╔══╝  ██║   ██║██║╚██╗██║
 ╚██████╔╝██║  ██║██║  ██║██║ ╚═╝ ██║███████╗███████╗███████╗╚██████╔╝██║ ╚████║
  ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚═╝╚══════╝╚══════╝╚══════╝ ╚═════╝ ╚═╝  ╚═══╝
"#;

fn art_lines() -> Vec<&'static str> {
    WELCOME_ART.trim_start_matches('\n').lines().collect()
}

// -----------------------------------------------------------------------------
// Main
// -----------------------------------------------------------------------------

fn main() -> io::Result<()> {
    let (term_rows, term_cols) = terminal::size()
        .map(|(c, r)| (r as usize, c as usize))
        .unwrap_or((24, 80));

    let art_height = art_lines().len();
    let rows = term_rows.saturating_sub(art_height).max(1);
    let cols = term_cols;

    let theme = Arc::new(Mutex::new(load_theme()));

    let _guard = RawModeGuard::new()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        terminal::Clear(ClearType::All),
        cursor::Hide
    )?;
    // Set default background from theme so cleared screen and empty cells use it
    if let Ok(t) = theme.lock() {
        let (r, g, b) = t.default_background;
        execute!(
            stdout,
            crossterm::style::SetBackgroundColor(crossterm::style::Color::Rgb { r, g, b })
        )?;
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let cmd = CommandBuilder::new(shell.clone());
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let master = pair.master;
    let mut pty_writer = master
        .take_writer()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let mut pty_reader = master
        .try_clone_reader()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let screen = Arc::new(Mutex::new(Screen::new(rows, cols)));
    let performer = Arc::new(Mutex::new(TerminalPerform {
        screen: Arc::clone(&screen),
    }));
    let running = Arc::new(AtomicBool::new(true));

    let performer_reader = Arc::clone(&performer);
    let running_reader = Arc::clone(&running);
    let reader_handle = thread::spawn(move || {
        let mut parser = Parser::new();
        let mut buf = [0u8; 4096];
        while running_reader.load(Ordering::Relaxed) {
            match pty_reader.read(&mut buf) {
                Ok(0) => {
                    running_reader.store(false, Ordering::Relaxed);
                    break;
                }
                Ok(n) => {
                    for b in &buf[..n] {
                        if let Ok(mut perf) = performer_reader.lock() {
                            parser.advance(&mut *perf, *b);
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    execute!(stdout, event::EnableMouseCapture)?;

    // Selection state: drag updates end; Ctrl+Shift+C copies.
    let mut selection: Option<Selection> = None;
    let mut selecting = false;

    while running.load(Ordering::Relaxed) {
        // Exit when shell exits (Ctrl+D or `exit`)
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        if event::poll(Duration::from_millis(16)).unwrap_or(false) {
            match event::read() {
                Ok(Event::Key(KeyEvent {
                    code,
                    modifiers,
                    ..
                })) => {
                    // Ctrl+Shift+C: copy selection to clipboard (do not send to PTY)
                    if modifiers.contains(KeyModifiers::CONTROL)
                        && modifiers.contains(KeyModifiers::SHIFT)
                        && code == KeyCode::Char('c')
                    {
                        if let Some(ref sel) = selection {
                            if let Ok(s) = screen.lock() {
                                let text = sel.extract_from(&s);
                                if !text.is_empty() {
                                    if let Ok(mut clip) = arboard::Clipboard::new() {
                                        let _ = clip.set_text(&text);
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    // Ctrl+Shift+T: open theme config in $EDITOR and reload theme
                    if modifiers.contains(KeyModifiers::CONTROL)
                        && modifiers.contains(KeyModifiers::SHIFT)
                        && code == KeyCode::Char('t')
                    {
                        let _ = open_theme_config_and_reload(&mut stdout, &theme);
                        continue;
                    }

                    let bytes = key_to_bytes(code, modifiers);
                    for b in bytes {
                        let _ = pty_writer.write_all(&[b]);
                    }
                    let _ = pty_writer.flush();
                }
                Ok(Event::Mouse(me)) => {
                    // Terminal row 0..art_height is the banner; grid starts at art_height
                    let row = me.row.saturating_sub(art_height as u16) as usize;
                    let col = me.column as usize;
                    match me.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            selecting = true;
                            selection = Some(Selection {
                                start_row: row,
                                start_col: col,
                                end_row: row,
                                end_col: col,
                            });
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            if selecting {
                                if let Some(ref mut sel) = selection {
                                    sel.end_row = row;
                                    sel.end_col = col;
                                }
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            selecting = false;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Resize(c, r)) => {
                    let new_term_rows = r as usize;
                    let new_term_cols = c as usize;
                    let new_rows = new_term_rows.saturating_sub(art_height).max(1);
                    let new_cols = new_term_cols;
                    let _ = master.resize(PtySize {
                        rows: new_rows as u16,
                        cols: c,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                    if let Ok(mut s) = screen.lock() {
                        s.resize(new_rows, new_cols);
                    }
                    selection = None;
                }
                _ => {}
            }
        }

        // Redraw every frame: art at top, then terminal grid below
        if let (Ok(s), Ok(t)) = (screen.lock(), theme.lock()) {
            let _ = render(&s, selection.as_ref(), art_height, &*t, &mut stdout);
        }
    }

    running.store(false, Ordering::Relaxed);
    let _ = reader_handle.join();

    execute!(
        stdout,
        event::DisableMouseCapture,
        cursor::Show,
        terminal::LeaveAlternateScreen
    )?;
    Ok(())
}

/// Restore terminal on drop.
struct RawModeGuard;

impl RawModeGuard {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

fn key_to_bytes(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    match code {
        KeyCode::Char('c') if ctrl => vec![0x03],
        KeyCode::Char('z') if ctrl => vec![0x1a],
        KeyCode::Char('d') if ctrl => vec![0x04],
        KeyCode::Char('\\') if ctrl => vec![0x1c],
        KeyCode::Char(c) if ctrl && c >= 'a' && c <= 'z' => {
            vec![(c as u8) - b'a' + 1]
        }
        KeyCode::Char(c) if ctrl && c >= '@' && c <= '_' => {
            vec![(c as u8) - b'@']
        }
        KeyCode::Char(c) => vec![c as u8],
        KeyCode::Enter => vec![b'\r'],
        KeyCode::BackTab => vec![0x1b, b'[', b'Z'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => vec![0x1b, b'[', b'A'],
        KeyCode::Down => vec![0x1b, b'[', b'B'],
        KeyCode::Right => vec![0x1b, b'[', b'C'],
        KeyCode::Left => vec![0x1b, b'[', b'D'],
        KeyCode::Home => vec![0x1b, b'[', b'H'],
        KeyCode::End => vec![0x1b, b'[', b'F'],
        KeyCode::PageUp => vec![0x1b, b'[', b'5', b'~'],
        KeyCode::PageDown => vec![0x1b, b'[', b'6', b'~'],
        KeyCode::Delete => vec![0x1b, b'[', b'3', b'~'],
        KeyCode::Insert => vec![0x1b, b'[', b'2', b'~'],
        _ => vec![],
    }
}
