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
use url::Url;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;

pub enum TuiMessage {
    DirLoaded(TuiNode, Vec<TuiNode>),
    DirLoadError(TuiNode, String),
    PreviewLoaded(TuiNode, String),
    PreviewLoadError(TuiNode, String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TuiNode {
    LocalDir(PathBuf),
    LocalFile(PathBuf),
    WebDir { url: String, title: Option<String> },
    TweetNode { url: String, text_preview: String },
}

impl TuiNode {
    pub fn is_dir(&self) -> bool {
        matches!(self, TuiNode::LocalDir(_) | TuiNode::WebDir { .. })
    }
    
    pub fn is_file(&self) -> bool {
        !self.is_dir()
    }

    pub fn display_name(&self) -> String {
        match self {
            TuiNode::LocalDir(p) | TuiNode::LocalFile(p) => p.file_name().unwrap_or_default().to_string_lossy().to_string(),
            TuiNode::WebDir { url, title } => {
                let name = if let Ok(u) = Url::parse(url) {
                    if let Some(segments) = u.path_segments() {
                        let last = segments.last().unwrap_or("");
                        if last.is_empty() { u.host_str().unwrap_or(url).to_string() } else { last.to_string() }
                    } else { u.host_str().unwrap_or(url).to_string() }
                } else { url.clone() };
                
                if let Some(t) = title {
                    if name.is_empty() { return t.clone(); }
                    format!("{} ({})", t, name)
                } else {
                    if name.is_empty() { return url.clone(); }
                    name
                }
            },
            TuiNode::TweetNode { url: _, text_preview } => text_preview.chars().take(60).collect::<String>().replace("\n", " ") + "...",
        }
    }

    pub fn target_str(&self) -> String {
        match self {
            TuiNode::LocalDir(p) | TuiNode::LocalFile(p) => p.to_string_lossy().to_string(),
            TuiNode::WebDir { url, .. } | TuiNode::TweetNode { url, .. } => url.clone(),
        }
    }

    pub fn from_arg(arg: &str) -> Self {
        if let Ok(url) = Url::parse(arg) {
            if url.scheme() == "http" || url.scheme() == "https" {
                return TuiNode::WebDir { url: arg.to_string(), title: None };
            }
        }
        let p = PathBuf::from(arg);
        if p.is_dir() {
            TuiNode::LocalDir(p)
        } else {
            TuiNode::LocalFile(p)
        }
    }
}

pub struct App<'a> {
    pub current_dir: TuiNode,
    pub root_dir: TuiNode,
    pub current_items: Vec<TuiNode>,
    pub selected_index: usize,
    pub selected_paths: std::collections::HashSet<TuiNode>,
    pub history: std::collections::HashMap<TuiNode, usize>,
    pub preview_content: String,
    pub view_scroll: u16,
    pub should_quit: bool,
    pub should_execute: bool,
    pub active_flags: &'a mut filegoblin::cli::Cli,
    pub dir_file_counts: std::collections::HashMap<TuiNode, usize>,
    
    // Async mechanisms
    pub rx_msgs: Receiver<TuiMessage>,
    pub tx_msgs: Sender<TuiMessage>,
    pub is_loading_dir: bool,
    pub nav_stack: Vec<TuiNode>,
    pub dir_cache: std::collections::HashMap<TuiNode, Vec<TuiNode>>,
    pub preview_cache: std::collections::HashMap<TuiNode, String>,
    pub is_input_mode: bool,
    pub input_buffer: String,
    pub input_cursor: usize,
    pub last_write_file: Option<String>,
}

impl<'a> App<'a> {
    pub fn new(flags: &'a mut filegoblin::cli::Cli) -> Self {
        let path_str = flags.paths.first().map(|s| s.as_str()).unwrap_or(".");
        let root = TuiNode::from_arg(path_str);
        let current_dir = root.clone();
        
        let initial_write = flags.write.clone();

        let (tx, rx) = channel();

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
            rx_msgs: rx,
            tx_msgs: tx,
            is_loading_dir: false,
            nav_stack: Vec::new(),
            dir_cache: std::collections::HashMap::new(),
            preview_cache: std::collections::HashMap::new(),
            is_input_mode: false,
            input_buffer: String::new(),
            input_cursor: 0,
            last_write_file: initial_write,
        }
    }

    pub fn process_messages(&mut self) -> bool {
        let mut ui_dirty = false;
        while let Ok(msg) = self.rx_msgs.try_recv() {
            match msg {
                TuiMessage::DirLoaded(node, items) => {
                    self.dir_cache.insert(node.clone(), items.clone());
                    if self.current_dir == node {
                        self.is_loading_dir = false;
                        self.current_items = items;
                        
                        // Sort items: folders first, then files
                        self.current_items.sort_by(|a, b| {
                            let a_dir = a.is_dir();
                            let b_dir = b.is_dir();
                            if a_dir && !b_dir {
                                std::cmp::Ordering::Less
                            } else if !a_dir && b_dir {
                                std::cmp::Ordering::Greater
                            } else {
                                a.display_name().cmp(&b.display_name())
                            }
                        });

                        // Restore previous position if it exists, otherwise 0
                        if let Some(&saved_idx) = self.history.get(&self.current_dir) {
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

                        self.request_preview_update();
                        ui_dirty = true;
                    }
                }
                TuiMessage::DirLoadError(node, err) => {
                    if self.current_dir == node {
                        self.is_loading_dir = false;
                        self.current_items.clear();
                        self.preview_content = format!("Error loading directory: {}", err);
                        ui_dirty = true;
                    }
                }
                TuiMessage::PreviewLoaded(node, preview) => {
                    self.preview_cache.insert(node.clone(), preview.clone());
                    if !self.current_items.is_empty() && self.current_items[self.selected_index] == node {
                        self.preview_content = preview;
                        ui_dirty = true;
                    }
                }
                TuiMessage::PreviewLoadError(node, err) => {
                    if !self.current_items.is_empty() && self.current_items[self.selected_index] == node {
                        self.preview_content = format!("Error loading preview: {}", err);
                        ui_dirty = true;
                    }
                }
            }
        }
        ui_dirty
    }

    pub fn sniff(&mut self) {
        self.current_items.clear();

        if let Some(cached_items) = self.dir_cache.get(&self.current_dir) {
            self.current_items = cached_items.clone();
            if let Some(&saved_idx) = self.history.get(&self.current_dir) {
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
            self.request_preview_update();
            return;
        }

        self.preview_content = "Diving into the cave...".to_string();
        self.is_loading_dir = true;

        let node = self.current_dir.clone();
        let tx = self.tx_msgs.clone();

        thread::spawn(move || {
            match &node {
                TuiNode::LocalDir(path) => {
                    let mut items = Vec::new();
                    if let Ok(entries) = std::fs::read_dir(path) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            let is_hidden = p.file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| s.starts_with('.') && s != "." && s != "..")
                                .unwrap_or(false);

                            if !is_hidden {
                                if p.is_dir() {
                                    items.push(TuiNode::LocalDir(p));
                                } else {
                                    items.push(TuiNode::LocalFile(p));
                                }
                            }
                        }
                    }
                    let _ = tx.send(TuiMessage::DirLoaded(node, items));
                }
                TuiNode::WebDir { url, .. } => {
                    // Check if it's twitter
                    if url.contains("twitter.com") || url.contains("x.com") {
                        let twitter = filegoblin::parsers::twitter::TwitterGobbler { flavor: filegoblin::flavors::Flavor::Human };
                        match twitter.get_thread_nodes(url) {
                            Ok(thread_items) => {
                                let items: Vec<TuiNode> = thread_items.into_iter().map(|(tid, text)| {
                                    TuiNode::TweetNode { url: tid, text_preview: text }
                                }).collect();
                                let _ = tx.send(TuiMessage::DirLoaded(node.clone(), items));
                            }
                            Err(e) => {
                                let _ = tx.send(TuiMessage::DirLoadError(node.clone(), format!("{}", e)));
                            }
                        }
                    } else {
                        // Standard web page, extract links
                        if let Ok(res) = reqwest::blocking::Client::builder().use_rustls_tls().build().unwrap().get(url).send() {
                            if let Ok(html) = res.text() {
                                let document = scraper::Html::parse_document(&html);
                                let selector = scraper::Selector::parse("a").unwrap();
                                let mut items = Vec::new();
                                if let Ok(base_url) = Url::parse(url) {
                                    for element in document.select(&selector) {
                                        if let Some(href) = element.value().attr("href") {
                                            if let Ok(mut next_url) = base_url.join(href) {
                                                let s = next_url.scheme();
                                                if s == "http" || s == "https" {
                                                    next_url.set_fragment(None);
                                                    let title = element.text().collect::<Vec<_>>().join(" ");
                                                    let title_opt = if title.trim().is_empty() { None } else { Some(title.trim().to_string()) };
                                                    items.push(TuiNode::WebDir { url: next_url.to_string(), title: title_opt });
                                                }
                                            }
                                        }
                                    }
                                }
                                items.sort_by_key(|n| n.target_str());
                                items.dedup_by_key(|n| n.target_str());
                                let _ = tx.send(TuiMessage::DirLoaded(node.clone(), items));
                            } else {
                                let _ = tx.send(TuiMessage::DirLoadError(node.clone(), "Failed to read HTML text".to_string()));
                            }
                        } else {
                            let _ = tx.send(TuiMessage::DirLoadError(node.clone(), "Failed to fetch URL".to_string()));
                        }
                    }
                }
                _ => {
                    let _ = tx.send(TuiMessage::DirLoaded(node, vec![]));
                }
            }
        });
    }

    pub fn next(&mut self) {
        if !self.current_items.is_empty() && !self.is_loading_dir {
            self.selected_index = (self.selected_index + 1) % self.current_items.len();
            self.view_scroll = 0;
            self.request_preview_update();
        }
    }

    pub fn previous(&mut self) {
        if !self.current_items.is_empty() && !self.is_loading_dir {
            if self.selected_index > 0 {
                self.selected_index -= 1;
            } else {
                self.selected_index = self.current_items.len() - 1;
            }
            self.view_scroll = 0;
            self.request_preview_update();
        }
    }

    pub fn toggle_selection(&mut self) {
        if !self.current_items.is_empty() && !self.is_loading_dir {
            let node = self.current_items[self.selected_index].clone();
            
            if node.is_file() {
                if self.selected_paths.contains(&node) {
                    self.selected_paths.remove(&node);
                } else {
                    self.selected_paths.insert(node);
                }
            } else if node.is_dir() {
                // If local dir, support nested select
                if let TuiNode::LocalDir(path) = &node {
                    let mut nested_files = Vec::new();
                    for entry in ignore::WalkBuilder::new(path).build().flatten() {
                        if entry.file_type().is_some_and(|ft| ft.is_file()) {
                            nested_files.push(TuiNode::LocalFile(entry.into_path()));
                        }
                    }
                    
                    self.dir_file_counts.insert(node.clone(), nested_files.len());
                    
                    let all_selected = nested_files.iter().all(|f| self.selected_paths.contains(f));
                    if all_selected {
                        for f in nested_files { self.selected_paths.remove(&f); }
                    } else {
                        for f in nested_files { self.selected_paths.insert(f); }
                    }
                } else if matches!(node, TuiNode::WebDir { .. }) {
                    // For WebDir, just select the dir node itself (we treat it as a page to scrape later)
                    if self.selected_paths.contains(&node) {
                        self.selected_paths.remove(&node);
                    } else {
                        self.selected_paths.insert(node);
                    }
                }
            }
        }
    }

    pub fn enter_directory(&mut self) {
        if !self.current_items.is_empty() && !self.is_loading_dir {
            let node = self.current_items[self.selected_index].clone();
            if node.is_dir() {
                // Save current position before diving
                self.history.insert(self.current_dir.clone(), self.selected_index);
                self.nav_stack.push(self.current_dir.clone());
                
                self.current_dir = node;
                self.selected_index = 0;
                self.sniff();
            }
        }
    }

    pub fn leave_directory(&mut self) {
        if self.current_dir != self.root_dir && !self.is_loading_dir {
            // Find parent using chronological navigation stack
            let parent_node = self.nav_stack.pop().or_else(|| {
                // Fallback for local files just in case
                match &self.current_dir {
                    TuiNode::LocalDir(p) | TuiNode::LocalFile(p) => {
                        p.parent().map(|parent| TuiNode::LocalDir(parent.to_path_buf()))
                    }
                    _ => None
                }
            });

            if let Some(parent) = parent_node {
                self.current_dir = parent;
                self.sniff();
            }
        }
    }

    pub fn scroll_down(&mut self) {
        self.view_scroll = self.view_scroll.saturating_add(3);
    }

    pub fn scroll_up(&mut self) {
        self.view_scroll = self.view_scroll.saturating_sub(3);
    }

    fn request_preview_update(&mut self) {
        if self.current_items.is_empty() {
            self.preview_content = "Nothing here but dust and spiders. Feed me a file!".to_string();
            return;
        }

        let node = self.current_items[self.selected_index].clone();
        
        if let Some(cached_preview) = self.preview_cache.get(&node) {
            self.preview_content = cached_preview.clone();
            return;
        }

        match &node {
            TuiNode::LocalDir(path) => {
                let item_count = std::fs::read_dir(path).map(|d| d.count()).unwrap_or(0);
                self.preview_content = format!(
                    "📁 Cave: {}\n\nThis cave contains {} items.\n\nPress Right Arrow or 'l' to enter the cave.\nPress Space to select the entire cave for consumption.",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    item_count
                );
            }
            TuiNode::LocalFile(path) => {
                match std::fs::read_to_string(path) {
                    Ok(content) => self.preview_content = content,
                    Err(_) => self.preview_content = "This one is too gristly! (Binary or unreadable file)".to_string(),
                }
            }
            TuiNode::WebDir { url, title: _ } => {
                self.preview_content = "Fetching web preview from network...".to_string();
                let tx = self.tx_msgs.clone();
                let url_c = url.clone();
                thread::spawn(move || {
                    if let Ok(res) = reqwest::blocking::Client::builder().use_rustls_tls().build().unwrap().get(&url_c).send() {
                        if !res.status().is_success() {
                            let _ = tx.send(TuiMessage::PreviewLoadError(node, format!("HTTP Error: {}", res.status())));
                            return;
                        }
                        if let Ok(html) = res.text() {
                            let gobbler = filegoblin::parsers::web::WebGobbler { extract_full: false };
                            match filegoblin::parsers::gobble::Gobble::gobble_str(&gobbler, &html) {
                                Ok(md) => { let _ = tx.send(TuiMessage::PreviewLoaded(node, md)); }
                                Err(e) => { let _ = tx.send(TuiMessage::PreviewLoadError(node, format!("{}", e))); }
                            }
                        }
                    } else {
                        let _ = tx.send(TuiMessage::PreviewLoadError(node, "Network request failed".to_string()));
                    }
                });
            }
            TuiNode::TweetNode { text_preview, .. } => {
                self.preview_content = text_preview.clone();
            }
        }
    }
}

pub fn run_tui(args: &mut filegoblin::cli::Cli) -> Result<Option<Vec<String>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(args);
    app.sniff();

    let res = run_app(&mut terminal, &mut app);

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
             let selected: Vec<String> = app.selected_paths.into_iter().map(|n| n.target_str()).collect();
             return Ok(Some(selected));
        } else if !app.current_items.is_empty() && app.selected_index < app.current_items.len() {
             return Ok(Some(vec![app.current_items[app.selected_index].target_str()]));
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

    const C_PRIMARY: Color = Color::Rgb(167, 255, 0);
    const C_SECONDARY: Color = Color::Rgb(139, 69, 19);
    const C_ACCENT: Color = Color::Rgb(255, 191, 0); 
    const C_MUTED: Color = Color::Rgb(112, 128, 144);

    let mut last_tick = std::time::Instant::now();
    let tick_rate = std::time::Duration::from_millis(125); // 8Hz Snappy Jitter
    let mut jitter_state: u8 = 0;

    loop {
        if last_tick.elapsed() >= tick_rate {
            jitter_state = (jitter_state + 1) % 8;
            last_tick = std::time::Instant::now();
        }

        // Process any async background events (loaded directories / loaded previews)
        app.process_messages();

        // Lazy-load file counts for partially selected local directories
        let mut missing_counts = Vec::new();
        for p in &app.current_items {
            if let TuiNode::LocalDir(path) = p {
                if !app.dir_file_counts.contains_key(p) && app.selected_paths.iter().any(|s| {
                    if let TuiNode::LocalFile(sp) = s { sp.starts_with(path) } else { false }
                }) {
                    missing_counts.push(p.clone());
                }
            }
        }
        for p in missing_counts {
            if let TuiNode::LocalDir(path) = &p {
                let count = ignore::WalkBuilder::new(path)
                    .build()
                    .flatten()
                    .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
                    .count();
                app.dir_file_counts.insert(p, count);
            }
        }

        terminal.draw(|f| {
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(6), Constraint::Min(0), Constraint::Length(3)].as_ref())
                .split(f.area());

            let chunks = Layout::default()
                 .direction(Direction::Horizontal)
                 .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
                 .split(main_chunks[1]);

            let is_eating = !app.selected_paths.is_empty();
            let eyes = match (is_eating, jitter_state) {
                (true, 0..=1) => "(^w^)",
                (true, 2..=7) => "(>_<)",
                (false, 0..=6) => "(o_o)",
                (false, 7)     => "(-_-)",
                _ => "(o_o)",
            };
            
            let mouth = if is_eating && jitter_state % 2 == 0 { "(V)" } else { "(W)" };

            let mut goblin_quote = "I'm hungry for files...".to_string();
            if app.is_loading_dir {
                goblin_quote = "Sniffing the network / disk... Wait for it...".to_string();
            } else if !app.current_items.is_empty() && app.selected_index < app.current_items.len() {
                let hovered = &app.current_items[app.selected_index];
                if hovered.is_dir() {
                    if let TuiNode::WebDir { .. } = hovered {
                        if app.dir_cache.contains_key(hovered) {
                            goblin_quote = "A cached web link! Select to ingest ONLY this page, or dive in for scrape history.".to_string();
                        } else {
                            goblin_quote = "A web link! Select to ingest ONLY this page, or dive in to scrape outbound links.".to_string();
                        }
                    } else {
                        let known_count = app.dir_file_counts.get(hovered).copied().unwrap_or(0);
                        if known_count > 50 {
                             goblin_quote = "A massive hoard! It will take ages to chew...".to_string();
                        } else {
                             goblin_quote = "A juicy cave! We'll chew through the whole thing.".to_string();
                        }
                    }
                } else if let TuiNode::TweetNode { .. } = hovered {
                    goblin_quote = "A juicy tweet thread! Very digestible.".to_string();
                } else if let TuiNode::LocalFile(p) = hovered {
                    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                        match ext {
                            "md" | "txt" | "json" | "csv" => goblin_quote = "Ah, crunchy text. Easy to digest.".to_string(),
                            "pdf" => goblin_quote = "A PDF? Grr... tough rind, but I'll crack it.".to_string(),
                            "rs" | "go" | "py" | "js" | "ts" => goblin_quote = "Code! Sweet, structured code.".to_string(),
                            "png" | "jpg" | "jpeg" | "webp" => goblin_quote = "An image? Let me get my reading glasses...".to_string(),
                            _ => goblin_quote = "Looks exotic. I wonder what it tastes like...".to_string(),
                        }
                    }
                }
            } else {
                goblin_quote = "Nothing here but dust and spiders. Pah!".to_string();
            }

            let header_text = vec![
                Line::from(vec![
                    Span::styled(format!("    {}  ", eyes), Style::default().fg(C_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("\"{}\"", goblin_quote), Style::default().fg(C_ACCENT).add_modifier(Modifier::ITALIC)),
                ]),
                Line::from(vec![
                    Span::styled(format!("     {}   ", mouth), Style::default().fg(C_PRIMARY).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("   --m-m-- ", Style::default().fg(C_PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled(" filegoblin v1.5 ", Style::default().fg(C_SECONDARY).add_modifier(Modifier::BOLD)),
                    Span::raw(" | "),
                    Span::styled(format!(":: {}", app.current_dir.target_str()), Style::default().fg(C_MUTED)),
                ]),
            ];

            let header_block = Paragraph::new(header_text)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .border_style(Style::default().fg(C_SECONDARY))
                );
            f.render_widget(header_block, main_chunks[0]);

            let items: Vec<ListItem> = app
                .current_items
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let mut name = p.display_name();
                    if p.is_dir() {
                        name = format!("📁 {}/", name);
                    } else if let TuiNode::WebDir { .. } = p {
                        name = format!("🌐 {}", name);
                    } else if let TuiNode::TweetNode { .. } = p {
                        name = format!("🐦 {}", name);
                    }
                    
                    let mut is_selected_full = app.selected_paths.contains(p);
                    let mut is_selected_partial = false;
                    
                    if p.is_dir() {
                        if let TuiNode::LocalDir(path) = p {
                            is_selected_partial = app.selected_paths.iter().any(|s| {
                                if let TuiNode::LocalFile(sp) = s { sp.starts_with(path) } else { false }
                            });
                            
                            if is_selected_partial && let Some(&total_files) = app.dir_file_counts.get(p) {
                                let selected_count = app.selected_paths.iter().filter(|s| {
                                    if let TuiNode::LocalFile(sp) = s { sp.starts_with(path) } else { false }
                                }).count();
                                if selected_count == total_files && total_files > 0 {
                                    is_selected_full = true;
                                    is_selected_partial = false;
                                }
                            }
                        }
                    }
                    
                    let is_highlighted = i == app.selected_index;
                    let mut spans = Vec::new();

                    if is_selected_full {
                        let (teeth, color) = match jitter_state % 4 {
                            0 => ("v ", C_PRIMARY),
                            1 => ("vw", C_PRIMARY),
                            2 => ("wW", C_ACCENT), 
                            _ => ("Wv", C_PRIMARY),
                        };
                        spans.push(Span::styled(teeth, Style::default().fg(color).add_modifier(Modifier::BOLD)));
                    } else if is_selected_partial {
                         spans.push(Span::styled("~ ", Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD)));
                    } else if is_highlighted {
                        spans.push(Span::styled("► ", Style::default().fg(C_SECONDARY).add_modifier(Modifier::BOLD)));
                    } else {
                        spans.push(Span::raw("  "));
                    }

                    if is_highlighted && is_selected_full {
                         spans.push(Span::styled(name, Style::default().bg(C_PRIMARY).fg(Color::Black).add_modifier(Modifier::BOLD)));
                    } else if is_highlighted && is_selected_partial {
                         spans.push(Span::styled(name, Style::default().bg(C_ACCENT).fg(Color::Black).add_modifier(Modifier::BOLD)));
                    } else if is_highlighted {
                         spans.push(Span::styled(name, Style::default().bg(C_SECONDARY).fg(Color::White).add_modifier(Modifier::BOLD)));
                    } else if is_selected_full {
                         spans.push(Span::styled(name, Style::default().fg(C_PRIMARY).add_modifier(Modifier::BOLD)));
                    } else if is_selected_partial {
                         spans.push(Span::styled(name, Style::default().fg(C_ACCENT)));
                    } else {
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

            let eye = match jitter_state {
                0..=3 => "(o_o)",
                4 => "(-_-)",    
                5 | 6 => "(^w^)",
                _ => "(-_-)",    
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

            let copy_color = if app.active_flags.copy { C_PRIMARY } else { C_MUTED };
            let open_color = if app.active_flags.open { C_PRIMARY } else { C_MUTED };
            let split_color = if app.active_flags.split { C_PRIMARY } else { C_MUTED };
            let chunk_color = if app.active_flags.chunk.is_some() { C_PRIMARY } else { C_MUTED };
            let scrub_color = if app.active_flags.scrub { C_PRIMARY } else { C_MUTED };
            let tokens_color = if app.active_flags.tokens { C_PRIMARY } else { C_MUTED };
            let write_color = if app.active_flags.write.is_some() { C_PRIMARY } else { C_MUTED };
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
                Span::styled("[w]rite", Style::default().fg(write_color).add_modifier(Modifier::BOLD)),
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
            
            // Draw input overlay if in input mode
            if app.is_input_mode {
                let area = f.area();
                let input_area = ratatui::layout::Rect::new(
                    area.width.saturating_sub(60) / 2,
                    area.height.saturating_sub(3) / 2,
                    60.min(area.width),
                    3.min(area.height)
                );
                f.render_widget(ratatui::widgets::Clear, input_area);
                // Keep the cursor blinking by styling the character
                let is_blink = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() % 1000 > 500;
                let mut spans = Vec::new();
                for (i, c) in app.input_buffer.chars().enumerate() {
                    if i == app.input_cursor {
                        if is_blink {
                            spans.push(Span::styled(c.to_string(), Style::default().add_modifier(Modifier::REVERSED)));
                        } else {
                            spans.push(Span::raw(c.to_string()));
                        }
                    } else {
                        spans.push(Span::raw(c.to_string()));
                    }
                }
                if app.input_cursor == app.input_buffer.chars().count() {
                    if is_blink {
                        spans.push(Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)));
                    } else {
                        spans.push(Span::raw(" "));
                    }
                }
                
                let input_block = Paragraph::new(Line::from(spans))
                    .block(Block::default()
                        .title(" Set Write output filename (e.g. out.md) - [Enter] to Save / [Esc] to Cancel ")
                        .borders(Borders::ALL)
                        .border_type(ratatui::widgets::BorderType::Rounded)
                        .border_style(Style::default().fg(C_PRIMARY))
                    )
                    .style(Style::default().fg(Color::Yellow));
                f.render_widget(input_block, input_area);
            }
        })?;

        if event::poll(std::time::Duration::from_millis(16))? && let Event::Key(key) = event::read()? && key.kind == event::KeyEventKind::Press {
            if app.is_input_mode {
                match key.code {
                    KeyCode::Enter => {
                        let trimmed = app.input_buffer.trim().to_string();
                        if trimmed.is_empty() {
                            app.active_flags.write = None;
                        } else {
                            app.active_flags.write = Some(trimmed.clone());
                            app.last_write_file = Some(trimmed);
                        }
                        app.is_input_mode = false;
                    }
                    KeyCode::Esc => {
                        app.is_input_mode = false;
                    }
                    KeyCode::Left => {
                        app.input_cursor = app.input_cursor.saturating_sub(1);
                    }
                    KeyCode::Right => {
                        if app.input_cursor < app.input_buffer.chars().count() {
                            app.input_cursor += 1;
                        }
                    }
                    KeyCode::Backspace => {
                        if app.input_cursor > 0 {
                            let mut chars: Vec<char> = app.input_buffer.chars().collect();
                            chars.remove(app.input_cursor - 1);
                            app.input_buffer = chars.into_iter().collect();
                            app.input_cursor -= 1;
                        }
                    }
                    KeyCode::Delete => {
                        let mut chars: Vec<char> = app.input_buffer.chars().collect();
                        if app.input_cursor < chars.len() {
                            chars.remove(app.input_cursor);
                            app.input_buffer = chars.into_iter().collect();
                        }
                    }
                    KeyCode::Char(c) => {
                        let mut chars: Vec<char> = app.input_buffer.chars().collect();
                        chars.insert(app.input_cursor, c);
                        app.input_buffer = chars.into_iter().collect();
                        app.input_cursor += 1;
                    }
                    _ => {}
                }
            } else {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                        KeyCode::Enter => {
                            app.should_execute = true;
                            app.should_quit = true;
                        }
                        KeyCode::Char(' ') => app.toggle_selection(),
                        KeyCode::Char('w') => {
                            if app.active_flags.write.is_some() {
                                // Instantly untoggle
                                app.active_flags.write = None;
                            } else {
                                // Open input box
                                app.is_input_mode = true;
                                app.input_buffer = app.last_write_file.clone().unwrap_or_else(|| "out.md".to_string());
                                app.input_cursor = app.input_buffer.chars().count();
                            }
                        }
                        KeyCode::Char('c') => app.active_flags.copy = !app.active_flags.copy,
                        KeyCode::Char('o') => app.active_flags.open = !app.active_flags.open,
                        KeyCode::Char('p') => {
                            app.active_flags.split = !app.active_flags.split;
                            if app.active_flags.split {
                                app.active_flags.chunk = None;
                            }
                        },
                        KeyCode::Char('n') => {
                            if app.active_flags.chunk.is_none() {
                                app.active_flags.chunk = Some("100k".to_string());
                                app.active_flags.split = false;
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
                        KeyCode::PageDown => app.scroll_down(),
                        KeyCode::PageUp => app.scroll_up(),
                        KeyCode::Char('d') => {
                                app.scroll_down();
                        }
                        KeyCode::Char('u') => {
                                app.scroll_up();
                        }
                        _ => {}
                    }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
