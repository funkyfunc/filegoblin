use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    Terminal,
};
use std::{io, path::PathBuf};

/// The internal state of our TUI Application.
pub struct App<'a> {
    pub files: Vec<PathBuf>,
    pub selected_index: usize,
    pub selected_files: std::collections::HashSet<usize>,
    pub preview_content: String,
    pub view_scroll: u16,
    pub should_quit: bool,
    pub should_execute: bool,
    pub active_flags: &'a mut crate::cli::Cli,
}

impl<'a> App<'a> {
    pub fn new(flags: &'a mut crate::cli::Cli) -> Self {
        Self {
            files: Vec::new(),
            selected_index: 0,
            selected_files: std::collections::HashSet::new(),
            preview_content: "Loading...".to_string(),
            view_scroll: 0,
            should_quit: false,
            should_execute: false,
            active_flags: flags,
        }
    }

    /// Sniffs (reads) the target path and populates the initial Hoard.
    pub fn sniff(&mut self) -> Result<()> {
        self.files.clear();
        let path_str = self.active_flags.path.as_deref().unwrap_or("");
        let path = std::path::Path::new(path_str);
        
        if !path.exists() {
            return Ok(());
        }

        if path.is_file() {
            self.files.push(path.to_path_buf());
        } else {
            for entry in ignore::WalkBuilder::new(path).build().flatten() {
                if entry.file_type().is_some_and(|ft| ft.is_file()) {
                    self.files.push(entry.into_path());
                }
            }
        }
        
        self.update_preview();
        Ok(())
    }

    pub fn next(&mut self) {
        if !self.files.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.files.len();
            self.view_scroll = 0;
            self.update_preview();
        }
    }

    pub fn previous(&mut self) {
        if !self.files.is_empty() {
            if self.selected_index > 0 {
                self.selected_index -= 1;
            } else {
                self.selected_index = self.files.len() - 1;
            }
            self.view_scroll = 0;
            self.update_preview();
        }
    }

    pub fn toggle_selection(&mut self) {
        if !self.files.is_empty() {
            if self.selected_files.contains(&self.selected_index) {
                self.selected_files.remove(&self.selected_index);
            } else {
                self.selected_files.insert(self.selected_index);
            }
        }
    }

    pub fn scroll_down(&mut self) {
        self.view_scroll = self.view_scroll.saturating_add(3); // Fast scroll
    }

    pub fn scroll_up(&mut self) {
        self.view_scroll = self.view_scroll.saturating_sub(3);
    }

    /// Chews the currently selected file and updates the preview markdown.
    fn update_preview(&mut self) {
        if self.files.is_empty() {
            self.preview_content = "Nothing here but dust and spiders. Feed me a file!".to_string();
            return;
        }

        let path = &self.files[self.selected_index];
        // We do a simple read here for the preview. Full ingestion happens via gobble_app later.
        match std::fs::read_to_string(path) {
            Ok(content) => self.preview_content = content,
            Err(_) => self.preview_content = "This one is too gristly! (Binary or unreadable file)".to_string(),
        }
    }
}

pub fn run_tui(args: &mut crate::cli::Cli) -> Result<Option<Vec<PathBuf>>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and load files
    let mut app = App::new(args);
    app.sniff()?;

    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("{:?}", err);
    }

    if app.should_execute {
        if !app.selected_files.is_empty() {
             let mut selected = Vec::new();
             for &idx in &app.selected_files {
                 selected.push(app.files[idx].clone());
             }
             return Ok(Some(selected));
        } else if !app.files.is_empty() {
             return Ok(Some(vec![app.files[app.selected_index].clone()]));
        }
    }
    
    Ok(None)
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> 
where 
    <B as ratatui::backend::Backend>::Error: std::error::Error + Send + Sync + 'static,
{
    use ratatui::{
        layout::{Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, List, ListItem, Paragraph},
    };

    let mut last_tick = std::time::Instant::now();
    let tick_rate = std::time::Duration::from_millis(100); // 10Hz Jitter
    let mut jitter_state = false;

    loop {
        if last_tick.elapsed() >= tick_rate {
            jitter_state = !jitter_state;
            last_tick = std::time::Instant::now();
        }

        terminal.draw(|f| {
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
                .split(f.area());

            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
                .split(main_chunks[0]);

            // Render The Hoard (Left Pane)
            let items: Vec<ListItem> = app
                .files
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let name = p.file_name().unwrap_or_default().to_string_lossy();
                    
                    let is_selected = app.selected_files.contains(&i);
                    let is_highlighted = i == app.selected_index;
                    
                    let mut spans = Vec::new();

                    if is_selected {
                        let teeth = if jitter_state { "V " } else { "v " };
                        spans.push(Span::styled(teeth, Style::default().fg(Color::Rgb(167, 255, 0)).add_modifier(Modifier::BOLD)));
                    } else if is_highlighted {
                        spans.push(Span::styled("> ", Style::default().fg(Color::Rgb(139, 69, 19)).add_modifier(Modifier::BOLD)));
                    } else {
                        spans.push(Span::raw("  "));
                    }

                    if is_highlighted {
                         spans.push(Span::styled(name, Style::default().add_modifier(Modifier::BOLD)));
                    } else if is_selected {
                         spans.push(Span::styled(name, Style::default().fg(Color::Rgb(167, 255, 0))));
                    } else {
                         spans.push(Span::raw(name));
                    }

                    ListItem::new(Line::from(spans))
                })
                .collect();

            let hoard_block = List::new(items)
                .highlight_symbol(">")
                .block(Block::default().borders(Borders::ALL).title(" 📁 The Hoard ").border_style(Style::default().fg(Color::Rgb(139, 69, 19))));
            f.render_widget(hoard_block, chunks[0]);

            // Render Preview (Right Pane)
            let preview_block = Paragraph::new(app.preview_content.as_str())
                .block(Block::default().borders(Borders::ALL).title(" 👀 Preview (Crunching...) ").border_style(Style::default().fg(Color::Rgb(139, 69, 19))))
                .scroll((app.view_scroll, 0))
                .style(Style::default().fg(Color::White));
            f.render_widget(preview_block, chunks[1]);

            // Render Bottom Bar (Options)
            let copy_color = if app.active_flags.copy { Color::Rgb(167, 255, 0) } else { Color::DarkGray };
            let open_color = if app.active_flags.open { Color::Rgb(167, 255, 0) } else { Color::DarkGray };
            let split_color = if app.active_flags.split { Color::Rgb(167, 255, 0) } else { Color::DarkGray };
            let scrub_color = if app.active_flags.scrub { Color::Rgb(167, 255, 0) } else { Color::DarkGray };
            let tokens_color = if app.active_flags.tokens { Color::Rgb(167, 255, 0) } else { Color::DarkGray };

            let bottom_text = Line::from(vec![
                Span::raw(" Toggles: "),
                Span::styled("[c]opy", Style::default().fg(copy_color).add_modifier(Modifier::BOLD)),
                Span::raw(" | "),
                Span::styled("[o]pen", Style::default().fg(open_color).add_modifier(Modifier::BOLD)),
                Span::raw(" | "),
                Span::styled("s[p]lit", Style::default().fg(split_color).add_modifier(Modifier::BOLD)),
                Span::raw(" | "),
                Span::styled("[s]crub", Style::default().fg(scrub_color).add_modifier(Modifier::BOLD)),
                Span::raw(" | "),
                Span::styled("[t]okens", Style::default().fg(tokens_color).add_modifier(Modifier::BOLD)),
                Span::raw("      "),
                Span::styled(" [Space] Select | [Enter] Gobble | [q] Quit ", Style::default().fg(Color::Rgb(139, 69, 19)).add_modifier(Modifier::BOLD)),
            ]);

            let bottom_block = Paragraph::new(bottom_text)
                .block(Block::default().borders(Borders::ALL));
            f.render_widget(bottom_block, main_chunks[1]);
        })?;

        // Poll with a timeout so the loop iter runs fast enough to render the jitter animation
        if event::poll(std::time::Duration::from_millis(16))? && let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                KeyCode::Enter => {
                    app.should_execute = true;
                    app.should_quit = true;
                }
                KeyCode::Char(' ') => app.toggle_selection(),
                KeyCode::Char('c') => app.active_flags.copy = !app.active_flags.copy,
                KeyCode::Char('o') => app.active_flags.open = !app.active_flags.open,
                KeyCode::Char('p') => app.active_flags.split = !app.active_flags.split,
                KeyCode::Char('s') => app.active_flags.scrub = !app.active_flags.scrub,
                KeyCode::Char('t') => app.active_flags.tokens = !app.active_flags.tokens,
                KeyCode::Down | KeyCode::Char('j') => app.next(),
                KeyCode::Up | KeyCode::Char('k') => app.previous(),
                // Page down preview
                KeyCode::PageDown => app.scroll_down(),
                // Page up preview
                KeyCode::PageUp => app.scroll_up(),
                // Emulate vim scrolling on the right pane using half-page jumps
                KeyCode::Char('d') => {
                        // A simple Ctrl+d emulation without checking modifiers for MVP
                        app.scroll_down();
                }
                KeyCode::Char('u') => {
                        app.scroll_up();
                }
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
