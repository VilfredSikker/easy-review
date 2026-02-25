/// Simple word-wrap helper.
/// Uses `chars().count()` for the width check so multi-byte UTF-8 strings
/// are measured in characters, not bytes.
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
        let mut current = String::new();
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
            } else if current.is_empty() {
                current = word.to_string();
            } else if current.chars().count() + 1 + word_len <= max_width {
                current.push(' ');
                current.push_str(word);
            } else {
                result.push(current);
                current = word.to_string();
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
