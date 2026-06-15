#[derive(Debug, Clone, Default)]
pub struct GobbleDocument {
    pub path: String,
    pub content: String,
    pub token_count: Option<usize>,
    pub relevance_score: Option<f32>,
}

impl GobbleDocument {
    pub fn new(path: String, content: String) -> Self {
        Self {
            path,
            content,
            token_count: None,
            relevance_score: None,
        }
    }
}
