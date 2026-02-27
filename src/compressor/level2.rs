use super::TokenTransformer;
use std::borrow::Cow;
use logos::Logos;

#[derive(Logos, Debug, PartialEq)]
enum RustToken {
    #[regex(r"///.*", priority = 11, allow_greedy = true)] // Doc comment
    DocComment,
    #[regex(r"//.*", priority = 10, allow_greedy = true)] // Line comment
    LineComment,
    #[regex(r"/\*([^*]|\*[^/])*\*/", priority = 10)] // Block comment
    BlockComment,
    #[regex(r#""([^"\\]|\\[\s\S])*""#, priority = 5)] // String literal
    StringLiteral,
    #[regex(r"r#*.*#*", allow_greedy = true)] // Raw String Literal (simplified)
    RawStringLiteral,
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", priority = 1)]
    Identifier,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[regex(r"[^{}a-zA-Z0-9_]", priority = 1)] // Catch-all for operators and other punctuation
    Other,
    #[regex(r"[ \t\n\f]+")] // Whitespace token
    Whitespace,
}



pub struct CommentStripper {
    language: String,
}

impl CommentStripper {
    pub fn new(language: &str) -> Self {
        Self { language: language.to_string() }
    }
}

impl TokenTransformer for CommentStripper {
    fn transform<'a>(&self, input: &'a str) -> Cow<'a, str> {
        match self.language.to_lowercase().as_str() {
            "rust" | "rs" | "js" | "ts" | "c" | "cpp" | "java" | "go" => {
               // Only stripping rustline/block comments as proof of concept for Logos. 
               // Extending accurately to all brace languages takes more time, 
               // so we'll treat them roughly the same for this Level 2 implementation.
               let mut result = String::with_capacity(input.len());
               let mut lex = RustToken::lexer(input);
               let mut last_end = 0;

               while let Some(res) = lex.next() {
                   let span = lex.span();
                   // Push any skipped text (like whitespace)
                   if span.start > last_end {
                        result.push_str(&input[last_end..span.start]);
                   }
                   
                   if let Ok(token) = res {
                       match token {
                           RustToken::LineComment | RustToken::BlockComment => {
                               // Strip entirely (do nothing)
                           }
                           RustToken::DocComment => {
                               // Keep doc comments
                               result.push_str(lex.slice());
                           }
                           _ => {
                               // Any other valid matched token is kept
                               result.push_str(lex.slice());
                           }
                       }
                   } else {
                       // Keep error tokens too
                       result.push_str(lex.slice());
                   }
                   last_end = span.end;
               }
               // Grab anything after the last token
               if last_end < input.len() {
                   result.push_str(&input[last_end..]);
               }
               
               // Optionally cleanup double empty lines or whatever
               Cow::Owned(result.replace("\n \n", "\n\n"))
            }

            "python" | "py" | "yaml" | "yml" => {
                // Python and YAML rely on indentation, so we must be extremely careful.
                // For level 2, we just strip pure comments (lines starting with #) to avoid breaking structure.
                let mut result = String::with_capacity(input.len());
                
                for line in input.lines() {
                    let trimmed = line.trim_start();
                    if trimmed.starts_with('#') {
                        // Keep the line but just the indentation to maintain visual structure if needed
                        // Actually, replacing with empty line is safer to drop tokens without breaking blocks
                        result.push('\n');
                    } else {
                        result.push_str(line);
                        result.push('\n');
                    }
                }
                Cow::Owned(result.trim_end().to_string())
            }
            _ => Cow::Borrowed(input)
        }
    }
}


pub struct Minifier;

impl TokenTransformer for Minifier {
    fn transform<'a>(&self, input: &'a str) -> Cow<'a, str> {
        // Attempt JSON minification first
        #[allow(clippy::collapsible_if)]
        if input.trim_start().starts_with('{') || input.trim_start().starts_with('[') {
             if let Ok(value) = serde_json::from_str::<serde_json::Value>(input) {
                 if let Ok(minified) = serde_json::to_string(&value) {
                     return Cow::Owned(minified);
                 }
             }
        }
        
        // Attempt HTML minification
        if input.trim_start().starts_with('<') {
            let mut cfg = minify_html::Cfg::new();
            cfg.minify_css = true;
            cfg.minify_js = true;
            
            let minified = minify_html::minify(input.as_bytes(), &cfg);
            if let Ok(s) = String::from_utf8(minified) {
                return Cow::Owned(s);
            }
        }

        Cow::Borrowed(input)
    }
}
