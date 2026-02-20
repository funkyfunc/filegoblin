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
            _ => anyhow::bail!("Invalid flavor: {}. Supported flavors: human, anthropic, gpt, gemini", s),
        }
    }
}

pub fn format_output(flavor: &Flavor, filename: &str, content: &str) -> String {
    match flavor {
        Flavor::Human => {
            format!("{}\n", content.trim())
        }
        Flavor::Anthropic => {
            format!(
                "<file path=\"{}\">\n<content>\n{}\n</content>\n</file>\n",
                filename,
                content.trim()
            )
        }
        Flavor::Gpt => {
            let tokens = content.len() / 4; // Absolute rough estimate
            format!(
                "---\nfile: {}\ntokens: {}\nmode: full\n---\n```\n{}\n```\n",
                filename,
                tokens,
                content.trim()
            )
        }
        Flavor::Gemini => {
            format!(
                "// --- FILE_START: {} ---\n{}\n",
                filename,
                content.trim()
            )
        }
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
        assert!(result.contains("// --- FILE_START: test.txt ---"));
        assert!(result.contains("Hello World"));
    }
}
