//! Bounded, read-only model for saved Jupyter notebook cells and outputs.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use html5ever::tendril::TendrilSink;
use html5ever::{ParseOpts, QualName, local_name, namespace_url, ns, parse_fragment};
use image::{ImageDecoder as _, ImageFormat, Limits};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use serde_json::Value;

use super::{Content, Previewer, Probe};

const MAX_NOTEBOOK_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CELLS: usize = 500;
const MAX_OUTPUTS_PER_CELL: usize = 100;
const MAX_TEXT_CHARS: usize = 512 * 1024;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_IMAGE_BYTES: usize = 64 * 1024 * 1024;

/// Parsed cells and saved outputs from a notebook, without execution or file access.
#[derive(Debug, Clone, PartialEq)]
pub struct Notebook {
    /// Cells in their original notebook order.
    pub cells: Vec<Cell>,
    /// Kernel language hint used only to select a code highlighter.
    pub language: Option<String>,
    /// Whether bounded cell, text, or output limits omitted content.
    pub truncated: bool,
}

/// One notebook cell with its safe, saved content.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    /// Rendered Markdown that contains only locally safe Markdown constructs.
    Markdown(String),
    /// Literal source and its saved output items.
    Code {
        source: String,
        outputs: Vec<Output>,
    },
    /// Literal raw-cell text without HTML interpretation.
    Raw(String),
    /// Content that this preview deliberately does not interpret.
    Placeholder(String),
}

/// A saved output suitable for displaying as text or a decoded raster image.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Saved text with a short type label.
    Text { label: String, text: String },
    /// Decoded raster pixels in row-major RGBA order.
    Image {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// Unsupported or invalid saved output with a user-visible reason.
    Placeholder(String),
}

/// F3 previewer for Jupyter/IPython notebook JSON.
pub struct NotebookPreviewer;

impl Previewer for NotebookPreviewer {
    fn name(&self) -> &'static str {
        "Notebook"
    }

    fn matches(&self, probe: &Probe) -> bool {
        !probe.is_dir && probe.ext == "ipynb"
    }

    fn load(&self, probe: &Probe) -> io::Result<Content> {
        let notebook = read_notebook(&probe.path)?;
        Ok(Content::Notebook(std::sync::Arc::new(notebook)))
    }
}

/// Reads a bounded notebook document and parses its nbformat 4 cells.
pub fn read_notebook(path: &Path) -> io::Result<Notebook> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_NOTEBOOK_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_NOTEBOOK_BYTES {
        return Err(invalid("notebook exceeds the 16 MiB preview limit"));
    }
    parse_notebook(&bytes)
}

/// Parses notebook JSON while preserving unsupported content as explicit placeholders.
pub fn parse_notebook(bytes: &[u8]) -> io::Result<Notebook> {
    let root: Value = serde_json::from_slice(bytes)
        .map_err(|e| invalid(&format!("invalid notebook JSON: {e}")))?;
    let object = root
        .as_object()
        .ok_or_else(|| invalid("notebook root must be an object"))?;
    let major = object
        .get("nbformat")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("notebook is missing an integer nbformat version"))?;
    if major != 4 {
        return Err(invalid("only nbformat 4 notebooks are supported"));
    }
    let cells = object
        .get("cells")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("notebook cells must be an array"))?;
    if cells.len() > MAX_CELLS {
        return Err(invalid("notebook exceeds the 500 cell preview limit"));
    }
    let mut parsed = Vec::with_capacity(cells.len());
    let mut truncated = false;
    let mut image_bytes = 0usize;
    let metadata = object.get("metadata").and_then(Value::as_object);
    let language = metadata
        .and_then(|metadata| metadata.get("language_info"))
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .or_else(|| {
            metadata
                .and_then(|metadata| metadata.get("kernelspec"))
                .and_then(|value| value.get("language"))
                .and_then(Value::as_str)
        })
        .map(|value| value.chars().take(40).collect());
    for cell in cells {
        let Some(cell) = cell.as_object() else {
            return Err(invalid("notebook cell must be an object"));
        };
        let kind = cell.get("cell_type").and_then(Value::as_str).unwrap_or("");
        let source = field_text(Some(
            cell.get("source")
                .ok_or_else(|| invalid("notebook cell is missing source"))?,
        ))?;
        let source = bounded(source, &mut truncated);
        let parsed_cell = match kind {
            "markdown" => match unsupported_markdown(&source) {
                Some(reason) => Cell::Placeholder(reason.into()),
                None => Cell::Markdown(source),
            },
            "raw"
                if cell
                    .get("metadata")
                    .and_then(|metadata| metadata.get("format"))
                    .and_then(Value::as_str)
                    == Some("text/html")
                    || contains_html_node(&source) =>
            {
                Cell::Placeholder("HTML cell omitted; HTML is not rendered".into())
            }
            "raw" => Cell::Raw(source),
            "code" => {
                let outputs = cell
                    .get("outputs")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid("notebook code cell outputs must be an array"))?;
                let outputs = parse_outputs(outputs, &mut truncated, &mut image_bytes)?;
                Cell::Code { source, outputs }
            }
            _ => Cell::Placeholder(format!(
                "Unsupported notebook cell type: {}",
                shown_kind(kind)
            )),
        };
        parsed.push(parsed_cell);
    }
    Ok(Notebook {
        cells: parsed,
        language,
        truncated,
    })
}

fn parse_outputs(
    outputs: &[Value],
    truncated: &mut bool,
    image_bytes: &mut usize,
) -> io::Result<Vec<Output>> {
    let mut parsed = Vec::new();
    for output in outputs.iter().take(MAX_OUTPUTS_PER_CELL) {
        let Some(object) = output.as_object() else {
            parsed.push(Output::Placeholder("Malformed saved output".into()));
            continue;
        };
        let kind = object
            .get("output_type")
            .and_then(Value::as_str)
            .unwrap_or("");
        match kind {
            "stream" => {
                let text = bounded(field_text(object.get("text"))?, truncated);
                if !text.is_empty() {
                    let name = object
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("stream");
                    parsed.push(Output::Text {
                        label: shown_kind(name),
                        text,
                    });
                }
            }
            "error" => {
                let text = object
                    .get("traceback")
                    .and_then(Value::as_array)
                    .map(|lines| {
                        lines
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .or_else(|| {
                        object
                            .get("evalue")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "Saved error output".into());
                let text = bounded(strip_ansi(&text), truncated);
                parsed.push(Output::Text {
                    label: "Error".into(),
                    text,
                });
            }
            "display_data" | "execute_result" => {
                let data = object.get("data").and_then(Value::as_object);
                let Some(data) = data else {
                    parsed.push(Output::Placeholder("Malformed MIME bundle".into()));
                    continue;
                };
                let has_html = data.contains_key("text/html");
                let image_key = ["image/png", "image/jpeg"]
                    .into_iter()
                    .find(|key| data.contains_key(*key));
                if let Some(key) = image_key {
                    let value = data.get(key).expect("key came from MIME map");
                    match decode_image(value, key, MAX_TOTAL_IMAGE_BYTES - *image_bytes) {
                        Ok(Output::Image {
                            width,
                            height,
                            rgba,
                        }) => {
                            *image_bytes += rgba.len();
                            parsed.push(Output::Image {
                                width,
                                height,
                                rgba,
                            });
                        }
                        Ok(other) => parsed.push(other),
                        Err(message) => {
                            if message.contains("64 MiB total") {
                                *truncated = true;
                            }
                            parsed.push(Output::Placeholder(message));
                        }
                    }
                } else if has_html {
                    let image = data
                        .get("text/html")
                        .and_then(|value| field_text(Some(value)).ok())
                        .as_deref()
                        .and_then(html_data_image);
                    if let Some((mime, encoded)) = image {
                        match decode_image(
                            &Value::String(encoded),
                            mime,
                            MAX_TOTAL_IMAGE_BYTES - *image_bytes,
                        ) {
                            Ok(Output::Image {
                                width,
                                height,
                                rgba,
                            }) => {
                                *image_bytes += rgba.len();
                                parsed.push(Output::Image {
                                    width,
                                    height,
                                    rgba,
                                });
                            }
                            Ok(other) => parsed.push(other),
                            Err(message) => {
                                if message.contains("64 MiB total") {
                                    *truncated = true;
                                }
                                parsed.push(Output::Placeholder(message));
                            }
                        }
                    } else {
                        parsed.push(Output::Placeholder("HTML output is not rendered".into()));
                    }
                } else if let Some(value) = data.get("text/plain") {
                    let text = bounded(field_text(Some(value))?, truncated);
                    if !text.is_empty() {
                        parsed.push(Output::Text {
                            label: "Output".into(),
                            text,
                        });
                    }
                } else if !has_html {
                    let label = data
                        .keys()
                        .next()
                        .map(|key| format!("Unsupported output: {key}"))
                        .unwrap_or_else(|| "Empty MIME bundle".into());
                    parsed.push(Output::Placeholder(label));
                }
            }
            _ => parsed.push(Output::Placeholder(format!(
                "Unsupported saved output: {}",
                shown_kind(kind)
            ))),
        }
    }
    if outputs.len() > MAX_OUTPUTS_PER_CELL {
        *truncated = true;
        parsed.push(Output::Placeholder(
            "Further outputs omitted (100 output limit)".into(),
        ));
    }
    Ok(parsed)
}

fn decode_image(value: &Value, mime: &str, remaining_bytes: usize) -> Result<Output, String> {
    let data = field_text(Some(value)).map_err(|_| format!("Invalid {mime} output"))?;
    let data: String = data
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    let data = STANDARD
        .decode(data)
        .map_err(|_| format!("Invalid base64 in {mime} output"))?;
    if data.len() > MAX_IMAGE_BYTES {
        return Err(format!("{mime} output exceeds the 8 MiB image limit"));
    }
    let format = if mime == "image/png" {
        ImageFormat::Png
    } else {
        ImageFormat::Jpeg
    };
    let reader = image::ImageReader::with_format(std::io::Cursor::new(data), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| format!("Invalid or oversized {mime} output"))?;
    let (width, height) = decoder.dimensions();
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_IMAGE_PIXELS || width > 4096 || height > 4096 {
        return Err(format!("{mime} output exceeds the 4 megapixel limit"));
    }
    let rgba_bytes = usize::try_from(pixels.saturating_mul(4)).unwrap_or(usize::MAX);
    if rgba_bytes > remaining_bytes {
        return Err("Notebook images exceed the 64 MiB total preview limit".into());
    }
    decoder
        .set_limits(limits)
        .map_err(|_| format!("Invalid or oversized {mime} output"))?;
    let decoded = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| format!("Invalid or oversized {mime} output"))?;
    Ok(Output::Image {
        width,
        height,
        rgba: decoded.to_rgba8().into_raw(),
    })
}

/// Extracts a data URI from one image and inert image-only wrappers.
fn html_data_image(html: &str) -> Option<(&'static str, String)> {
    let context = QualName::new(None, ns!(html), local_name!("div"));
    let dom = parse_fragment(RcDom::default(), ParseOpts::default(), context, Vec::new()).one(html);
    fn visit(node: &Handle, image: &mut Option<(&'static str, String)>) -> bool {
        match &node.data {
            NodeData::Text { contents } => contents.borrow().trim().is_empty(),
            NodeData::Element { name, attrs, .. } => {
                let tag = name.local.as_ref();
                if tag == "img" {
                    if image.is_some() {
                        return false;
                    }
                    let attrs = attrs.borrow();
                    let Some(src) = attrs.iter().find(|attr| attr.name.local.as_ref() == "src")
                    else {
                        return false;
                    };
                    if attrs.iter().any(|attr| {
                        !matches!(
                            attr.name.local.as_ref(),
                            "src" | "alt" | "width" | "height" | "class" | "style"
                        )
                    }) {
                        return false;
                    }
                    let src = src.value.to_string();
                    let (mime, base64) =
                        if let Some(encoded) = src.strip_prefix("data:image/png;base64,") {
                            ("image/png", encoded)
                        } else if let Some(encoded) = src.strip_prefix("data:image/jpeg;base64,") {
                            ("image/jpeg", encoded)
                        } else {
                            return false;
                        };
                    // Bound the encoded data before allocating a copy or decoding it.
                    if base64.len() > (MAX_IMAGE_BYTES * 4).div_ceil(3) + 4 {
                        return false;
                    }
                    *image = Some((mime, base64.to_owned()));
                } else {
                    let attrs = attrs.borrow();
                    if !matches!(tag, "div" | "span" | "p" | "figure")
                        || attrs.iter().any(|attr| {
                            !matches!(attr.name.local.as_ref(), "class" | "style" | "id")
                        })
                    {
                        return false;
                    }
                }
                node.children
                    .borrow()
                    .iter()
                    .all(|child| visit(child, image))
            }
            _ => false,
        }
    }

    let mut image = None;
    dom.document
        .children
        .borrow()
        .iter()
        .all(|child| match &child.data {
            // html5ever places parsed fragments under this synthetic document root.
            NodeData::Element { name, attrs, .. }
                if name.local.as_ref() == "html" && attrs.borrow().is_empty() =>
            {
                child
                    .children
                    .borrow()
                    .iter()
                    .all(|nested| visit(nested, &mut image))
            }
            _ => visit(child, &mut image),
        })
        .then_some(image)
        .flatten()
}

fn field_text(value: Option<&Value>) -> io::Result<String> {
    match value {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(Value::Array(lines)) => lines
            .iter()
            .map(|line| {
                line.as_str()
                    .ok_or_else(|| invalid("notebook text array must contain strings"))
            })
            .collect::<io::Result<Vec<_>>>()
            .map(|lines| lines.concat()),
        None => Ok(String::new()),
        _ => Err(invalid(
            "notebook text field must be a string or array of strings",
        )),
    }
}

fn bounded(text: String, truncated: &mut bool) -> String {
    if text.len() <= MAX_TEXT_CHARS {
        return text;
    }
    let mut end = MAX_TEXT_CHARS;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    *truncated = true;
    text[..end].to_string()
}

fn contains_html_node(text: &str) -> bool {
    let Ok(tree) = markdown::to_mdast(text, &markdown_options()) else {
        return false;
    };
    fn visit(node: &markdown::mdast::Node) -> bool {
        matches!(node, markdown::mdast::Node::Html(_))
            || node
                .children()
                .is_some_and(|children| children.iter().any(visit))
    }
    visit(&tree)
}

fn unsupported_markdown(text: &str) -> Option<&'static str> {
    let tree = match markdown::to_mdast(text, &markdown_options()) {
        Ok(tree) => tree,
        Err(_) => return Some("Markdown cell could not be safely inspected"),
    };
    fn visit(node: &markdown::mdast::Node) -> Option<&'static str> {
        match node {
            markdown::mdast::Node::Html(_) => Some("HTML cell omitted; HTML is not rendered"),
            markdown::mdast::Node::Image(_) | markdown::mdast::Node::ImageReference(_) => {
                Some("Markdown images are not loaded from notebook files")
            }
            markdown::mdast::Node::Math(_) | markdown::mdast::Node::InlineMath(_) => {
                Some("Math in Markdown cells is not rendered")
            }
            markdown::mdast::Node::MdxJsxFlowElement(_)
            | markdown::mdast::Node::MdxJsxTextElement(_) => {
                Some("Interactive Markdown content is not rendered")
            }
            _ => node
                .children()
                .and_then(|children| children.iter().find_map(visit)),
        }
    }
    visit(&tree)
}

fn markdown_options() -> markdown::ParseOptions {
    markdown::ParseOptions {
        constructs: markdown::Constructs {
            math_flow: true,
            math_text: true,
            ..markdown::Constructs::default()
        },
        ..markdown::ParseOptions::default()
    }
}

fn shown_kind(kind: &str) -> String {
    let kind: String = kind.chars().take(80).collect();
    if kind.is_empty() {
        "unknown".into()
    } else {
        kind
    }
}

fn strip_ansi(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Cell, NotebookPreviewer, Output, Previewer, Probe, decode_image, parse_notebook};
    use base64::Engine as _;
    use image::{ImageEncoder as _, Rgba, RgbaImage};
    use serde_json::Value;

    fn probe(path: &std::path::Path) -> Probe {
        Probe {
            path: path.to_path_buf(),
            name: "demo.ipynb".into(),
            ext: "ipynb".into(),
            is_dir: false,
            is_file: true,
            size: 0,
            head: Vec::new(),
        }
    }

    fn notebook(cells: &str) -> Vec<u8> {
        format!(r#"{{"nbformat":4,"nbformat_minor":99,"cells":[{cells}]}}"#).into_bytes()
    }

    #[test]
    fn parses_markdown_code_stream_plain_error_and_raster() {
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &RgbaImage::from_pixel(1, 1, Rgba([1, 2, 3, 255])).into_raw(),
                1,
                1,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let json = notebook(&format!(
            r##"
          {{"cell_type":"markdown","source":["# Café ","☕"]}},
          {{"cell_type":"code","source":"print(1)","outputs":[
            {{"output_type":"stream","text":["hi"," there"]}},
            {{"output_type":"execute_result","data":{{"text/plain":"42"}}}},
            {{"output_type":"error","traceback":["\u001b[31mTraceback\u001b[0m"]}},
            {{"output_type":"display_data","data":{{"image/png":"{encoded}"}}}}
          ]}}
        "##
        ));
        let parsed = parse_notebook(&json).unwrap();
        assert_eq!(parsed.cells[0], Cell::Markdown("# Café ☕".into()));
        let Cell::Code { outputs, .. } = &parsed.cells[1] else {
            panic!("expected code")
        };
        assert_eq!(
            outputs[0],
            Output::Text {
                label: "stream".into(),
                text: "hi there".into()
            }
        );
        assert_eq!(
            outputs[1],
            Output::Text {
                label: "Output".into(),
                text: "42".into()
            }
        );
        assert_eq!(
            outputs[2],
            Output::Text {
                label: "Error".into(),
                text: "Traceback".into()
            }
        );
        assert!(
            matches!(&outputs[3], Output::Image { width: 1, height: 1, rgba } if rgba == &[1, 2, 3, 255])
        );
    }

    #[test]
    fn html_bundle_without_image_fragment_displays_only_its_placeholder() {
        let parsed = parse_notebook(&notebook(
            r#"{"cell_type":"code","source":"","outputs":[
          {"output_type":"display_data","data":{"text/html":"<b>hello</b>","text/plain":"hello"}}
        ]}"#,
        ))
        .unwrap();
        let Cell::Code { outputs, .. } = &parsed.cells[0] else {
            panic!("expected code")
        };
        assert_eq!(
            outputs[0],
            Output::Placeholder("HTML output is not rendered".into())
        );
        assert_eq!(outputs.len(), 1);
    }

    #[test]
    fn html_bundle_prefers_explicit_sibling_raster_data() {
        let png = test_png(1, 1, [4, 5, 6, 255]);
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let parsed = parse_notebook(&notebook(&format!(
            r#"{{"cell_type":"code","source":"","outputs":[{{"output_type":"display_data","data":{{"text/html":"<script>bad()</script>","image/png":"{encoded}"}}}}]}}"#
        )))
        .unwrap();
        let Cell::Code { outputs, .. } = &parsed.cells[0] else {
            panic!("expected code")
        };
        assert!(
            matches!(&outputs[0], Output::Image { width: 1, height: 1, rgba } if rgba == &[4, 5, 6, 255])
        );
    }

    #[test]
    fn html_image_only_data_uri_is_decoded_and_unsafe_fragments_are_placeholders() {
        let png = base64::engine::general_purpose::STANDARD.encode(test_png(1, 1, [7, 8, 9, 255]));
        let html = vec![
            "<div class=\"output_subarea\"><img alt=\"plot\" width=\"1\" ".to_owned(),
            format!("src=\"data:image/png;base64,{png}\"></div>"),
        ];
        let parsed = parse_notebook(&notebook(&format!(
            r#"{{"cell_type":"code","source":"","outputs":[{{"output_type":"display_data","data":{{"text/html":{}}}}}]}}"#,
            serde_json::to_string(&html).unwrap()
        )))
        .unwrap();
        let Cell::Code { outputs, .. } = &parsed.cells[0] else {
            panic!("expected code")
        };
        assert!(
            matches!(&outputs[0], Output::Image { width: 1, height: 1, rgba } if rgba == &[7, 8, 9, 255])
        );

        for html in [
            "<img src=\"https://example.test/image.png\">",
            "<img src=\"data:image/png;base64,AA==\"><script>bad()</script>",
            "<img src=\"data:image/png;base64,AA==\" onerror=\"bad()\">",
            "<div><img src=\"data:image/png;base64,AA==\">caption</div>",
            "<img src=\"data:image/svg+xml;base64,AA==\">",
        ] {
            let parsed = parse_notebook(&notebook(&format!(
                r#"{{"cell_type":"code","source":"","outputs":[{{"output_type":"display_data","data":{{"text/html":{}}}}}]}}"#,
                serde_json::to_string(html).unwrap()
            )))
            .unwrap();
            let Cell::Code { outputs, .. } = &parsed.cells[0] else {
                panic!("expected code")
            };
            assert_eq!(
                outputs[0],
                Output::Placeholder("HTML output is not rendered".into())
            );
        }
    }

    #[test]
    fn html_in_markdown_and_raw_is_explicit_placeholder() {
        let parsed = parse_notebook(&notebook(
            r#"
          {"cell_type":"markdown","source":"<div>hello</div>"},
          {"cell_type":"raw","source":"<script>alert(1)</script>"}
        "#,
        ))
        .unwrap();
        assert_eq!(
            parsed.cells[0],
            Cell::Placeholder("HTML cell omitted; HTML is not rendered".into())
        );
        assert_eq!(
            parsed.cells[1],
            Cell::Placeholder("HTML cell omitted; HTML is not rendered".into())
        );
    }

    #[test]
    fn html_in_fenced_code_remains_code_and_markdown_images_are_placeholders() {
        let parsed = parse_notebook(&notebook(
            r#"
          {"cell_type":"markdown","source":"```html\n<div>literal</div>\n``` and `<i>literal</i>`"},
          {"cell_type":"markdown","source":"![image](file.png)"}
        "#,
        ))
        .unwrap();
        assert!(matches!(&parsed.cells[0], Cell::Markdown(_)));
        assert_eq!(
            parsed.cells[1],
            Cell::Placeholder("Markdown images are not loaded from notebook files".into())
        );
    }

    #[test]
    fn raw_html_format_is_a_placeholder_even_without_a_tag() {
        let parsed = parse_notebook(&notebook(
            r#"{"cell_type":"raw","metadata":{"format":"text/html"},"source":"plain"}"#,
        ))
        .unwrap();
        assert_eq!(
            parsed.cells[0],
            Cell::Placeholder("HTML cell omitted; HTML is not rendered".into())
        );
    }

    #[test]
    fn rejects_invalid_roots_versions_fields_and_bad_base64() {
        assert!(parse_notebook(b"[]").is_err());
        assert!(parse_notebook(br#"{"nbformat":3,"cells":[]}"#).is_err());
        assert!(
            parse_notebook(br#"{"nbformat":4,"cells":[{"cell_type":"code","source":[1]}]}"#)
                .is_err()
        );
        assert!(parse_notebook(br#"{"nbformat":4,"cells":[{"cell_type":"raw"}]}"#).is_err());
        let parsed = parse_notebook(&notebook(r#"{"cell_type":"code","source":"","outputs":[{"output_type":"display_data","data":{"image/png":"!"}}]}"#)).unwrap();
        let Cell::Code { outputs, .. } = &parsed.cells[0] else {
            panic!("expected code")
        };
        assert!(matches!(&outputs[0], Output::Placeholder(message) if message.contains("base64")));
    }

    #[test]
    fn caps_cell_count_dimensions_and_total_image_memory() {
        let too_many_cells = format!(
            r#"{{"nbformat":4,"cells":[{}]}}"#,
            (0..=super::MAX_CELLS)
                .map(|_| r#"{"cell_type":"raw","source":""}"#)
                .collect::<Vec<_>>()
                .join(",")
        );
        assert!(parse_notebook(too_many_cells.as_bytes()).is_err());

        let mut oversized = Vec::new();
        image::codecs::png::PngEncoder::new(&mut oversized)
            .write_image(
                &RgbaImage::from_pixel(2049, 2048, Rgba([0, 0, 0, 255])).into_raw(),
                2049,
                2048,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        assert!(
            decode_image(
                &Value::String(base64::engine::general_purpose::STANDARD.encode(oversized)),
                "image/png",
                super::MAX_TOTAL_IMAGE_BYTES,
            )
            .is_err()
        );

        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &RgbaImage::from_pixel(1024, 1024, Rgba([1, 2, 3, 255])).into_raw(),
                1024,
                1024,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let html = format!("<img src=\"data:image/png;base64,{encoded}\">");
        let html = serde_json::to_string(&html).unwrap();
        let outputs = (0..17)
            .map(|_| format!(r#"{{"output_type":"display_data","data":{{"text/html":{html}}}}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let parsed = parse_notebook(
            format!(
                r#"{{"nbformat":4,"cells":[{{"cell_type":"code","source":"","outputs":[{outputs}]}}]}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let Cell::Code { outputs, .. } = &parsed.cells[0] else {
            panic!("expected code")
        };
        assert!(
            matches!(outputs.last(), Some(Output::Placeholder(text)) if text.contains("64 MiB"))
        );
        assert!(parsed.truncated);
    }

    fn test_png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &RgbaImage::from_pixel(width, height, Rgba(color)).into_raw(),
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        png
    }

    #[test]
    fn matches_ipynb_and_loads_file_offline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("demo.ipynb");
        std::fs::write(&path, notebook(r#"{"cell_type":"markdown","source":"hi"}"#)).unwrap();
        assert!(NotebookPreviewer.matches(&probe(&path)));
        assert!(matches!(
            NotebookPreviewer.load(&probe(&path)).unwrap(),
            super::super::Content::Notebook(_)
        ));
    }
}
