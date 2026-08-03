//! `--walk`: browse a site or a directory, read each page, pick what to keep.
//!
//! The problem this solves: `--horde` on a website converts everything it can
//! reach, so you get the changelog, the 2019 blog archive and every language
//! variant alongside the four pages you wanted. You cannot know what is worth
//! converting until you have seen it, and by then you have already converted it
//! all.
//!
//! Navigation state lives in [`WalkState`] and is pure — every key produces a
//! state transition that can be tested without a terminal. Drawing is a
//! separate function that only reads that state. The engine underneath
//! (`filegoblin_core::explore`) decides what a node renders as and where it
//! leads; this file is only the screen.
//!
//! Output discipline: the UI draws to **stderr** and stdout carries only the
//! converted markdown, so `filegoblin --walk <target> > context.md` behaves
//! exactly like every other invocation.

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::{cursor, execute, terminal};
use filegoblin_core::cli::Cli;
use filegoblin_core::explore::{self, Child, Node, Preview};
use std::io::{IsTerminal, Write};

/// What a keypress means. Named so tests read as intent rather than key codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    ScrollUp,
    ScrollDown,
    Toggle,
    Follow,
    Back,
    Done,
    Quit,
}

/// One level of the walk: where we are, how it renders, and where it leads.
struct Frame {
    node: Node,
    preview: Preview,
    children: Vec<Child>,
    cursor: usize,
    scroll: usize,
}

/// The walk's navigation and selection state.
pub struct WalkState {
    stack: Vec<Frame>,
    /// Chosen targets, in the order they were picked. A `Vec` rather than a set
    /// because the output order should match the order the user built it in.
    selected: Vec<String>,
    finished: bool,
    cancelled: bool,
}

impl WalkState {
    fn new(root: Node, args: &Cli) -> Self {
        let frame = Self::load(root, args);
        Self {
            stack: vec![frame],
            selected: Vec::new(),
            finished: false,
            cancelled: false,
        }
    }

    fn load(node: Node, args: &Cli) -> Frame {
        let preview = explore::preview(&node, args);
        let children = explore::children(&node, args);
        Frame {
            node,
            preview,
            children,
            cursor: 0,
            scroll: 0,
        }
    }

    fn frame(&self) -> &Frame {
        self.stack.last().expect("the stack is never empty")
    }

    fn frame_mut(&mut self) -> &mut Frame {
        self.stack.last_mut().expect("the stack is never empty")
    }

    /// Rows in the picker: index 0 is always the page you are on, so `space`
    /// means the same thing everywhere and there is no separate "keep this one"
    /// key to discover.
    fn row_count(&self) -> usize {
        self.frame().children.len() + 1
    }

    /// The node a row refers to, or `None` for a row that does not exist.
    fn row_node(&self, row: usize) -> Option<&Node> {
        if row == 0 {
            Some(&self.frame().node)
        } else {
            self.frame().children.get(row - 1).map(|c| &c.node)
        }
    }

    pub fn is_selected(&self, node: &Node) -> bool {
        self.selected.iter().any(|t| t == &node.target())
    }

    #[cfg(test)]
    pub fn selected(&self) -> &[String] {
        &self.selected
    }

    /// Applies one action. `args` is needed because following a link loads a
    /// new frame, which converts.
    pub fn apply(&mut self, action: Action, args: &Cli) {
        match action {
            Action::Up => {
                let f = self.frame_mut();
                f.cursor = f.cursor.saturating_sub(1);
            }
            Action::Down => {
                let max = self.row_count().saturating_sub(1);
                let f = self.frame_mut();
                f.cursor = (f.cursor + 1).min(max);
            }
            Action::ScrollUp => {
                let f = self.frame_mut();
                f.scroll = f.scroll.saturating_sub(3);
            }
            Action::ScrollDown => {
                // Clamped against the body length when drawing; over-scrolling
                // here would just stick at the bottom.
                let f = self.frame_mut();
                f.scroll += 3;
            }
            Action::Toggle => {
                let Some(node) = self.row_node(self.frame().cursor).cloned() else {
                    return;
                };
                let target = node.target();
                if let Some(i) = self.selected.iter().position(|t| t == &target) {
                    self.selected.remove(i);
                } else {
                    self.selected.push(target);
                }
            }
            Action::Follow => {
                let cursor = self.frame().cursor;
                // Row 0 is the current page; there is nowhere to follow to.
                if cursor == 0 {
                    return;
                }
                let Some(node) = self.row_node(cursor).cloned() else {
                    return;
                };
                self.stack.push(Self::load(node, args));
            }
            Action::Back => {
                // Never pop the root: an empty stack has no state to draw.
                if self.stack.len() > 1 {
                    self.stack.pop();
                }
            }
            Action::Done => self.finished = true,
            Action::Quit => {
                self.cancelled = true;
                self.finished = true;
            }
        }
    }
}

/// Maps a key to an action, or `None` for keys we ignore.
fn action_for(code: KeyCode, mods: KeyModifiers) -> Option<Action> {
    if mods.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('c')) {
        return Some(Action::Quit);
    }
    Some(match code {
        KeyCode::Up | KeyCode::Char('k') => Action::Up,
        KeyCode::Down | KeyCode::Char('j') => Action::Down,
        KeyCode::PageUp => Action::ScrollUp,
        KeyCode::PageDown => Action::ScrollDown,
        KeyCode::Char(' ') => Action::Toggle,
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => Action::Follow,
        KeyCode::Backspace | KeyCode::Left | KeyCode::Char('b') | KeyCode::Char('h') => {
            Action::Back
        }
        KeyCode::Char('d') => Action::Done,
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        _ => return None,
    })
}

/// Restores the terminal however we leave — including on a panic, which would
/// otherwise strand the user in raw mode with no echo.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode().context("Failed to put the terminal in raw mode")?;
        execute!(
            std::io::stderr(),
            terminal::EnterAlternateScreen,
            cursor::Hide
        )?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            std::io::stderr(),
            cursor::Show,
            terminal::LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

/// Runs the picker and returns the chosen targets, or `None` if the user quit
/// without choosing.
pub fn run(root: &str, args: &Cli) -> Result<Option<Vec<String>>> {
    if !std::io::stderr().is_terminal() || !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "--walk needs a terminal (it draws an interactive picker on stderr).\n\
             For non-interactive use, drop --walk and pass the targets you want directly."
        );
    }

    let node = if root.starts_with("http://") || root.starts_with("https://") {
        Node::Url(root.to_string())
    } else {
        Node::Path(std::path::PathBuf::from(root))
    };

    let _guard = TerminalGuard::enter()?;
    let mut state = WalkState::new(node, args);

    loop {
        draw(&state)?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        // Windows reports press *and* release; acting on both double-steps.
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if let Some(action) = action_for(key.code, key.modifiers) {
            // A follow can take a moment (fetch + convert), so say so rather
            // than looking frozen.
            if action == Action::Follow && state.frame().cursor > 0 {
                let _ = status_line("Loading…");
            }
            state.apply(action, args);
        }
        if state.finished {
            break;
        }
    }

    if state.cancelled || state.selected.is_empty() {
        return Ok(None);
    }
    Ok(Some(state.selected.clone()))
}

/// Prints a transient message on the last row (used while loading).
fn status_line(msg: &str) -> Result<()> {
    let (_, h) = terminal::size()?;
    let mut err = std::io::stderr();
    execute!(err, cursor::MoveTo(0, h.saturating_sub(1)))?;
    write!(err, "\x1b[7m {:<20}\x1b[0m", msg)?;
    err.flush()?;
    Ok(())
}

fn draw(state: &WalkState) -> Result<()> {
    let (w, h) = terminal::size().unwrap_or((80, 24));
    let (w, h) = (w as usize, h as usize);
    let mut out = std::io::stderr();

    execute!(
        out,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    let frame = state.frame();
    // Split the screen: preview on top, picker below, one line of keys.
    let list_rows = (frame.children.len() + 1).min(h.saturating_sub(8)).max(3);
    let preview_rows = h.saturating_sub(list_rows + 4);

    // ---- header -----------------------------------------------------------
    let tokens = if frame.preview.tokens > 0 {
        format!("  {} tok", human_tokens(frame.preview.tokens))
    } else {
        String::new()
    };
    let depth = if state.stack.len() > 1 {
        format!("  ↳{}", state.stack.len() - 1)
    } else {
        String::new()
    };
    writeln_row(
        &mut out,
        &format!(
            "\x1b[7m {} {}{}\x1b[0m",
            truncate(
                &frame.node.display(),
                w.saturating_sub(tokens.len() + depth.len() + 3)
            ),
            tokens,
            depth
        ),
    )?;

    // ---- preview ----------------------------------------------------------
    let body = if let Some(err) = &frame.preview.error {
        format!("⚠️  Could not convert this one\n\n{}", err)
    } else {
        frame.preview.body.clone()
    };
    let lines: Vec<&str> = body.lines().collect();
    let max_scroll = lines.len().saturating_sub(preview_rows);
    let scroll = frame.scroll.min(max_scroll);
    for i in 0..preview_rows {
        let line = lines.get(scroll + i).copied().unwrap_or("");
        writeln_row(&mut out, &truncate(line, w.saturating_sub(1)))?;
    }

    // ---- picker -----------------------------------------------------------
    let more = if max_scroll > 0 {
        format!(" (PgUp/PgDn · {}/{})", scroll, max_scroll)
    } else {
        String::new()
    };
    writeln_row(
        &mut out,
        &format!(
            "\x1b[2m─ pick{} {}\x1b[0m",
            more,
            "─".repeat(w.saturating_sub(12 + more.len()))
        ),
    )?;

    // Keep the cursor row on screen when the list is longer than the space.
    let first = frame.cursor.saturating_sub(list_rows.saturating_sub(1));
    for row in first..(first + list_rows).min(state.row_count()) {
        let node = state.row_node(row).expect("row in range");
        let mark = if state.is_selected(node) {
            "◉"
        } else {
            "○"
        };
        let label = if row == 0 {
            format!("this page — {}", frame.preview.title)
        } else {
            let child = &frame.children[row - 1];
            let arrow = if child.node.is_dir() { "▸ " } else { "" };
            format!("{}{}", arrow, child.label)
        };
        let pointer = if row == frame.cursor { "❯" } else { " " };
        let line = format!(
            "{} {} {}",
            pointer,
            mark,
            truncate(&label, w.saturating_sub(6))
        );
        if row == frame.cursor {
            writeln_row(&mut out, &format!("\x1b[1m{}\x1b[0m", line))?;
        } else {
            writeln_row(&mut out, &line)?;
        }
    }

    // ---- keys -------------------------------------------------------------
    execute!(out, cursor::MoveTo(0, h.saturating_sub(1) as u16))?;
    write!(
        out,
        "\x1b[7m {} selected \x1b[0m \x1b[2m↑↓ move · space keep · ⏎ follow · b back · d done · q quit\x1b[0m",
        state.selected.len()
    )?;
    out.flush()?;
    Ok(())
}

fn writeln_row(out: &mut impl Write, s: &str) -> std::io::Result<()> {
    write!(out, "{}\r\n", s)
}

/// Truncates to a display width, counting characters rather than bytes so a
/// multi-byte title cannot slice mid-character.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1);
    s.chars().take(keep).collect::<String>() + "…"
}

fn human_tokens(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn args() -> Cli {
        Cli::parse_from(["filegoblin", "dummy"])
    }

    /// A directory tree gives the walk something to navigate without a network.
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("keep.md"), "# Keep\n\nWanted content.").unwrap();
        std::fs::write(dir.path().join("junk.md"), "# Junk\n\nUnwanted content.").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/deep.md"), "# Deep\n\nNested content.").unwrap();
        dir
    }

    fn state(dir: &tempfile::TempDir) -> WalkState {
        WalkState::new(Node::Path(dir.path().to_path_buf()), &args())
    }

    #[test]
    fn cursor_moves_within_bounds() {
        let dir = fixture();
        let mut s = state(&dir);
        // Row 0 is "this page", then sub/, junk.md, keep.md.
        assert_eq!(s.row_count(), 4);

        s.apply(Action::Up, &args());
        assert_eq!(s.frame().cursor, 0, "cannot move above the first row");

        for _ in 0..10 {
            s.apply(Action::Down, &args());
        }
        assert_eq!(s.frame().cursor, 3, "cannot move past the last row");
    }

    #[test]
    fn space_toggles_the_highlighted_row_including_the_current_page() {
        let dir = fixture();
        let mut s = state(&dir);

        // Row 0: the directory itself.
        s.apply(Action::Toggle, &args());
        assert_eq!(s.selected().len(), 1);
        assert!(s.selected()[0].ends_with(dir.path().file_name().unwrap().to_str().unwrap()));

        // Toggling again removes it.
        s.apply(Action::Toggle, &args());
        assert!(s.selected().is_empty());

        // A child row selects that child.
        s.apply(Action::Down, &args());
        s.apply(Action::Toggle, &args());
        assert_eq!(s.selected().len(), 1);
    }

    #[test]
    fn following_pushes_a_frame_and_back_pops_it() {
        let dir = fixture();
        let mut s = state(&dir);
        assert_eq!(s.stack.len(), 1);

        // Row 1 is `sub/` (directories sort first).
        s.apply(Action::Down, &args());
        s.apply(Action::Follow, &args());
        assert_eq!(s.stack.len(), 2, "followed into the subdirectory");
        assert!(s.frame().node.target().ends_with("sub"));

        s.apply(Action::Back, &args());
        assert_eq!(s.stack.len(), 1);
    }

    #[test]
    fn back_at_the_root_is_a_no_op_rather_than_an_empty_stack() {
        let dir = fixture();
        let mut s = state(&dir);
        for _ in 0..5 {
            s.apply(Action::Back, &args());
        }
        assert_eq!(s.stack.len(), 1, "the root frame must survive");
    }

    #[test]
    fn follow_on_the_current_page_row_does_nothing() {
        let dir = fixture();
        let mut s = state(&dir);
        s.apply(Action::Follow, &args());
        assert_eq!(s.stack.len(), 1, "row 0 has nowhere to follow to");
    }

    /// Selections survive navigation — the whole point is building a set while
    /// moving around.
    #[test]
    fn selections_persist_across_frames() {
        let dir = fixture();
        let mut s = state(&dir);

        s.apply(Action::Down, &args()); // sub/
        s.apply(Action::Follow, &args());
        s.apply(Action::Toggle, &args()); // keep sub/ itself
        s.apply(Action::Back, &args());

        assert_eq!(s.selected().len(), 1);
        assert!(s.selected()[0].ends_with("sub"));
    }

    #[test]
    fn quit_discards_the_selection_but_done_keeps_it() {
        let dir = fixture();

        let mut s = state(&dir);
        s.apply(Action::Toggle, &args());
        s.apply(Action::Quit, &args());
        assert!(s.finished && s.cancelled);

        let mut s = state(&dir);
        s.apply(Action::Toggle, &args());
        s.apply(Action::Done, &args());
        assert!(s.finished && !s.cancelled);
        assert_eq!(s.selected().len(), 1);
    }

    #[test]
    fn keys_map_to_the_actions_the_footer_advertises() {
        let n = KeyModifiers::NONE;
        assert_eq!(action_for(KeyCode::Char(' '), n), Some(Action::Toggle));
        assert_eq!(action_for(KeyCode::Enter, n), Some(Action::Follow));
        assert_eq!(action_for(KeyCode::Char('b'), n), Some(Action::Back));
        assert_eq!(action_for(KeyCode::Char('d'), n), Some(Action::Done));
        assert_eq!(action_for(KeyCode::Char('q'), n), Some(Action::Quit));
        assert_eq!(action_for(KeyCode::Esc, n), Some(Action::Quit));
        // vim keys, because this is a terminal
        assert_eq!(action_for(KeyCode::Char('j'), n), Some(Action::Down));
        assert_eq!(action_for(KeyCode::Char('k'), n), Some(Action::Up));
        // Ctrl-C must escape even though 'c' alone is unbound.
        assert_eq!(
            action_for(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Some(Action::Quit)
        );
        assert_eq!(action_for(KeyCode::Char('z'), n), None);
    }

    #[test]
    fn truncation_never_splits_a_multibyte_character() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello w…");
        // Would panic on a byte slice.
        let wide = "日本語のタイトルです";
        assert_eq!(truncate(wide, 4).chars().count(), 4);
    }
}
