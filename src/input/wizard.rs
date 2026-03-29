use crate::ai::PanelContent;
use crate::app::App;

pub fn lookup_wizard_symbol_refs(app: &mut App) {
    // Extract symbol: use the current selected file's first identifier found in the
    // current line content, or fall back to filename-based heuristic.
    // For now use a simple heuristic: extract the identifier at the current diff line.
    let symbol = {
        let tab = app.tab();
        let file = tab.selected_diff_file();
        if let Some(file) = file {
            // Get current line content
            let hunk = file.hunks.get(tab.current_hunk);
            let content = if let Some(h) = hunk {
                if let Some(line_idx) = tab.current_line {
                    h.lines.get(line_idx).map(|l| l.content.as_str())
                } else {
                    h.lines.first().map(|l| l.content.as_str())
                }
            } else {
                None
            };

            // Extract first word (identifier) from line
            if let Some(line) = content {
                let trimmed = line.trim_start_matches(['+', '-', ' ']);
                // Find first Rust/TS-like identifier: letter/underscore followed by word chars
                trimmed
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .find(|s| {
                        !s.is_empty()
                            && s.chars()
                                .next()
                                .is_some_and(|c| c.is_alphabetic() || c == '_')
                    })
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    };

    if symbol.is_empty() {
        app.notify("No symbol found under cursor");
        return;
    }

    let repo_root = app.tab().repo_root.clone();
    let diff_paths: std::collections::HashSet<String> =
        app.tab().files.iter().map(|f| f.path.clone()).collect();

    match crate::git::git_grep_symbol(&repo_root, &symbol) {
        Ok(matches) => {
            if matches.is_empty() {
                app.notify(&format!("No references found for '{}'", symbol));
                return;
            }
            let mut in_diff = Vec::new();
            let mut external = Vec::new();

            for m in matches {
                let entry = crate::app::SymbolRefEntry {
                    file: m.file.clone(),
                    line_num: m.line_num,
                    line_content: m.line_content,
                };
                if diff_paths.contains(&m.file) {
                    in_diff.push(entry);
                } else {
                    external.push(entry);
                }
            }

            let total = in_diff.len() + external.len();
            app.tab_mut().symbol_refs = Some(crate::app::SymbolRefsState {
                symbol: symbol.clone(),
                in_diff,
                external,
                cursor: 0,
            });
            app.tab_mut().panel = Some(PanelContent::SymbolRefs);
            app.notify(&format!("Found {} references to '{}'", total, symbol));
        }
        Err(e) => {
            app.notify(&format!("git grep failed: {}", e));
        }
    }
}
