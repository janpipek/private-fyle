use std::fs::File;
use std::io::{self, BufRead, BufReader};

use super::{Content, Previewer, Probe};

/// Rows and columns shown.
const MAX_ROWS: usize = 500;
const MAX_COLS: usize = 40;

/// CSV / TSV as a table; the first row is the header.
pub struct TablePreviewer;

impl Previewer for TablePreviewer {
    fn name(&self) -> &'static str {
        "Table"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && matches!(probe.ext.as_str(), "csv" | "tsv") && probe.looks_like_text()
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let delimiter = if probe.ext == "tsv" { '\t' } else { ',' };
        let mut lines = BufReader::new(File::open(&probe.path)?).lines();
        let mut records = Vec::new();
        let mut truncated = false;
        let mut pending = String::new();
        while let Some(line) = lines.next() {
            let line = line?;
            if !pending.is_empty() {
                pending.push('\n');
            }
            pending.push_str(&line);
            // A quoted field may span lines: wait for balanced quotes.
            if pending.matches('"').count() % 2 == 1 {
                continue;
            }
            if records.len() > MAX_ROWS {
                truncated = true;
                break;
            }
            records.push(parse_record(&pending, delimiter));
            pending.clear();
        }
        let header = if records.is_empty() {
            Vec::new()
        } else {
            records.remove(0)
        };
        let cap = |row: Vec<String>| row.into_iter().take(MAX_COLS).collect::<Vec<_>>();
        Ok(Content::Table {
            header: cap(header),
            rows: records.into_iter().map(cap).collect(),
            truncated,
        })
    }
}

/// One CSV record: fields split on `delimiter`, `"` quoting with `""` escapes.
pub fn parse_record(line: &str, delimiter: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == delimiter && !quoted => fields.push(std::mem::take(&mut field)),
            c => field.push(c),
        }
    }
    fields.push(field);
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_fields() {
        assert_eq!(parse_record("a,b,c", ','), ["a", "b", "c"]);
        assert_eq!(
            parse_record(r#""x, y","say ""hi""",,z"#, ','),
            ["x, y", "say \"hi\"", "", "z"]
        );
        assert_eq!(parse_record("a\tb", '\t'), ["a", "b"]);
        assert_eq!(parse_record("\"line\none\",2", ','), ["line\none", "2"]);
    }
}
