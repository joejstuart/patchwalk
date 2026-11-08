use std::{collections::HashMap, path::Path};
use syntect::{
    easy::HighlightLines,
    highlighting::{Style, ThemeSet},
    parsing::SyntaxSet,
};

/// Highlight once at load time; navigation only reads the cached tokens.
pub type HighlightedFiles = HashMap<String, Vec<Vec<(Style, String)>>>;
pub struct SourceCache {
    pub files: HighlightedFiles,
    pub after_files: HighlightedFiles,
}

impl SourceCache {
    pub fn new(files: &HashMap<String, Vec<String>>, after: &HashMap<String, Vec<String>>) -> Self {
        let syntaxes = SyntaxSet::load_defaults_nonewlines();
        let themes = ThemeSet::load_defaults();
        let mut theme = themes.themes["base16-ocean.dark"].clone();
        // Softer blue identifiers keep the warm accent distinct for proposals.
        for rule in &mut theme.scopes {
            if let Some(color) = &mut rule.style.foreground
                && (color.r, color.g, color.b) == (191, 97, 106)
            {
                (color.r, color.g, color.b) = (142, 183, 212);
            }
        }
        let highlight_files = |files: &HashMap<String, Vec<String>>| -> HighlightedFiles {
            files
                .iter()
                .map(|(path, lines)| {
                    let extension = Path::new(path)
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    let syntax = syntaxes
                        .find_syntax_by_extension(extension)
                        // The bundled syntax set has JavaScript but no TypeScript grammar.
                        .or_else(|| {
                            matches!(extension, "ts" | "tsx")
                                .then(|| syntaxes.find_syntax_by_extension("js"))
                                .flatten()
                        })
                        .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
                    let mut highlighter = HighlightLines::new(syntax, &theme);
                    let highlighted = lines
                        .iter()
                        .map(|line| {
                            highlighter
                                .highlight_line(line, &syntaxes)
                                .map(|spans| {
                                    spans
                                        .into_iter()
                                        .map(|(style, text)| (style, text.to_owned()))
                                        .collect()
                                })
                                .unwrap_or_else(|_| vec![(Style::default(), line.clone())])
                        })
                        .collect();
                    (path.clone(), highlighted)
                })
                .collect()
        };
        Self {
            files: highlight_files(files),
            after_files: highlight_files(after),
        }
    }
}
