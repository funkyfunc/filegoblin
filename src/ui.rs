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
    pub current_dir: PathBuf,
    pub root_dir: PathBuf,
    pub current_items: Vec<PathBuf>,
    pub selected_index: usize,
    pub selected_paths: std::collections::HashSet<PathBuf>,
    pub history: std::collections::HashMap<PathBuf, usize>,
    pub preview_content: String,
    pub view_scroll: u16,
    pub should_quit: bool,
    pub should_execute: bool,
    pub active_flags: &'a mut filegoblin::cli::Cli,
    pub dir_file_counts: std::collections::HashMap<PathBuf, usize>,
}

impl<'a> App<'a> {
    pub fn new(flags: &'a mut filegoblin::cli::Cli) -> Self {
        let path_str = flags.paths.first().map(|s| s.as_str()).unwrap_or(".");
        let root = std::path::Path::new(path_str).to_path_buf();
        let current_dir = root.clone();
        
        Self {
            current_dir,
            root_dir: root,
            current_items: Vec::new(),
            selected_index: 0,
            selected_paths: std::collections::HashSet::new(),
            history: std::collections::HashMap::new(),
            preview_content: "Loading...".to_string(),
            view_scroll: 0,
            should_quit: false,
            should_execute: false,
            active_flags: flags,
            dir_file_counts: std::collections::HashMap::new(),
        }
    }

    /// Sniffs (reads) the current directory and populates the Hoard.
    pub fn sniff(&mut self) -> Result<()> {
        self.current_items.clear();
        
        if !self.current_dir.exists() {
            return Ok(());
        }

        if self.current_dir.is_file() {
            self.current_items.push(self.current_dir.clone());
        } else {
            let mut dirs = Vec::new();
            let mut files = Vec::new();
            
            if let Ok(entries) = std::fs::read_dir(&self.current_dir) {
                // Ignore dotfiles and load entries
                for entry in entries.flatten() {
                    let path = entry.path();
                    let is_hidden = path.file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.starts_with('.') && s != "." && s != "..")
                        .unwrap_or(false);

                    if !is_hidden {
                        if path.is_dir() {
                            dirs.push(path);
                        } else if path.is_file() {
                            files.push(path);
                        }
                    }
                }
            }
            
            // Sort alphabetically within each category
            dirs.sort();
            files.sort();
            
            self.current_items.extend(dirs);
            self.current_items.extend(files);
        }
        
        self.update_preview();
        Ok(())
    }

    pub fn next(&mut self) {
        if !self.current_items.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.current_items.len();
            self.view_scroll = 0;
            self.update_preview();
        }
    }

    pub fn previous(&mut self) {
        if !self.current_items.is_empty() {
            if self.selected_index > 0 {
                self.selected_index -= 1;
            } else {
                self.selected_index = self.current_items.len() - 1;
            }
            self.view_scroll = 0;
            self.update_preview();
        }
    }

    pub fn toggle_selection(&mut self) {
        if !self.current_items.is_empty() {
            let path = self.current_items[self.selected_index].clone();
            
            if path.is_file() {
                if self.selected_paths.contains(&path) {
                    self.selected_paths.remove(&path);
                } else {
                    self.selected_paths.insert(path);
                }
            } else if path.is_dir() {
                let mut nested_files = Vec::new();
                for entry in ignore::WalkBuilder::new(&path).build().flatten() {
                    if entry.file_type().is_some_and(|ft| ft.is_file()) {
                        nested_files.push(entry.into_path());
                    }
                }
                
                self.dir_file_counts.insert(path.clone(), nested_files.len());
                
                // Check if ALL nested files are already selected
                let all_selected = nested_files.iter().all(|f| self.selected_paths.contains(f));
                
                if all_selected {
                    // Deselect all
                    for f in nested_files {
                        self.selected_paths.remove(&f);
                    }
                } else {
                    // Select all (even if partially selected)
                    for f in nested_files {
                        self.selected_paths.insert(f);
                    }
                }
            }
        }
    }

    pub fn enter_directory(&mut self) {
        if !self.current_items.is_empty() {
            let path = self.current_items[self.selected_index].clone();
            if path.is_dir() {
                // Save current position before diving
                self.history.insert(self.current_dir.clone(), self.selected_index);
                
                self.current_dir = path;
                self.selected_index = 0; // Reset index for the new cave
                let _ = self.sniff();
            }
        }
    }

    pub fn leave_directory(&mut self) {
        if self.current_dir != self.root_dir && let Some(parent) = self.current_dir.parent() {
                self.current_dir = parent.to_path_buf();
                let _ = self.sniff();
                
                // Restore previous position if it exists, otherwise default to 0
                if let Some(&saved_idx) = self.history.get(&self.current_dir) {
                    // Safety bounds check in case folder contents changed
                    if saved_idx < self.current_items.len() {
                        self.selected_index = saved_idx;
                    } else if !self.current_items.is_empty() {
                        self.selected_index = self.current_items.len() - 1;
                    } else {
                        self.selected_index = 0;
                    }
                } else {
                    self.selected_index = 0;
                }
                
                self.update_preview();
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
        if self.current_items.is_empty() {
            self.preview_content = "Nothing here but dust and spiders. Feed me a file!".to_string();
            return;
        }

        let path = &self.current_items[self.selected_index];
        if path.is_dir() {
            let item_count = std::fs::read_dir(path).map(|d| d.count()).unwrap_or(0);
            self.preview_content = format!(
                "📁 Cave: {}\n\nThis cave contains {} items.\n\nPress Right Arrow or 'l' to enter the cave.\nPress Space to select the entire cave for consumption.",
                path.file_name().unwrap_or_default().to_string_lossy(),
                item_count
            );
            return;
        }

        // We do a simple read here for the preview. Full ingestion happens via gobble_app later.
        match std::fs::read_to_string(path) {
            Ok(content) => self.preview_content = content,
            Err(_) => self.preview_content = "This one is too gristly! (Binary or unreadable file)".to_string(),
        }
    }
}

pub fn run_tui(args: &mut filegoblin::cli::Cli) -> Result<Option<Vec<PathBuf>>> {
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
        if !app.selected_paths.is_empty() {
             let selected = app.selected_paths.into_iter().collect();
             return Ok(Some(selected));
        } else if !app.current_items.is_empty() {
             return Ok(Some(vec![app.current_items[app.selected_index].clone()]));
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

    // --- Goblincore Theme Palette ---
    const C_PRIMARY: Color = Color::Rgb(167, 255, 0);   // Acid Green
    const C_SECONDARY: Color = Color::Rgb(139, 69, 19); // Earthy Brown
    const C_ACCENT: Color = Color::Rgb(255, 191, 0);    // Warning Amber
    const C_MUTED: Color = Color::Rgb(112, 128, 144);   // Stone Gray
    // --------------------------------

    let mut last_tick = std::time::Instant::now();
    let tick_rate = std::time::Duration::from_millis(125); // 8Hz Snappy Jitter
    let mut jitter_state: u8 = 0;

    loop {
        if last_tick.elapsed() >= tick_rate {
            jitter_state = (jitter_state + 1) % 8; // Cycles through 0-7
            last_tick = std::time::Instant::now();
        }

        // Lazy-load file counts for partially selected directories to avoid doing it in the draw loop
        let mut missing_counts = Vec::new();
        for p in &app.current_items {
            if p.is_dir() && !app.dir_file_counts.contains_key(p) && app.selected_paths.iter().any(|s| s.starts_with(p)) {
                missing_counts.push(p.clone());
            }
        }
        for p in missing_counts {
            let count = ignore::WalkBuilder::new(&p)
                .build()
                .flatten()
                .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
                .count();
            app.dir_file_counts.insert(p, count);
        }

        terminal.draw(|f| {
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(2), Constraint::Min(0), Constraint::Length(3)].as_ref())
                .split(f.area());

            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
                .split(main_chunks[1]);

            // Render Header (Top Bar)
            let header_text = Line::from(vec![
                Span::styled(" (o_o) filegoblin ", Style::default().fg(C_PRIMARY).add_modifier(Modifier::BOLD)),
                Span::styled(format!(":: {}", app.current_dir.display()), Style::default().fg(C_MUTED).add_modifier(Modifier::ITALIC)),
            ]);
            let header_block = Paragraph::new(header_text);
            f.render_widget(header_block, main_chunks[0]);

            // Render The Hoard (Left Pane)
            let items: Vec<ListItem> = app
                .current_items
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let mut name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    if p.is_dir() {
                        name = format!("📁 {}/", name);
                    }
                    
                    let mut is_selected_full = false;
                    let mut is_selected_partial = false;
                    
                    if p.is_file() {
                        is_selected_full = app.selected_paths.contains(p);
                    } else if p.is_dir() {
                        // Check if ANY file inside this directory is selected
                        is_selected_partial = app.selected_paths.iter().any(|s| s.starts_with(p));
                        
                        // PERFORMANCE FIX: 
                        // Instead of running WalkBuilder 8 times a second in the draw loop,
                        // we compare the number of selected files to our cached directory file count.
                        if is_selected_partial && let Some(&total_files) = app.dir_file_counts.get(p) {
                            let selected_count = app.selected_paths.iter().filter(|s| s.starts_with(p)).count();
                            if selected_count == total_files && total_files > 0 {
                                is_selected_full = true;
                                is_selected_partial = false;
                            }
                        }
                    }
                    
                    let is_highlighted = i == app.selected_index;
                    
                    let mut spans = Vec::new();

                    if is_selected_full {
                        let (teeth, color) = match jitter_state % 4 {
                            0 => ("v ", C_PRIMARY),
                            1 => ("vw", C_PRIMARY),
                            2 => ("wW", C_ACCENT),  // Yellow warning flash
                            _ => ("Wv", C_PRIMARY),
                        };
                        spans.push(Span::styled(teeth, Style::default().fg(color).add_modifier(Modifier::BOLD)));
                    } else if is_selected_partial {
                         // Partially selected directory
                         spans.push(Span::styled("~ ", Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD)));
                    } else if is_highlighted {
                        spans.push(Span::styled("► ", Style::default().fg(C_SECONDARY).add_modifier(Modifier::BOLD)));
                    } else {
                        spans.push(Span::raw("  "));
                    }

                    if is_highlighted && is_selected_full {
                         // Selected AND Highlighted: Glowing Green Background, Black text
                         spans.push(Span::styled(name, Style::default().bg(C_PRIMARY).fg(Color::Black).add_modifier(Modifier::BOLD)));
                    } else if is_highlighted && is_selected_partial {
                         // Partially Selected AND Highlighted: Accent Background, Black text
                         spans.push(Span::styled(name, Style::default().bg(C_ACCENT).fg(Color::Black).add_modifier(Modifier::BOLD)));
                    } else if is_highlighted {
                         // Just Highlighted: Brown Background, White text
                         spans.push(Span::styled(name, Style::default().bg(C_SECONDARY).fg(Color::White).add_modifier(Modifier::BOLD)));
                    } else if is_selected_full {
                         // Just Selected: Green text
                         spans.push(Span::styled(name, Style::default().fg(C_PRIMARY).add_modifier(Modifier::BOLD)));
                    } else if is_selected_partial {
                         // Just Partially Selected: Yellow text
                         spans.push(Span::styled(name, Style::default().fg(C_ACCENT)));
                    } else {
                         // Normal
                         spans.push(Span::raw(name));
                    }

                    ListItem::new(Line::from(spans))
                })
                .collect();

            let hoard_title = if jitter_state.is_multiple_of(2) { " 📁 The Hoard " } else { " 📂 The Hoard " };
            let hoard_block = List::new(items)
                .highlight_symbol("► ")
                .block(Block::default()
                    .padding(ratatui::widgets::Padding::horizontal(1))
                    .borders(Borders::ALL)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .title(hoard_title)
                    .title_style(Style::default().fg(C_PRIMARY).add_modifier(Modifier::BOLD))
                    .border_style(Style::default().fg(C_SECONDARY))
                );
            f.render_widget(hoard_block, chunks[0]);

            // Render Preview (Right Pane)
            let eye = match jitter_state {
                0..=3 => "(o_o)", // Open
                4 => "(-_-)",             // Blink
                5 | 6 => "(^w^)",         // Happy chew
                _ => "(-_-)",             // Blink
            };
            let preview_title = format!(" {} Preview (Crunching...) ", eye);

            let preview_block = Paragraph::new(app.preview_content.as_str())
                .block(Block::default()
                    .padding(ratatui::widgets::Padding::horizontal(1))
                    .borders(Borders::ALL)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .title(preview_title)
                    .title_style(Style::default().fg(C_PRIMARY).add_modifier(Modifier::BOLD))
                    .border_style(Style::default().fg(C_SECONDARY))
                )
                .scroll((app.view_scroll, 0))
                .style(Style::default().fg(Color::White));
            f.render_widget(preview_block, chunks[1]);

            // Render Bottom Bar (Options)
            // Render Bottom Bar (Options)
            let copy_color = if app.active_flags.copy { C_PRIMARY } else { C_MUTED };
            let open_color = if app.active_flags.open { C_PRIMARY } else { C_MUTED };
            let split_color = if app.active_flags.split { C_PRIMARY } else { C_MUTED };
            let chunk_color = if app.active_flags.chunk.is_some() { C_PRIMARY } else { C_MUTED };
            let scrub_color = if app.active_flags.scrub { C_PRIMARY } else { C_MUTED };
            let tokens_color = if app.active_flags.tokens { C_PRIMARY } else { C_MUTED };
            let compress_color = if app.active_flags.compress.is_some() { C_PRIMARY } else { C_MUTED };
            let compress_label = match &app.active_flags.compress {
                Some(filegoblin::cli::CompressionLevel::Contextual) => "co[m]press:CTX",
                Some(filegoblin::cli::CompressionLevel::Aggressive) => "co[m]press:AGG",
                Some(filegoblin::cli::CompressionLevel::Safe)       => "co[m]press:SAF",
                None                                                => "co[m]press",
            };

            let bottom_text = Line::from(vec![
                Span::raw(" Toggles: "),
                Span::styled("[c]opy", Style::default().fg(copy_color).add_modifier(Modifier::BOLD)),
                Span::raw(" | "),
                Span::styled("[o]pen", Style::default().fg(open_color).add_modifier(Modifier::BOLD)),
                Span::raw(" | "),
                Span::styled("s[p]lit", Style::default().fg(split_color).add_modifier(Modifier::BOLD)),
                Span::raw(" | "),
                Span::styled("chu[n]k", Style::default().fg(chunk_color).add_modifier(Modifier::BOLD)),
                Span::raw(" | "),
                Span::styled("[s]crub", Style::default().fg(scrub_color).add_modifier(Modifier::BOLD)),
                Span::raw(" | "),
                Span::styled("[t]okens", Style::default().fg(tokens_color).add_modifier(Modifier::BOLD)),
                Span::raw(" | "),
                Span::styled(compress_label, Style::default().fg(compress_color).add_modifier(Modifier::BOLD)),
                Span::raw("      "),
                Span::styled(" [Space] Select | [Enter] Gobble | [q] Quit ", Style::default().fg(C_SECONDARY).add_modifier(Modifier::BOLD)),
            ]);

            let bottom_block = Paragraph::new(bottom_text)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .border_style(Style::default().fg(C_MUTED))
                );
            f.render_widget(bottom_block, main_chunks[2]);
        })?;

        // Poll with a timeout so the loop iter runs fast enough to render the jitter animation
        if event::poll(std::time::Duration::from_millis(16))? && let Event::Key(key) = event::read()? && key.kind == event::KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                        KeyCode::Enter => {
                            app.should_execute = true;
                            app.should_quit = true;
                        }
                        KeyCode::Char(' ') => app.toggle_selection(),
                        KeyCode::Char('c') => app.active_flags.copy = !app.active_flags.copy,
                        KeyCode::Char('o') => app.active_flags.open = !app.active_flags.open,
                        KeyCode::Char('p') => {
                            app.active_flags.split = !app.active_flags.split;
                            if app.active_flags.split {
                                app.active_flags.chunk = None; // Mutually exclusive
                            }
                        },
                        KeyCode::Char('n') => {
                            if app.active_flags.chunk.is_none() {
                                app.active_flags.chunk = Some("100k".to_string());
                                app.active_flags.split = false; // Mutually exclusive
                            } else {
                                app.active_flags.chunk = None;
                            }
                        },
                        KeyCode::Char('m') => {
                            app.active_flags.compress = match app.active_flags.compress {
                                None => Some(filegoblin::cli::CompressionLevel::Contextual),
                                Some(filegoblin::cli::CompressionLevel::Contextual) => Some(filegoblin::cli::CompressionLevel::Aggressive),
                                Some(filegoblin::cli::CompressionLevel::Aggressive) => Some(filegoblin::cli::CompressionLevel::Safe),
                                Some(filegoblin::cli::CompressionLevel::Safe) => None,
                            };
                        },
                        KeyCode::Char('s') => app.active_flags.scrub = !app.active_flags.scrub,
                        KeyCode::Char('t') => app.active_flags.tokens = !app.active_flags.tokens,
                        KeyCode::Down | KeyCode::Char('j') => app.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.previous(),
                        KeyCode::Right | KeyCode::Char('l') => app.enter_directory(),
                        KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => app.leave_directory(),
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
