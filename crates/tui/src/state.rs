use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use url::Url;

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
            TuiNode::LocalDir(p) | TuiNode::LocalFile(p) => p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            TuiNode::WebDir { url, title } => {
                let name = if let Ok(u) = Url::parse(url) {
                    if let Some(mut segments) = u.path_segments() {
                        let last = segments.next_back().unwrap_or("");
                        if last.is_empty() {
                            u.host_str().unwrap_or(url).to_string()
                        } else {
                            last.to_string()
                        }
                    } else {
                        u.host_str().unwrap_or(url).to_string()
                    }
                } else {
                    url.clone()
                };

                if let Some(t) = title {
                    if name.is_empty() {
                        return t.clone();
                    }
                    format!("{} ({})", t, name)
                } else {
                    if name.is_empty() {
                        return url.clone();
                    }
                    name
                }
            }
            TuiNode::TweetNode {
                url: _,
                text_preview,
            } => {
                text_preview
                    .chars()
                    .take(60)
                    .collect::<String>()
                    .replace("\n", " ")
                    + "..."
            }
        }
    }

    pub fn target_str(&self) -> String {
        match self {
            TuiNode::LocalDir(p) | TuiNode::LocalFile(p) => p.to_string_lossy().to_string(),
            TuiNode::WebDir { url, .. } | TuiNode::TweetNode { url, .. } => url.clone(),
        }
    }

    pub fn from_arg(arg: &str) -> Self {
        if let Ok(url) = Url::parse(arg)
            && (url.scheme() == "http" || url.scheme() == "https")
        {
            return TuiNode::WebDir {
                url: arg.to_string(),
                title: None,
            };
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
    pub active_flags: &'a mut filegoblin_core::cli::Cli,
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
    pub fn new(flags: &'a mut filegoblin_core::cli::Cli) -> Self {
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

                        // Restore cursor position if we've been here before
                        if let Some(&prev_index) = self.history.get(&node) {
                            self.selected_index =
                                prev_index.min(self.current_items.len().saturating_sub(1));
                        } else {
                            self.selected_index = 0;
                        }

                        // Trigger an immediate preview load for whatever is now selected
                        self.trigger_preview();
                    }
                    ui_dirty = true;
                }
                TuiMessage::DirLoadError(node, err) => {
                    if self.current_dir == node {
                        self.is_loading_dir = false;
                        self.current_items = Vec::new();
                        self.preview_content = format!("Error loading directory: {}", err);
                    }
                    ui_dirty = true;
                }
                TuiMessage::PreviewLoaded(node, content) => {
                    self.preview_cache.insert(node.clone(), content.clone());
                    if !self.current_items.is_empty()
                        && self.selected_index < self.current_items.len()
                        && self.current_items[self.selected_index] == node
                    {
                        self.preview_content = content;
                    }
                    ui_dirty = true;
                }
                TuiMessage::PreviewLoadError(node, err) => {
                    if !self.current_items.is_empty()
                        && self.selected_index < self.current_items.len()
                        && self.current_items[self.selected_index] == node
                    {
                        self.preview_content = format!("Preview error: {}", err);
                    }
                    ui_dirty = true;
                }
            }
        }
        ui_dirty
    }

    /// Dispatches an asynchronous directory scan for the given node.
    pub fn sniff(&mut self) {
        // If we have a cached result, use it immediately
        if let Some(cached_items) = self.dir_cache.get(&self.current_dir) {
            self.current_items = cached_items.clone();
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
            if let Some(&prev_index) = self.history.get(&self.current_dir) {
                self.selected_index = prev_index.min(self.current_items.len().saturating_sub(1));
            } else {
                self.selected_index = 0;
            }
            self.trigger_preview();
            return;
        }

        self.is_loading_dir = true;
        let node = self.current_dir.clone();
        let tx = self.tx_msgs.clone();
        thread::spawn(move || match &node {
            TuiNode::LocalDir(path) => {
                let mut items = Vec::new();
                if let Ok(entries) = std::fs::read_dir(path) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                            if name.starts_with('.') {
                                continue;
                            }
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
            TuiNode::WebDir {
                #[allow(unused_variables)]
                url,
                ..
            } => {
                #[allow(unused_variables, unused_mut)]
                let mut handled = false;

                if !handled {
                    // Standard web page, extract links
                    #[cfg(feature = "web")]
                    {
                        if let Ok(res) = reqwest::blocking::Client::builder()
                            .use_rustls_tls()
                            .build()
                            .unwrap()
                            .get(url)
                            .send()
                        {
                            if let Ok(html) = res.text() {
                                let document = scraper::Html::parse_document(&html);
                                let selector = scraper::Selector::parse("a").unwrap();
                                let mut items = Vec::new();
                                if let Ok(base_url) = Url::parse(url) {
                                    for element in document.select(&selector) {
                                        if let Some(href) = element.value().attr("href")
                                            && let Ok(mut next_url) = base_url.join(href)
                                        {
                                            let s = next_url.scheme();
                                            if s == "http" || s == "https" {
                                                next_url.set_fragment(None);
                                                let title =
                                                    element.text().collect::<Vec<_>>().join(" ");
                                                let title_opt = if title.trim().is_empty() {
                                                    None
                                                } else {
                                                    Some(title.trim().to_string())
                                                };
                                                items.push(TuiNode::WebDir {
                                                    url: next_url.to_string(),
                                                    title: title_opt,
                                                });
                                            }
                                        }
                                    }
                                }
                                items.sort_by_key(|n| n.target_str());
                                items.dedup_by_key(|n| n.target_str());
                                let _ = tx.send(TuiMessage::DirLoaded(node.clone(), items));
                            } else {
                                let _ = tx.send(TuiMessage::DirLoadError(
                                    node.clone(),
                                    "Failed to read HTML text".to_string(),
                                ));
                            }
                        } else {
                            let _ = tx.send(TuiMessage::DirLoadError(
                                node.clone(),
                                "Failed to fetch URL".to_string(),
                            ));
                        }
                    }
                    #[cfg(not(feature = "web"))]
                    {
                        let _ = tx.send(TuiMessage::DirLoadError(
                            node.clone(),
                            "Web scraping requires the 'web' feature.".to_string(),
                        ));
                    }
                }
            }
            _ => {}
        });
    }

    pub fn next(&mut self) {
        if !self.current_items.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.current_items.len();
            self.view_scroll = 0;
            self.trigger_preview();
        }
    }

    pub fn previous(&mut self) {
        if !self.current_items.is_empty() {
            if self.selected_index == 0 {
                self.selected_index = self.current_items.len() - 1;
            } else {
                self.selected_index -= 1;
            }
            self.view_scroll = 0;
            self.trigger_preview();
        }
    }

    pub fn enter_directory(&mut self) {
        if self.current_items.is_empty() || self.selected_index >= self.current_items.len() {
            return;
        }

        let selected = &self.current_items[self.selected_index];
        if selected.is_dir() {
            self.history
                .insert(self.current_dir.clone(), self.selected_index);
            self.nav_stack.push(self.current_dir.clone());
            self.current_dir = selected.clone();
            self.current_items = Vec::new();
            self.selected_index = 0;
            self.preview_content = "Loading...".to_string();
            self.sniff();
        }
    }

    pub fn leave_directory(&mut self) {
        if let Some(prev_dir) = self.nav_stack.pop() {
            self.current_dir = prev_dir;
            self.current_items = Vec::new();
            self.selected_index = 0;
            self.preview_content = "Loading...".to_string();
            self.sniff();
        }
    }

    pub fn toggle_selection(&mut self) {
        if self.current_items.is_empty() || self.selected_index >= self.current_items.len() {
            return;
        }

        let item = self.current_items[self.selected_index].clone();

        match &item {
            TuiNode::LocalDir(path) => {
                // Toggle all files in the directory recursively
                let walker = ignore::WalkBuilder::new(path).build();
                let mut all_files: Vec<TuiNode> = Vec::new();
                for entry in walker.flatten() {
                    if entry.file_type().is_some_and(|ft| ft.is_file()) {
                        all_files.push(TuiNode::LocalFile(entry.path().to_path_buf()));
                    }
                }

                // If all are selected, deselect all. Otherwise, select all.
                let all_selected = all_files.iter().all(|f| self.selected_paths.contains(f));
                if all_selected {
                    for f in all_files {
                        self.selected_paths.remove(&f);
                    }
                } else {
                    for f in all_files {
                        self.selected_paths.insert(f);
                    }
                }
            }
            _ => {
                if self.selected_paths.contains(&item) {
                    self.selected_paths.remove(&item);
                } else {
                    self.selected_paths.insert(item);
                }
            }
        }

        // Auto-advance after toggling
        if self.selected_index < self.current_items.len().saturating_sub(1) {
            self.selected_index += 1;
            self.view_scroll = 0;
            self.trigger_preview();
        }
    }

    pub fn scroll_down(&mut self) {
        self.view_scroll = self.view_scroll.saturating_add(5);
    }

    pub fn scroll_up(&mut self) {
        self.view_scroll = self.view_scroll.saturating_sub(5);
    }

    pub fn trigger_preview(&mut self) {
        if self.current_items.is_empty() || self.selected_index >= self.current_items.len() {
            return;
        }

        let node = self.current_items[self.selected_index].clone();

        // Check cache first
        if let Some(cached) = self.preview_cache.get(&node) {
            self.preview_content = cached.clone();
            return;
        }

        match &node {
            TuiNode::LocalDir(_) => {
                self.preview_content = "Directory — press → to enter.".to_string();
            }
            TuiNode::LocalFile(path) => {
                // Direct file read for preview — keep it sync for speed
                self.preview_content = if let Ok(content) = std::fs::read_to_string(path) {
                    content.chars().take(5000).collect()
                } else if let Ok(bytes) = std::fs::read(path) {
                    format!(
                        "[Binary file: {} bytes]\n{}",
                        bytes.len(),
                        bytes
                            .iter()
                            .take(256)
                            .map(|b| format!("{:02X}", b))
                            .collect::<Vec<_>>()
                            .chunks(16)
                            .map(|chunk| chunk.join(" "))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                } else {
                    "This one is too gristly! (Binary or unreadable file)".to_string()
                }
            }
            TuiNode::WebDir {
                #[allow(unused_variables)]
                url,
                title: _,
            } => {
                #[cfg(feature = "web")]
                {
                    self.preview_content = "Fetching web preview from network...".to_string();
                    let tx = self.tx_msgs.clone();
                    let url_c = url.clone();
                    thread::spawn(move || {
                        if let Ok(res) = reqwest::blocking::Client::builder()
                            .use_rustls_tls()
                            .build()
                            .unwrap()
                            .get(&url_c)
                            .send()
                        {
                            if !res.status().is_success() {
                                let _ = tx.send(TuiMessage::PreviewLoadError(
                                    node,
                                    format!("HTTP Error: {}", res.status()),
                                ));
                                return;
                            }
                            if let Ok(html) = res.text() {
                                let gobbler = filegoblin_core::parsers::html::HtmlGobbler {
                                    source_url: Some(url_c.clone()),
                                    ..Default::default()
                                };
                                let opts =
                                    filegoblin_core::parsers::gobble::ParseOptions::default();
                                match filegoblin_core::parsers::gobble::Gobble::gobble_str(
                                    &gobbler, &html, &opts,
                                ) {
                                    Ok(md) => {
                                        let _ = tx.send(TuiMessage::PreviewLoaded(node, md));
                                    }
                                    Err(e) => {
                                        let _ = tx.send(TuiMessage::PreviewLoadError(
                                            node,
                                            format!("{}", e),
                                        ));
                                    }
                                }
                            }
                        } else {
                            let _ = tx.send(TuiMessage::PreviewLoadError(
                                node,
                                "Network request failed".to_string(),
                            ));
                        }
                    });
                }
                #[cfg(not(feature = "web"))]
                {
                    self.preview_content = "Web previews require the 'web' feature.".to_string();
                }
            }
            TuiNode::TweetNode { text_preview, .. } => {
                self.preview_content = text_preview.clone();
            }
        }
    }
}
