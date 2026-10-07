//! ODT (OpenDocument text) reader for the document preview.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Seek};

use roxmltree::Node;

use super::{Block, Document, MAX_COLS, MAX_ROWS, Run, parse_xml, push_text, read_entry};

const TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";

fn is(node: Node, ns: &str, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(ns) && node.tag_name().name() == name
}

#[derive(Clone, Copy, Default)]
struct Fmt {
    bold: bool,
    italic: bool,
}

#[derive(Default)]
struct Styles {
    /// Automatic text and paragraph styles by name.
    fmt: HashMap<String, Fmt>,
    /// Paragraph styles that start a new page.
    page_break: HashSet<String>,
    /// List style name to "level is numbered", indexed by level - 1.
    lists: HashMap<String, Vec<bool>>,
}

impl Styles {
    fn parse(root: Node) -> Self {
        let mut styles = Self::default();
        let Some(auto) = root.children().find(|n| is(*n, OFFICE, "automatic-styles")) else {
            return styles;
        };
        for node in auto.children().filter(Node::is_element) {
            if is(node, STYLE, "style") {
                let Some(name) = node.attribute((STYLE, "name")) else {
                    continue;
                };
                for props in node.children().filter(Node::is_element) {
                    if is(props, STYLE, "text-properties") {
                        let bold = props.attribute((FO, "font-weight")).is_some_and(|w| {
                            w == "bold" || w.parse::<u32>().is_ok_and(|n| n >= 600)
                        });
                        let italic = matches!(
                            props.attribute((FO, "font-style")),
                            Some("italic" | "oblique")
                        );
                        styles.fmt.insert(name.to_string(), Fmt { bold, italic });
                    } else if is(props, STYLE, "paragraph-properties")
                        && props.attribute((FO, "break-before")) == Some("page")
                    {
                        styles.page_break.insert(name.to_string());
                    }
                }
            } else if is(node, TEXT, "list-style")
                && let Some(name) = node.attribute((STYLE, "name"))
            {
                let mut levels = Vec::new();
                for level in node.children().filter(Node::is_element) {
                    let index = level
                        .attribute((TEXT, "level"))
                        .and_then(|l| l.parse::<usize>().ok())
                        .unwrap_or(1)
                        .clamp(1, 16)
                        - 1;
                    if levels.len() <= index {
                        levels.resize(index + 1, false);
                    }
                    levels[index] = is(level, TEXT, "list-level-style-number");
                }
                styles.lists.insert(name.to_string(), levels);
            }
        }
        styles
    }

    fn style_fmt(&self, node: Node) -> Fmt {
        node.attribute((TEXT, "style-name"))
            .and_then(|n| self.fmt.get(n))
            .copied()
            .unwrap_or_default()
    }
}

const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";

pub fn parse<R: Read + Seek>(archive: &mut zip::ZipArchive<R>) -> io::Result<Document> {
    let Some(xml) = read_entry(archive, "content.xml")? else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not an OpenDocument text file (content.xml is missing)",
        ));
    };
    let xml = parse_xml(&xml, "content.xml")?;
    let root = xml.root_element();
    let mut reader = Reader {
        styles: Styles::parse(root),
        doc: Document::default(),
    };
    if let Some(text) = root
        .children()
        .find(|n| is(*n, OFFICE, "body"))
        .and_then(|b| b.children().find(|n| is(*n, OFFICE, "text")))
    {
        reader.blocks(text);
    }
    Ok(reader.doc)
}

struct Reader {
    styles: Styles,
    doc: Document,
}

impl Reader {
    fn blocks(&mut self, parent: Node) {
        for node in parent.children().filter(Node::is_element) {
            if self.doc.is_full() {
                return;
            }
            let ns = node.tag_name().namespace();
            match (ns, node.tag_name().name()) {
                (Some(TEXT), "h") => {
                    let level = node
                        .attribute((TEXT, "outline-level"))
                        .and_then(|l| l.parse::<u8>().ok())
                        .unwrap_or(1)
                        .clamp(1, 6);
                    let runs = self.inline_runs(node);
                    self.break_before(node);
                    self.doc.push(Block::Heading { level, runs });
                }
                (Some(TEXT), "p") => {
                    let runs = self.inline_runs(node);
                    self.break_before(node);
                    self.doc.push(Block::Paragraph(runs));
                }
                (Some(TEXT), "list") => self.list(node, 0, None),
                (Some(TEXT), "tracked-changes") => {}
                (Some(TABLE), "table") => self.table(node),
                // Sections, indexes and other containers.
                (Some(TEXT), _) => self.blocks(node),
                _ => {}
            }
        }
    }

    fn break_before(&mut self, node: Node) {
        if node
            .attribute((TEXT, "style-name"))
            .is_some_and(|s| self.styles.page_break.contains(s))
        {
            self.doc.push(Block::Separator);
        }
    }

    fn list(&mut self, list: Node, depth: usize, inherited: Option<&str>) {
        let style = list.attribute((TEXT, "style-name")).or(inherited);
        let ordered = style
            .and_then(|s| self.styles.lists.get(s))
            .and_then(|levels| levels.get(depth))
            .copied()
            .unwrap_or(false);
        for item in list
            .children()
            .filter(|n| is(*n, TEXT, "list-item") || is(*n, TEXT, "list-header"))
        {
            let mut first = true;
            for content in item.children().filter(Node::is_element) {
                if self.doc.is_full() {
                    return;
                }
                if is(content, TEXT, "p") || is(content, TEXT, "h") {
                    let runs = self.inline_runs(content);
                    // Further paragraphs of one item continue it without a marker.
                    self.doc.push(if first {
                        Block::ListItem {
                            level: depth,
                            ordered,
                            runs,
                        }
                    } else {
                        Block::Paragraph(runs)
                    });
                    first = false;
                } else if is(content, TEXT, "list") {
                    self.list(content, depth + 1, style);
                } else {
                    self.blocks_of(content);
                }
            }
        }
    }

    /// Handles one block-level node that is not a paragraph (a table in a list).
    fn blocks_of(&mut self, node: Node) {
        if is(node, TABLE, "table") {
            self.table(node);
        }
    }

    fn table(&mut self, table: Node) {
        let mut rows = Vec::new();
        let mut cut = false;
        self.rows(table, &mut rows, &mut cut);
        self.doc.push(Block::Table(rows));
        if cut {
            self.doc.truncated = true;
        }
    }

    fn rows(&self, node: Node, rows: &mut Vec<Vec<String>>, cut: &mut bool) {
        for item in node.children().filter(Node::is_element) {
            if item.tag_name().namespace() != Some(TABLE) {
                continue;
            }
            match item.tag_name().name() {
                "table-header-rows" | "table-row-group" | "table-rows" => {
                    self.rows(item, rows, cut)
                }
                "table-row" => {
                    if rows.len() >= MAX_ROWS {
                        *cut = true;
                        return;
                    }
                    let mut row = Vec::new();
                    for cell in item.children().filter(Node::is_element) {
                        let covered = is(cell, TABLE, "covered-table-cell");
                        if !covered && !is(cell, TABLE, "table-cell") {
                            continue;
                        }
                        let count = |name: &str| {
                            cell.attribute((TABLE, name))
                                .and_then(|v| v.parse::<usize>().ok())
                                .unwrap_or(1)
                                .clamp(1, MAX_COLS)
                        };
                        let text = if covered {
                            String::new()
                        } else {
                            self.cell_text(cell)
                        };
                        let repeat = count("number-columns-repeated");
                        for _ in 0..repeat {
                            row.push(text.clone());
                            row.extend(std::iter::repeat_n(
                                String::new(),
                                count("number-columns-spanned") - 1,
                            ));
                        }
                    }
                    while row.last().is_some_and(String::is_empty) {
                        row.pop();
                    }
                    row.truncate(MAX_COLS);
                    let repeat = item
                        .attribute((TABLE, "number-rows-repeated"))
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1)
                        .min(MAX_ROWS);
                    if !(row.is_empty() && repeat > 1) {
                        for _ in 0..repeat.min(MAX_ROWS - rows.len()) {
                            rows.push(row.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn cell_text(&self, cell: Node) -> String {
        let mut parts = Vec::new();
        for p in cell.descendants().filter(|n| {
            (is(*n, TEXT, "p") || is(*n, TEXT, "h"))
                && !n
                    .ancestors()
                    .skip(1)
                    .any(|a| is(a, TEXT, "p") || is(a, TEXT, "h"))
        }) {
            let text: String = self
                .inline_runs(p)
                .iter()
                .map(|r| r.text.as_str())
                .collect();
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !text.is_empty() {
                parts.push(text);
            }
        }
        parts.join(" ")
    }

    fn inline_runs(&self, p: Node) -> Vec<Run> {
        let mut runs = Vec::new();
        self.inline(p, self.styles.style_fmt(p), &mut runs);
        runs
    }

    fn inline(&self, node: Node, fmt: Fmt, out: &mut Vec<Run>) {
        for item in node.children() {
            if item.is_text() {
                let text = item.text().unwrap_or("");
                // Whitespace collapses to one space, none at the start.
                let at_space = out
                    .last()
                    .is_none_or(|r| r.text.ends_with([' ', '\n', '\t']));
                let mut collapsed = String::new();
                let mut last_space = at_space;
                for ch in text.chars() {
                    if ch.is_whitespace() {
                        if !last_space {
                            collapsed.push(' ');
                        }
                        last_space = true;
                    } else {
                        collapsed.push(ch);
                        last_space = false;
                    }
                }
                push_text(out, &collapsed, fmt.bold, fmt.italic);
                continue;
            }
            if !item.is_element() {
                continue;
            }
            let ns = item.tag_name().namespace();
            match (ns, item.tag_name().name()) {
                (Some(TEXT), "span") => {
                    let own = self.styles.style_fmt(item);
                    let merged = Fmt {
                        bold: fmt.bold || own.bold,
                        italic: fmt.italic || own.italic,
                    };
                    self.inline(item, merged, out);
                }
                (Some(TEXT), "s") => {
                    let count = item
                        .attribute((TEXT, "c"))
                        .and_then(|c| c.parse::<usize>().ok())
                        .unwrap_or(1)
                        .min(256);
                    push_text(out, &" ".repeat(count), fmt.bold, fmt.italic);
                }
                (Some(TEXT), "tab") => push_text(out, "\t", fmt.bold, fmt.italic),
                (Some(TEXT), "line-break") => push_text(out, "\n", fmt.bold, fmt.italic),
                (Some(TEXT), "note" | "tracked-changes") | (Some(OFFICE), "annotation") => {}
                (Some(DRAW), "frame") => {
                    if item.children().any(|c| is(c, DRAW, "image")) {
                        let name = item.attribute((DRAW, "name")).unwrap_or("image");
                        push_text(out, &format!("[image: {name}]"), false, true);
                    }
                }
                _ => self.inline(item, fmt, out),
            }
        }
    }
}
