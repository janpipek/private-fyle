//! Word (DOCX) and OpenDocument text (ODT) previews: text and structure only.
//!
//! The archives are parsed into a format-neutral [`Document`] (headings,
//! paragraphs with bold/italic runs, lists, tables, separators) and rendered
//! by converting it to Markdown for the existing Markdown view. Layout,
//! fonts, colours, underline, footnotes, headers and images are not
//! reproduced; images show as `[image: name]` placeholders. Everything is
//! bounded: XML entries are limited when uncompressed (zip bombs) and the
//! document is cut after a number of blocks and characters.

mod docx;
mod odt;

use std::fs::File;
use std::io::{self, BufReader, Read, Seek};

use super::{Content, Previewer, Probe};

/// Largest uncompressed XML entry read (document, styles, numbering).
const MAX_XML: u64 = 8 * 1024 * 1024;
/// Blocks kept before the preview is cut.
const MAX_BLOCKS: usize = 5_000;
/// Characters of text kept before the preview is cut.
const MAX_CHARS: usize = 300_000;
/// Most rows kept per table, and most columns (also bounds merged cells).
const MAX_ROWS: usize = 1_000;
const MAX_COLS: usize = 64;

/// Inline text with the formatting the preview can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
}

/// One block of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Paragraph(Vec<Run>),
    ListItem {
        level: usize,
        ordered: bool,
        runs: Vec<Run>,
    },
    /// Rows of plain-text cells; the first row is shown as the header.
    Table(Vec<Vec<String>>),
    /// Page or section break.
    Separator,
}

/// A parsed document, cut at the preview limits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub truncated: bool,
    chars: usize,
}

fn runs_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

fn blank(runs: &[Run]) -> bool {
    runs.iter().all(|r| r.text.trim().is_empty())
}

impl Document {
    /// True once the limits are reached; parsers stop then.
    pub fn is_full(&self) -> bool {
        self.truncated
    }

    /// Adds a block unless it is empty or the limits are reached. Text-less
    /// paragraphs are dropped; the first block over a limit marks the
    /// document truncated.
    pub fn push(&mut self, block: Block) {
        if self.truncated {
            return;
        }
        let chars = match &block {
            Block::Heading { runs, .. } | Block::Paragraph(runs) | Block::ListItem { runs, .. } => {
                if blank(runs) {
                    return;
                }
                runs_text(runs).chars().count()
            }
            Block::Table(rows) => {
                if rows.iter().flatten().all(|c| c.trim().is_empty()) {
                    return;
                }
                rows.iter().flatten().map(|c| c.chars().count() + 3).sum()
            }
            Block::Separator => {
                if matches!(self.blocks.last(), None | Some(Block::Separator)) {
                    return;
                }
                0
            }
        };
        if self.blocks.len() >= MAX_BLOCKS || self.chars + chars > MAX_CHARS {
            self.truncated = true;
            return;
        }
        self.chars += chars;
        self.blocks.push(block);
    }

    /// Words in headings, paragraphs, list items and table cells.
    pub fn words(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| match b {
                Block::Heading { runs, .. }
                | Block::Paragraph(runs)
                | Block::ListItem { runs, .. } => runs_text(runs).split_whitespace().count(),
                Block::Table(rows) => rows
                    .iter()
                    .flatten()
                    .map(|c| c.split_whitespace().count())
                    .sum(),
                Block::Separator => 0,
            })
            .sum()
    }

    /// Markdown for the shared Markdown view.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let mut counters: Vec<usize> = Vec::new();
        let mut in_list = false;
        for block in &self.blocks {
            if !matches!(block, Block::ListItem { .. }) {
                counters.clear();
            }
            match block {
                Block::Heading { level, runs } => {
                    gap(&mut out, &mut in_list);
                    let plain: Vec<Run> = runs
                        .iter()
                        .map(|r| Run {
                            bold: false,
                            ..r.clone()
                        })
                        .collect();
                    let text = inline(&plain, true, false);
                    out.push_str(&format!("{} {}\n", "#".repeat(*level as usize), text));
                }
                Block::Paragraph(runs) => {
                    gap(&mut out, &mut in_list);
                    out.push_str(&inline(runs, false, true));
                    out.push('\n');
                }
                Block::ListItem {
                    level,
                    ordered,
                    runs,
                } => {
                    if !in_list {
                        gap(&mut out, &mut in_list);
                        in_list = true;
                    }
                    // Markdown lists cannot skip a nesting level.
                    let level = (*level).min(counters.len());
                    counters.truncate(level + 1);
                    counters.resize(level + 1, 0);
                    // A bullet ends any numbering at its level.
                    counters[level] = if *ordered { counters[level] + 1 } else { 0 };
                    out.push_str(&"    ".repeat(level));
                    if *ordered {
                        out.push_str(&format!("{}. ", counters[level]));
                    } else {
                        out.push_str("- ");
                    }
                    out.push_str(&inline(runs, true, false));
                    out.push('\n');
                }
                Block::Table(rows) => {
                    gap(&mut out, &mut in_list);
                    table(&mut out, rows);
                }
                Block::Separator => {
                    gap(&mut out, &mut in_list);
                    out.push_str("---\n");
                }
            }
        }
        out
    }
}

/// Blank line between blocks.
fn gap(out: &mut String, in_list: &mut bool) {
    *in_list = false;
    if !out.is_empty() {
        out.push('\n');
    }
}

fn table(out: &mut String, rows: &[Vec<String>]) {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return;
    }
    for (i, row) in rows.iter().enumerate() {
        out.push('|');
        for c in 0..width {
            let cell = row.get(c).map(String::as_str).unwrap_or("");
            let cell = cell.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push(' ');
            out.push_str(&escape(&cell));
            out.push_str(" |");
        }
        out.push('\n');
        if i == 0 {
            out.push('|');
            out.push_str(&" --- |".repeat(width));
            out.push('\n');
        }
    }
}

/// Backslash-escapes characters that Markdown would interpret.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(
            ch,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '~' | '#' | '&'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Merges neighbouring runs with the same formatting.
fn merged(runs: &[Run]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for run in runs.iter().filter(|r| !r.text.is_empty()) {
        match out.last_mut() {
            Some(last) if last.bold == run.bold && last.italic == run.italic => {
                last.text.push_str(&run.text);
            }
            _ => out.push(run.clone()),
        }
    }
    out
}

/// Runs as Markdown. `soft` turns line breaks into spaces (headings, list
/// items); otherwise they become hard breaks. `lead` escapes a block marker
/// at the start (paragraphs).
fn inline(runs: &[Run], soft: bool, lead: bool) -> String {
    let mut out = String::new();
    for run in merged(runs) {
        let text = run.text.replace('\t', "\u{2003}").replace('\r', "");
        let text = if soft { text.replace('\n', " ") } else { text };
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                out.push_str("  \n");
            }
            let core = line.trim();
            if core.is_empty() {
                out.push_str(line);
                continue;
            }
            let start = line.len() - line.trim_start().len();
            let end = start + core.len();
            let mark = match (run.bold, run.italic) {
                (true, true) => "***",
                (true, false) => "**",
                (false, true) => "*",
                (false, false) => "",
            };
            out.push_str(&line[..start]);
            out.push_str(mark);
            out.push_str(&escape(core));
            out.push_str(mark);
            out.push_str(&line[end..]);
        }
    }
    let out = out.trim().to_string();
    if lead { escape_lead(out) } else { out }
}

/// Escapes what could turn a paragraph into a list or rule.
fn escape_lead(text: String) -> String {
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && matches!(text[digits..].chars().next(), Some('.' | ')')) {
        return format!("{}\\{}", &text[..digits], &text[digits..]);
    }
    if text.starts_with(['-', '+', '=']) {
        return format!("\\{text}");
    }
    text
}

/// Reads a zip entry as text, refusing entries over `MAX_XML` uncompressed
/// (by declared size and by what actually decompresses).
fn read_entry<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> io::Result<Option<String>> {
    let entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(io::Error::other(format!("{name}: {e}"))),
    };
    let too_large = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{name} is too large to preview"),
        )
    };
    if entry.size() > MAX_XML {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    entry.take(MAX_XML + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_XML {
        return Err(too_large());
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// Parses XML without DTDs (no entity expansion) and with a node limit.
fn parse_xml<'a>(text: &'a str, name: &str) -> io::Result<roxmltree::Document<'a>> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: 2_000_000,
        ..Default::default()
    };
    roxmltree::Document::parse_with_options(text, options)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{name}: {e}")))
}

/// Appends text to `runs`.
fn push_text(runs: &mut Vec<Run>, text: &str, bold: bool, italic: bool) {
    if !text.is_empty() {
        runs.push(Run {
            text: text.to_string(),
            bold,
            italic,
        });
    }
}

/// Parses a DOCX or ODT archive (`odt` selects the format) into a document.
pub fn parse_archive<R: Read + Seek>(mut reader: R, odt: bool) -> io::Result<Document> {
    let mut head = [0u8; 4];
    let n = reader.read(&mut head)?;
    reader.rewind()?;
    if head[..n] == [0xD0, 0xCF, 0x11, 0xE0] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "encrypted or legacy binary document (not supported)",
        ));
    }
    let mut archive = zip::ZipArchive::new(reader).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("not a valid document archive: {e}"),
        )
    })?;
    if odt {
        odt::parse(&mut archive)
    } else {
        docx::parse(&mut archive)
    }
}

/// Word and OpenDocument text documents shown as formatted text.
pub struct DocumentPreviewer;

impl Previewer for DocumentPreviewer {
    fn name(&self) -> &'static str {
        "Document"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir
            && matches!(
                probe.ext.as_str(),
                "docx" | "docm" | "dotx" | "dotm" | "odt" | "ott"
            )
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let odt = matches!(probe.ext.as_str(), "odt" | "ott");
        let doc = parse_archive(BufReader::new(File::open(&probe.path)?), odt)?;
        let mut text = format!("*{} words*\n\n", doc.words());
        text.push_str(&doc.to_markdown());
        if doc.truncated {
            text.push_str("\n*Preview truncated.*\n");
        }
        Ok(Content::Markdown { text })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::Path;

    use tempfile::TempDir;

    use super::{Block, DocumentPreviewer, MAX_BLOCKS, MAX_XML, parse_archive};
    use crate::preview::{Content, Previewer, Probe};

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    /// Writes a zip with the given entries.
    fn zip_file(dir: &TempDir, name: &str, entries: &[(&str, &str)]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("create"));
        let options = zip::write::SimpleFileOptions::default();
        for (entry, body) in entries {
            zip.start_file(*entry, options).expect("start");
            zip.write_all(body.as_bytes()).expect("write");
        }
        zip.finish().expect("finish");
        path
    }

    fn docx(dir: &TempDir, body: &str, extra: &[(&str, &str)]) -> std::path::PathBuf {
        let xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>"#
        );
        let mut entries = vec![("word/document.xml", xml.as_str())];
        entries.extend_from_slice(extra);
        zip_file(dir, "a.docx", &entries)
    }

    fn odt(dir: &TempDir, auto_styles: &str, body: &str) -> std::path::PathBuf {
        let xml = format!(
            r#"<?xml version="1.0"?><office:document-content
 xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0"
 xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"
 xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"
 xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0"
 xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"
 xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0">
 <office:automatic-styles>{auto_styles}</office:automatic-styles>
 <office:body><office:text>{body}</office:text></office:body></office:document-content>"#
        );
        zip_file(dir, "a.odt", &[("content.xml", &xml)])
    }

    fn markdown(path: &Path) -> String {
        let probe = Probe::read(path).expect("probe");
        match DocumentPreviewer.load(&probe).expect("load") {
            Content::Markdown { text } => text,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn parse(path: &Path, odt: bool) -> std::io::Result<super::Document> {
        parse_archive(std::fs::File::open(path).expect("open"), odt)
    }

    #[test]
    fn docx_headings_formatting_and_skipped_deletions() {
        let dir = TempDir::new().expect("tmp");
        let path = docx(
            &dir,
            r#"<w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Intro</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">Plain </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r>
<w:r><w:rPr><w:i/></w:rPr><w:t xml:space="preserve"> it_a</w:t></w:r>
<w:del><w:r><w:delText>gone</w:delText></w:r></w:del><w:ins><w:r><w:t>!</w:t></w:r></w:ins>
<w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t></w:r></w:p>
<w:sdt><w:sdtContent><w:p><w:r><w:t>In sdt</w:t></w:r></w:p></w:sdtContent></w:sdt>
<w:p><w:r><w:drawing><wp:inline xmlns:wp="x"><wp:docPr id="1" name="Logo"/></wp:inline></w:drawing></w:r></w:p>"#,
            &[],
        );
        let md = markdown(&path);
        assert!(md.starts_with("*"), "{md}");
        assert!(md.contains("## Intro\n"), "{md}");
        assert!(md.contains("Plain **bold**"), "{md}");
        assert!(md.contains("*it\\_a*"), "{md}");
        assert!(md.contains("!a\u{2003}b  \nc"), "{md}");
        assert!(!md.contains("gone"));
        assert!(md.contains("In sdt"));
        assert!(md.contains("\\[image: Logo\\]"), "{md}");
    }

    #[test]
    fn docx_named_heading_styles_and_page_breaks() {
        let dir = TempDir::new().expect("tmp");
        let path = docx(
            &dir,
            r#"<w:p><w:pPr><w:pStyle w:val="S1"/></w:pPr><w:r><w:t>Chapter</w:t></w:r></w:p>
<w:p><w:r><w:t>One</w:t></w:r><w:r><w:br w:type="page"/></w:r></w:p>
<w:p><w:r><w:t>Two</w:t></w:r></w:p>"#,
            &[(
                "word/styles.xml",
                &format!(
                    r#"<w:styles xmlns:w="{W}"><w:style w:styleId="S1"><w:name w:val="heading 1"/></w:style></w:styles>"#
                ),
            )],
        );
        let doc = parse(&path, false).expect("parse");
        assert!(matches!(doc.blocks[0], Block::Heading { level: 1, .. }));
        assert!(doc.blocks.contains(&Block::Separator));
        assert!(markdown(&path).contains("\n---\n"));
    }

    #[test]
    fn docx_lists_nest_and_number() {
        let dir = TempDir::new().expect("tmp");
        let item = |num: u32, lvl: u32, text: &str| {
            format!(
                r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{lvl}"/><w:numId w:val="{num}"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
            )
        };
        let body = [
            item(1, 0, "a"),
            item(1, 1, "a1"),
            item(2, 0, "first"),
            item(2, 0, "second"),
        ]
        .concat();
        let numbering = format!(
            r#"<w:numbering xmlns:w="{W}">
<w:abstractNum w:abstractNumId="5"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum>
<w:abstractNum w:abstractNumId="6"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="5"/></w:num>
<w:num w:numId="2"><w:abstractNumId w:val="6"/></w:num></w:numbering>"#
        );
        let path = docx(&dir, &body, &[("word/numbering.xml", &numbering)]);
        let md = markdown(&path);
        assert!(md.contains("- a\n    - a1\n"), "{md}");
        assert!(md.contains("1. first\n2. second\n"), "{md}");
    }

    #[test]
    fn docx_tables_with_merged_cells() {
        let dir = TempDir::new().expect("tmp");
        let path = docx(
            &dir,
            r#"<w:tbl><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>Head</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>a|b</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
            &[],
        );
        let md = markdown(&path);
        assert!(
            md.contains("| Head |  |\n| --- | --- |\n| a\\|b | c |"),
            "{md}"
        );
    }

    #[test]
    fn docx_truncates_and_reports() {
        let dir = TempDir::new().expect("tmp");
        let body = "<w:p><w:r><w:t>x</w:t></w:r></w:p>".repeat(MAX_BLOCKS + 10);
        let path = docx(&dir, &body, &[]);
        let doc = parse(&path, false).expect("parse");
        assert_eq!(doc.blocks.len(), MAX_BLOCKS);
        assert!(doc.truncated);
        assert!(markdown(&path).contains("Preview truncated"));
    }

    #[test]
    fn oversized_xml_is_refused() {
        let dir = TempDir::new().expect("tmp");
        let big = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>"#,
            "a".repeat(MAX_XML as usize)
        );
        let path = zip_file(&dir, "big.docx", &[("word/document.xml", &big)]);
        let err = parse(&path, false).expect_err("too large");
        assert!(err.to_string().contains("too large"), "{err}");
    }

    #[test]
    fn corrupt_and_foreign_input_gives_messages() {
        let dir = TempDir::new().expect("tmp");
        let junk = dir.path().join("junk.docx");
        std::fs::write(&junk, b"not a zip at all").expect("write");
        let err = parse(&junk, false).expect_err("junk");
        assert!(err.to_string().contains("not a valid"), "{err}");

        let ole = dir.path().join("old.docx");
        std::fs::write(&ole, [0xD0, 0xCF, 0x11, 0xE0, 0, 0, 0, 0]).expect("write");
        let err = parse(&ole, false).expect_err("ole");
        assert!(err.to_string().contains("encrypted or legacy"), "{err}");

        let other = zip_file(&dir, "other.docx", &[("hello.txt", "hi")]);
        let err = parse(&other, false).expect_err("no document.xml");
        assert!(err.to_string().contains("word/document.xml"), "{err}");

        let bad = zip_file(&dir, "bad.docx", &[("word/document.xml", "<w:p>")]);
        assert!(parse(&bad, false).is_err());

        let dtd = zip_file(
            &dir,
            "dtd.docx",
            &[(
                "word/document.xml",
                "<!DOCTYPE a [<!ENTITY x \"y\">]><a>&x;</a>",
            )],
        );
        assert!(parse(&dtd, false).is_err());
    }

    #[test]
    fn odt_headings_spans_lists_and_whitespace() {
        let dir = TempDir::new().expect("tmp");
        let path = odt(
            &dir,
            r#"<style:style style:name="T1" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
<style:style style:name="T2" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>
<text:list-style style:name="L1"><text:list-level-style-bullet text:level="1"/></text:list-style>
<text:list-style style:name="L2"><text:list-level-style-number text:level="1"/></text:list-style>"#,
            r#"<text:h text:outline-level="1">Title</text:h>
<text:p>Hello <text:span text:style-name="T1">bold</text:span><text:s text:c="2"/>and
   <text:span text:style-name="T2">it</text:span><text:tab/>x<text:line-break/>y<text:note><text:note-body><text:p>NOTE</text:p></text:note-body></text:note></text:p>
<text:list text:style-name="L1"><text:list-item><text:p>one</text:p>
 <text:list><text:list-item><text:p>nested</text:p></text:list-item></text:list></text:list-item></text:list>
<text:list text:style-name="L2"><text:list-item><text:p>n1</text:p></text:list-item><text:list-item><text:p>n2</text:p></text:list-item></text:list>
<table:table><table:table-row><table:table-cell><text:p>H1</text:p></table:table-cell><table:table-cell table:number-columns-spanned="2"><text:p>H2</text:p></table:table-cell><table:covered-table-cell/></table:table-row>
<table:table-row><table:table-cell><text:p>r</text:p></table:table-cell></table:table-row></table:table>
<draw:frame draw:name="pic"/>
<text:p><draw:frame draw:name="Photo"><draw:image/></draw:frame></text:p>"#,
        );
        let md = markdown(&path);
        assert!(md.contains("# Title\n"), "{md}");
        assert!(
            md.contains("Hello **bold**  and *it*\u{2003}x  \ny"),
            "{md}"
        );
        assert!(!md.contains("NOTE"));
        assert!(md.contains("- one\n    - nested\n"), "{md}");
        assert!(md.contains("1. n1\n2. n2\n"), "{md}");
        assert!(md.contains("| H1 | H2 |"), "{md}");
        assert!(md.contains("\\[image: Photo\\]"), "{md}");
    }

    #[test]
    fn odt_missing_content_and_deleted_changes() {
        let dir = TempDir::new().expect("tmp");
        let other = zip_file(&dir, "x.odt", &[("mimetype", "text")]);
        let err = parse(&other, true).expect_err("no content");
        assert!(err.to_string().contains("content.xml"), "{err}");

        let path = odt(
            &dir,
            "",
            r#"<text:tracked-changes><text:changed-region><text:deletion><text:p>removed</text:p></text:deletion></text:changed-region></text:tracked-changes><text:p>kept</text:p>"#,
        );
        let md = markdown(&path);
        assert!(md.contains("kept") && !md.contains("removed"), "{md}");
    }

    #[test]
    fn registry_prefers_documents_over_the_zip_listing() {
        let dir = TempDir::new().expect("tmp");
        let registry = crate::preview::Registry::with_defaults();
        for path in [
            docx(&dir, "<w:p><w:r><w:t>Hi</w:t></w:r></w:p>", &[]),
            odt(&dir, "", "<text:p>Hi</text:p>"),
        ] {
            let preview = registry.preview(&path).expect("preview");
            assert_eq!(preview.kind, "Document");
            assert!(matches!(preview.content, Content::Markdown { .. }));
        }
    }
}
