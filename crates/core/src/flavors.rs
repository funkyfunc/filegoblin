use anyhow::Result;

#[derive(Debug, Clone, PartialEq)]
pub enum Flavor {
    Human,
    Anthropic,
    Gpt,
    Gemini,
}

impl std::str::FromStr for Flavor {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "human" => Ok(Flavor::Human),
            "anthropic" => Ok(Flavor::Anthropic),
            "gpt" => Ok(Flavor::Gpt),
            "gemini" => Ok(Flavor::Gemini),
            _ => anyhow::bail!(
                "Invalid flavor: {}. Supported flavors: human, anthropic, gpt, gemini",
                s
            ),
        }
    }
}

pub fn format_output(flavor: &Flavor, filename: &str, content: &str) -> String {
    format!(
        "{}{}{}",
        format_header(flavor, filename, content.len()),
        content.trim(),
        format_footer(flavor)
    )
}

pub fn format_header(flavor: &Flavor, filename: &str, approx_len: usize) -> String {
    match flavor {
        Flavor::Human => String::new(),
        Flavor::Anthropic => format!("<file path=\"{}\">\n<content>\n", filename),
        Flavor::Gpt => {
            let tokens = approx_len / 4;
            format!(
                "---\nfile: {}\ntokens: {}\nmode: full\n---\n```\n",
                filename, tokens
            )
        }
        Flavor::Gemini => String::new(),
    }
}

pub fn format_footer(flavor: &Flavor) -> String {
    match flavor {
        Flavor::Human => "\n".to_string(),
        Flavor::Anthropic => "\n</content>\n</file>\n".to_string(),
        Flavor::Gpt => "\n```\n".to_string(),
        Flavor::Gemini => "\n".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_flavor() {
        let content = "Hello World";
        let result = format_output(&Flavor::Human, "test.txt", content);
        assert_eq!(result, "Hello World\n");
    }

    #[test]
    fn test_anthropic_flavor() {
        let content = "Hello World";
        let result = format_output(&Flavor::Anthropic, "test.txt", content);
        assert!(result.contains("<file path=\"test.txt\">"));
        assert!(result.contains("<content>\nHello World\n</content>"));
    }

    #[test]
    fn test_gpt_flavor() {
        let content = "Hello World";
        let result = format_output(&Flavor::Gpt, "test.txt", content);
        assert!(result.contains("file: test.txt"));
        assert!(result.contains("```\nHello World\n```"));
    }

    #[test]
    fn test_gemini_flavor() {
        let content = "Hello World";
        let result = format_output(&Flavor::Gemini, "test.txt", content);
        // Gemini flavor now just trims content (FILE_START header is added upstream by lib.rs)
        assert_eq!(result, "Hello World\n");
    }
}
