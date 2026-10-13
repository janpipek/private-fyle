//! Window titles shared by native decorations and themed headers.

/// Formats an editor title, keeping unsaved changes visible in the header.
pub fn editor_title(name: &str, modified: bool) -> String {
    let marker = if modified { "● " } else { "" };
    format!("{marker}{name} — Fyle")
}

#[cfg(test)]
mod tests {
    use super::editor_title;

    #[test]
    fn saved_document_has_an_unmarked_title() {
        assert_eq!(editor_title("readme.md", false), "readme.md — Fyle");
    }

    #[test]
    fn unsaved_document_retains_its_name_and_gets_a_marker() {
        assert_eq!(editor_title("zażółć.rs", true), "● zażółć.rs — Fyle");
    }
}
