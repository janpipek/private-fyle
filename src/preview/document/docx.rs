//! DOCX (Office Open XML WordprocessingML) reader for the document preview.

use std::collections::HashMap;
use std::io::{self, Read, Seek};

use roxmltree::Node;

use super::{Block, Document, MAX_COLS, MAX_ROWS, Run, parse_xml, push_text, read_entry};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const MC: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

fn is(node: Node, ns: &str, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(ns) && node.tag_name().name() == name
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|c| is(*c, W, name))
}

/// A `w:` attribute value.
fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attribute((W, name))
}

/// Whether a toggle property such as `w:b` is on.
fn toggle(props: Option<Node>, name: &str) -> bool {
    props
        .and_then(|p| child(p, name))
        .is_some_and(|n| !matches!(attr(n, "val"), Some("0" | "false" | "off")))
}

/// Numbering definitions: which list levels are numbered rather than bulleted.
#[derive(Default)]
struct Numbering {
    /// `numId` to `abstractNumId`.
    nums: HashMap<String, String>,
    /// `abstractNumId` to the numbered levels.
    ordered: HashMap<String, HashMap<usize, bool>>,
}

impl Numbering {
    fn parse(xml: &str) -> io::Result<Self> {
        let doc = parse_xml(xml, "word/numbering.xml")?;
        let mut numbering = Self::default();
        for node in doc.root_element().children() {
            if is(node, W, "abstractNum") {
                let Some(id) = attr(node, "abstractNumId") else {
                    continue;
                };
                let levels = node
                    .children()
                    .filter(|l| is(*l, W, "lvl"))
                    .filter_map(|l| {
                        let level = attr(l, "ilvl")?.parse().ok()?;
                        let format = attr(child(l, "numFmt")?, "val")?;
                        Some((level, !matches!(format, "bullet" | "none")))
                    })
                    .collect();
                numbering.ordered.insert(id.to_string(), levels);
            } else if is(node, W, "num")
                && let (Some(id), Some(abstract_id)) = (
                    attr(node, "numId"),
                    child(node, "abstractNumId").and_then(|n| attr(n, "val")),
                )
            {
                numbering
                    .nums
                    .insert(id.to_string(), abstract_id.to_string());
            }
        }
        Ok(numbering)
    }

    fn is_ordered(&self, num_id: &str, level: usize) -> bool {
        self.nums
            .get(num_id)
            .and_then(|a| self.ordered.get(a))
            .and_then(|levels| levels.get(&level))
            .copied()
            .unwrap_or(false)
    }
}

struct Reader {
    /// Style id to lowercase style name.
    styles: HashMap<String, String>,
    numbering: Numbering,
    doc: Document,
}

/// Heading level from a style name like "heading 2", "Heading2", "Title".
fn heading_level(name: &str) -> Option<u8> {
    let name: String = name.to_lowercase().split_whitespace().collect();
    if name == "title" {
        return Some(1);
    }
    if name == "subtitle" {
        return Some(2);
    }
    let level: u8 = name.strip_prefix("heading")?.parse().ok()?;
    (1..=9).contains(&level).then_some(level.min(6))
}

pub fn parse<R: Read + Seek>(archive: &mut zip::ZipArchive<R>) -> io::Result<Document> {
    let Some(xml) = read_entry(archive, "word/document.xml")? else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a Word document (word/document.xml is missing)",
        ));
    };
    let mut styles = HashMap::new();
    if let Some(xml) = read_entry(archive, "word/styles.xml")? {
        let doc = parse_xml(&xml, "word/styles.xml")?;
        for style in doc.root_element().children().filter(|s| is(*s, W, "style")) {
            if let (Some(id), Some(name)) = (
                attr(style, "styleId"),
                child(style, "name").and_then(|n| attr(n, "val")),
            ) {
                styles.insert(id.to_string(), name.to_lowercase());
            }
        }
    }
    let numbering = match read_entry(archive, "word/numbering.xml")? {
        Some(xml) => Numbering::parse(&xml)?,
        None => Numbering::default(),
    };
    let doc = parse_xml(&xml, "word/document.xml")?;
    let mut reader = Reader {
        styles,
        numbering,
        doc: Document::default(),
    };
    if let Some(body) = child(doc.root_element(), "body") {
        reader.blocks(body);
    }
    Ok(reader.doc)
}

/// Inline formatting state.
#[derive(Clone, Copy, Default)]
struct Fmt {
    bold: bool,
    italic: bool,
}

impl Reader {
    /// Block-level children of the body, a content control or a revision.
    fn blocks(&mut self, parent: Node) {
        for node in parent.children().filter(Node::is_element) {
            if self.doc.is_full() {
                return;
            }
            if node.tag_name().namespace() != Some(W) {
                continue;
            }
            match node.tag_name().name() {
                "p" => self.paragraph(node),
                "tbl" => self.table(node),
                "sdt" => {
                    if let Some(content) = child(node, "sdtContent") {
                        self.blocks(content);
                    }
                }
                "ins" | "moveTo" | "customXml" | "smartTag" => self.blocks(node),
                _ => {}
            }
        }
    }

    fn style_name(&self, id: &str) -> String {
        self.styles
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_lowercase())
    }

    fn paragraph(&mut self, p: Node) {
        let mut runs = Vec::new();
        let mut page_break = false;
        collect(p, Fmt::default(), &mut runs, &mut page_break);

        let props = child(p, "pPr");
        let style = props
            .and_then(|pr| child(pr, "pStyle"))
            .and_then(|s| attr(s, "val"))
            .map(|id| self.style_name(id))
            .unwrap_or_default();
        let outline = props
            .and_then(|pr| child(pr, "outlineLvl"))
            .and_then(|n| attr(n, "val"))
            .and_then(|v| v.parse::<u8>().ok())
            .filter(|l| *l < 9)
            .map(|l| (l + 1).min(6));
        let numbering = props.and_then(|pr| child(pr, "numPr")).and_then(|num| {
            let id = attr(child(num, "numId")?, "val")?;
            let level = child(num, "ilvl")
                .and_then(|n| attr(n, "val"))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            (id != "0").then(|| (id.to_string(), level))
        });

        let block = if let Some(level) = heading_level(&style).or(outline) {
            Block::Heading { level, runs }
        } else if let Some((id, level)) = numbering {
            let ordered = self.numbering.is_ordered(&id, level);
            Block::ListItem {
                level,
                ordered,
                runs,
            }
        } else if style.starts_with("list bullet") || style.starts_with("list number") {
            Block::ListItem {
                level: 0,
                ordered: style.starts_with("list number"),
                runs,
            }
        } else {
            Block::Paragraph(runs)
        };
        self.doc.push(block);
        if page_break || props.is_some_and(|pr| child(pr, "sectPr").is_some()) {
            self.doc.push(Block::Separator);
        }
    }

    fn table(&mut self, tbl: Node) {
        let mut rows = Vec::new();
        let mut cut = false;
        for tr in tbl.children().filter(|n| is(*n, W, "tr")) {
            if rows.len() >= MAX_ROWS {
                cut = true;
                break;
            }
            let mut row: Vec<String> = Vec::new();
            for tc in tr.children().filter(|n| is(*n, W, "tc")) {
                let props = child(tc, "tcPr");
                let span = props
                    .and_then(|p| child(p, "gridSpan"))
                    .and_then(|n| attr(n, "val"))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, MAX_COLS);
                let continued = props
                    .and_then(|p| child(p, "vMerge"))
                    .is_some_and(|n| attr(n, "val") != Some("restart"));
                let text = if continued {
                    String::new()
                } else {
                    cell_text(tc)
                };
                row.push(text);
                row.extend(std::iter::repeat_n(String::new(), span - 1));
            }
            row.truncate(MAX_COLS);
            rows.push(row);
        }
        self.doc.push(Block::Table(rows));
        if cut {
            self.doc.truncated = true;
        }
    }
}

/// Plain text of a table cell: its paragraphs (nested tables included).
fn cell_text(tc: Node) -> String {
    let mut parts = Vec::new();
    for p in tc.descendants().filter(|n| is(*n, W, "p")) {
        let mut runs = Vec::new();
        collect(p, Fmt::default(), &mut runs, &mut false);
        let text: String = runs.iter().map(|r| r.text.as_str()).collect();
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            parts.push(text);
        }
    }
    parts.join(" ")
}

/// Collects the runs of a paragraph or of an inline wrapper. Deleted text is
/// skipped, insertions, links and content controls are descended into.
fn collect(node: Node, fmt: Fmt, out: &mut Vec<Run>, page_break: &mut bool) {
    for item in node.children().filter(Node::is_element) {
        let ns = item.tag_name().namespace();
        if ns == Some(MC) && item.tag_name().name() == "AlternateContent" {
            if let Some(choice) = item.children().find(|c| is(*c, MC, "Choice")) {
                collect(choice, fmt, out, page_break);
            }
            continue;
        }
        if ns != Some(W) {
            continue;
        }
        match item.tag_name().name() {
            "r" => run(item, fmt, out, page_break),
            "hyperlink" | "ins" | "moveTo" | "smartTag" | "fldSimple" | "customXml" | "bdo"
            | "dir" => collect(item, fmt, out, page_break),
            "sdt" => {
                if let Some(content) = child(item, "sdtContent") {
                    collect(content, fmt, out, page_break);
                }
            }
            _ => {}
        }
    }
}

fn run(r: Node, base: Fmt, out: &mut Vec<Run>, page_break: &mut bool) {
    let props = child(r, "rPr");
    let fmt = Fmt {
        bold: base.bold || toggle(props, "b"),
        italic: base.italic || toggle(props, "i"),
    };
    for item in r.children().filter(Node::is_element) {
        if item.tag_name().namespace() != Some(W) {
            continue;
        }
        match item.tag_name().name() {
            "t" => push_text(out, item.text().unwrap_or(""), fmt.bold, fmt.italic),
            "tab" => push_text(out, "\t", fmt.bold, fmt.italic),
            "noBreakHyphen" => push_text(out, "-", fmt.bold, fmt.italic),
            "br" if attr(item, "type") == Some("page") => *page_break = true,
            "br" | "cr" => push_text(out, "\n", fmt.bold, fmt.italic),
            "drawing" | "pict" | "object" => {
                let name = item
                    .descendants()
                    .find(|n| n.is_element() && n.tag_name().name() == "docPr")
                    .and_then(|n| {
                        n.attribute("name")
                            .filter(|s| !s.is_empty())
                            .or(n.attribute("descr"))
                    })
                    .unwrap_or("image");
                push_text(out, &format!("[image: {name}]"), false, true);
            }
            _ => {}
        }
    }
}
