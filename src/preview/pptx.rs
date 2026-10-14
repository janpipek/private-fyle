//! Bounded, read-only text model for PowerPoint presentations.

use std::io::{self, Read, Seek};
use std::path::Path;

use roxmltree::{Document, Node};

use super::{Content, Previewer, Probe};

const MAX_FILE: u64 = 32 * 1024 * 1024;
const MAX_ENTRY: u64 = 2 * 1024 * 1024;
const MAX_TOTAL_XML: usize = 16 * 1024 * 1024;
const MAX_SLIDES: usize = 300;
/// UTF-8 bytes of extracted slide text.
const MAX_TEXT: usize = 300_000;
const PRESENTATION_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const DRAWING_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// One ordered slide with its extracted text and named image placeholders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slide {
    /// Non-empty text paragraphs in their slide order.
    pub text: Vec<String>,
    /// Basenames of embedded images, displayed as placeholders.
    pub images: Vec<String>,
}

/// Bounded text outline of a presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presentation {
    /// Slides in the order listed by `ppt/presentation.xml`.
    pub slides: Vec<Slide>,
    /// True when the text limit omitted later paragraphs or slides.
    pub truncated: bool,
}

/// F3 previewer for PowerPoint Open XML presentations.
pub struct PresentationPreviewer;

impl Previewer for PresentationPreviewer {
    fn name(&self) -> &'static str {
        "PowerPoint"
    }
    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && matches!(probe.ext.as_str(), "pptx" | "pptm")
    }
    fn load(&self, probe: &Probe) -> io::Result<Content> {
        Ok(Content::Presentation(std::sync::Arc::new(
            read_presentation(&probe.path)?,
        )))
    }
}

/// Reads a presentation without rendering or executing embedded content.
pub fn read_presentation(path: &Path) -> io::Result<Presentation> {
    let file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size > MAX_FILE {
        return Err(invalid("presentation exceeds the 32 MiB preview limit"));
    }
    parse_archive(file)
}

fn parse_archive<R: Read + Seek>(reader: R) -> io::Result<Presentation> {
    let mut zip = zip::ZipArchive::new(reader)
        .map_err(|e| invalid(&format!("not a valid presentation archive: {e}")))?;
    if zip.len() > 3_000 {
        return Err(invalid("presentation has too many archive entries"));
    }
    let mut total = 0usize;
    let presentation = read_xml(&mut zip, "ppt/presentation.xml", &mut total)?
        .ok_or_else(|| invalid("presentation.xml is missing"))?;
    let rels = read_xml(&mut zip, "ppt/_rels/presentation.xml.rels", &mut total)?
        .ok_or_else(|| invalid("presentation relationships are missing"))?;
    let rel_doc = xml(&rels, "presentation relationships")?;
    let relationships = rel_doc
        .root_element()
        .children()
        .filter(|n| n.is_element() && n.tag_name().namespace() == Some(PACKAGE_REL_NS))
        .filter_map(|n| {
            Some((
                n.attribute("Id")?.to_string(),
                n.attribute("Target")?.to_string(),
            ))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let doc = xml(&presentation, "presentation.xml")?;
    let ids = doc
        .descendants()
        .filter(|n| {
            n.is_element()
                && n.tag_name().namespace() == Some(PRESENTATION_NS)
                && n.tag_name().name() == "sldId"
        })
        .collect::<Vec<_>>();
    if ids.len() > MAX_SLIDES {
        return Err(invalid("presentation exceeds the 300 slide preview limit"));
    }
    let mut result = Presentation {
        slides: Vec::new(),
        truncated: false,
    };
    let mut text_bytes = 0usize;
    for id in ids {
        let Some(rel_id) = id.attribute((REL_NS, "id")) else {
            return Err(invalid("slide entry is missing its relationship id"));
        };
        let Some(target) = relationships.get(rel_id) else {
            return Err(invalid("slide relationship is missing"));
        };
        let path = normalize_target("ppt", target);
        let Some(slide_xml) = read_xml(&mut zip, &path, &mut total)? else {
            return Err(invalid(&format!("{path} is missing")));
        };
        let slide_rels_path = relationship_path(&path);
        let slide_rels = read_xml(&mut zip, &slide_rels_path, &mut total)?;
        let image_names = slide_rels
            .as_deref()
            .map(parse_image_relationships)
            .unwrap_or_default();
        let slide_doc = xml(&slide_xml, &path)?;
        let mut slide = Slide {
            text: Vec::new(),
            images: Vec::new(),
        };
        for node in slide_doc.descendants().filter(Node::is_element) {
            if node.tag_name().namespace() == Some(DRAWING_NS)
                && node.tag_name().name() == "blip"
                && let Some(rel_id) = node.attribute((REL_NS, "embed"))
            {
                slide.images.push(
                    image_names
                        .get(rel_id)
                        .cloned()
                        .unwrap_or_else(|| "embedded image".into()),
                );
            }
        }
        // Text is capped across the presentation, while paragraphs retain their order.
        for paragraph in paragraphs(&slide_doc) {
            let bytes = paragraph.len();
            if text_bytes.saturating_add(bytes) > MAX_TEXT {
                result.truncated = true;
                break;
            }
            text_bytes += bytes;
            slide.text.push(paragraph);
        }
        result.slides.push(slide);
        if result.truncated {
            break;
        }
    }
    if result.slides.is_empty() {
        return Err(invalid("presentation contains no readable slides"));
    }
    Ok(result)
}

fn paragraphs(doc: &Document<'_>) -> Vec<String> {
    doc.descendants()
        .filter(|n| {
            n.is_element()
                && n.tag_name().namespace() == Some(DRAWING_NS)
                && n.tag_name().name() == "p"
        })
        .map(|p| {
            p.descendants()
                .filter(|n| {
                    n.is_element()
                        && n.tag_name().namespace() == Some(DRAWING_NS)
                        && n.tag_name().name() == "t"
                })
                .filter_map(|n| n.text())
                .collect::<String>()
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_image_relationships(text: &str) -> std::collections::HashMap<String, String> {
    let Ok(doc) = xml(text, "slide relationships") else {
        return Default::default();
    };
    doc.root_element()
        .children()
        .filter(|n| n.is_element() && n.tag_name().namespace() == Some(PACKAGE_REL_NS))
        .filter_map(|n| {
            let id = n.attribute("Id")?;
            let target = n.attribute("Target")?;
            let ty = n.attribute("Type")?;
            ty.ends_with("/image").then(|| {
                (
                    id.to_string(),
                    target.rsplit('/').next().unwrap_or(target).to_string(),
                )
            })
        })
        .collect()
}

fn relationship_path(path: &str) -> String {
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
    format!("{dir}/_rels/{file}.rels")
}

fn normalize_target(base: &str, target: &str) -> String {
    let combined = if target.starts_with('/') {
        target.trim_start_matches('/').to_string()
    } else {
        format!("{base}/{target}")
    };
    let mut parts = Vec::new();
    for part in combined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn read_xml<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    total: &mut usize,
) -> io::Result<Option<String>> {
    let mut entry = match zip.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(invalid(&format!("{name}: {error}"))),
    };
    if entry.size() > MAX_ENTRY {
        return Err(invalid(&format!("{name} exceeds the 2 MiB XML limit")));
    }
    *total = total.saturating_add(entry.size() as usize);
    if *total > MAX_TOTAL_XML {
        return Err(invalid(
            "presentation XML exceeds the 16 MiB total preview limit",
        ));
    }
    let mut bytes = Vec::new();
    entry.by_ref().take(MAX_ENTRY + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_ENTRY {
        return Err(invalid(&format!("{name} exceeds the 2 MiB XML limit")));
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn xml<'a>(text: &'a str, name: &str) -> io::Result<Document<'a>> {
    Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 2_000_000,
            ..Default::default()
        },
    )
    .map_err(|e| invalid(&format!("{name}: {e}")))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{
        DRAWING_NS, MAX_TEXT, PACKAGE_REL_NS, PRESENTATION_NS, REL_NS, normalize_target,
        paragraphs, parse_archive,
    };

    fn archive(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, contents) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("fixture entry starts");
            std::io::Write::write_all(&mut zip, contents.as_bytes()).expect("fixture entry writes");
        }
        zip.finish().expect("fixture archive finishes").into_inner()
    }

    fn presentation_xml(ids: &str) -> String {
        format!(
            r#"<p:presentation xmlns:p="{PRESENTATION_NS}" xmlns:r="{REL_NS}"><p:sldIdLst>{ids}</p:sldIdLst></p:presentation>"#
        )
    }

    fn slide_xml(text: &str) -> String {
        format!(
            r#"<p:sld xmlns:p="{PRESENTATION_NS}" xmlns:a="{DRAWING_NS}"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
        )
    }

    fn rels(targets: &[(&str, &str)]) -> String {
        let body = targets
            .iter()
            .map(|(id, target)| format!(r#"<Relationship Id="{id}" Target="{target}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide"/>"#))
            .collect::<String>();
        format!(r#"<Relationships xmlns="{PACKAGE_REL_NS}">{body}</Relationships>"#)
    }

    #[test]
    fn resolves_relationship_targets() {
        assert_eq!(
            normalize_target("ppt", "slides/slide1.xml"),
            "ppt/slides/slide1.xml"
        );
        assert_eq!(
            normalize_target("ppt/slides", "../media/image1.png"),
            "ppt/media/image1.png"
        );
    }

    #[test]
    fn extracts_slide_paragraphs_in_order() {
        let doc = roxmltree::Document::parse(r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:p><a:r><a:t>Hello</a:t></a:r><a:r><a:t> world</a:t></a:r></a:p><a:p><a:t>Next</a:t></a:p></p:sld>"#).expect("fixture XML is valid");
        assert_eq!(paragraphs(&doc), ["Hello world", "Next"]);
    }

    #[test]
    fn reads_slides_in_presentation_relationship_order() {
        let archive = archive(&[
            (
                "ppt/presentation.xml",
                &presentation_xml(
                    r#"<p:sldId id="1" r:id="slide-ten"/><p:sldId id="2" r:id="slide-two"/>"#,
                ),
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                &rels(&[
                    ("slide-ten", "slides/slide10.xml"),
                    ("slide-two", "slides/slide2.xml"),
                ]),
            ),
            ("ppt/slides/slide2.xml", &slide_xml("two")),
            ("ppt/slides/slide10.xml", &slide_xml("ten")),
        ]);
        let parsed = parse_archive(Cursor::new(archive)).expect("fixture presentation parses");
        assert_eq!(parsed.slides[0].text, ["ten"]);
        assert_eq!(parsed.slides[1].text, ["two"]);
    }

    #[test]
    fn marks_truncated_text_and_rejects_a_missing_slide_part() {
        let long_text = "x".repeat(MAX_TEXT + 1);
        let oversized = archive(&[
            (
                "ppt/presentation.xml",
                &presentation_xml(r#"<p:sldId id="1" r:id="r1"/>"#),
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                &rels(&[("r1", "slides/slide1.xml")]),
            ),
            ("ppt/slides/slide1.xml", &slide_xml(&long_text)),
        ]);
        let parsed = parse_archive(Cursor::new(oversized)).expect("bounded presentation parses");
        assert!(parsed.truncated);

        let missing = archive(&[
            (
                "ppt/presentation.xml",
                &presentation_xml(r#"<p:sldId id="1" r:id="r1"/>"#),
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                &rels(&[("r1", "slides/slide1.xml")]),
            ),
        ]);
        assert!(parse_archive(Cursor::new(missing)).is_err());
    }
}
