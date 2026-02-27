/// Simple word-wrap helper.
/// Uses `chars().count()` for the width check so multi-byte UTF-8 strings
/// are measured in characters, not bytes.
/// Preserves leading whitespace on the first segment of each line.
// TODO(risk:minor): when max_width == 0 the function returns the whole input string
// unsplit. Callers that pass this output to a fixed-width terminal cell (e.g. a
// Ratatui Paragraph) will then render a line longer than the widget width. The
// safest contract would be to return an empty vec or a single empty string so callers
// get no output rather than overflowing output.
pub(crate) fn word_wrap(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut result = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            result.push(String::new());
            continue;
        }
        // Capture leading whitespace to preserve indentation
        // TODO(risk:medium): line.len() counts bytes but line.trim_start().len() also
        // counts bytes, so indent_len is correct for byte-indexing. However &line[..indent_len]
        // will panic with a byte-boundary panic if the leading whitespace contains
        // multi-byte characters (e.g. a line starting with a U+2003 EM SPACE). Use
        // line.char_indices() or split_at() with a char boundary to be safe.
        let indent_len = line.len() - line.trim_start().len();
        let indent = &line[..indent_len];
        let indent_chars = indent.chars().count();

        let mut current = String::new();
        let mut is_first_segment = true;
        for word in line.split_whitespace() {
            let word_len = word.chars().count();
            if word_len > max_width {
                // Flush current line before breaking the long word
                if !current.is_empty() {
                    result.push(current);
                    current = String::new();
                }
                // Break the word into max_width chunks
                let mut chars = word.chars().peekable();
                while chars.peek().is_some() {
                    let chunk: String = chars.by_ref().take(max_width).collect();
                    result.push(chunk);
                }
                is_first_segment = false;
            } else if current.is_empty() {
                if is_first_segment && indent_chars > 0 {
                    // Prepend original indentation on the first segment
                    current = format!("{}{}", indent, word);
                } else {
                    current = word.to_string();
                }
            } else if current.chars().count() + 1 + word_len <= max_width {
                current.push(' ');
                current.push_str(word);
            } else {
                result.push(current);
                current = word.to_string();
                is_first_segment = false;
            }
        }
        if !current.is_empty() {
            result.push(current);
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_leading_whitespace() {
        let result = word_wrap("    function foo() {", 40);
        assert_eq!(result, vec!["    function foo() {"]);
    }

    #[test]
    fn preserves_tab_indentation() {
        let result = word_wrap("\t\tlet x = 1;", 40);
        assert_eq!(result, vec!["\t\tlet x = 1;"]);
    }

    #[test]
    fn no_indent_unchanged() {
        let result = word_wrap("hello world", 40);
        assert_eq!(result, vec!["hello world"]);
    }

    #[test]
    fn wraps_at_max_width() {
        let result = word_wrap("    aaa bbb ccc", 10);
        // "    aaa" = 7 chars, fits; adding " bbb" = 11, doesn't fit
        assert_eq!(result, vec!["    aaa", "bbb ccc"]);
    }

    #[test]
    fn empty_line_preserved() {
        let result = word_wrap("", 40);
        assert_eq!(result, vec![""]);
    }
}
