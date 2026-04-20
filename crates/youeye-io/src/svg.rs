//! SVG parse + serialize.
//!
//! Canonical round-trip: first `from_svg` + `to_svg` produces a normalized
//! form (sorted attrs, 2-space indent, `\n` line endings, self-closing empty
//! elements, `<?xml ...?>` declaration). Every subsequent load+save of that
//! output is byte-identical.
//!
//! Non-goals for this slice:
//! - Parsing `transform=""` values into `kurbo::Affine` — the original string
//!   is preserved in `extra_attrs["transform"]` and re-emitted verbatim.
//! - Mapping `<g>` to `Frame`. Everything that's a group in SVG parses to
//!   `Node::Group` regardless of auto-layout hints; the `Frame` variant is
//!   there for Phase 3 to wire up.
//! - Foreign SVG normalization via `usvg`. This module only handles the
//!   youeye dialect; foreign-SVG best-effort import is a separate entry
//!   point in a later phase.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};
use youeye_doc::kurbo;
use youeye_doc::{
    Color, Document, Ellipse, Fill, Group, Node, NodeBase, Paint, Path, Rect, Stroke, Text, Tokens,
    Variables, ViewBox,
};

use crate::LINE_ENDING;

const INDENT: &str = "  ";
const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>";

/// Parse an SVG string into a `Document`.
pub fn from_svg(input: &str) -> Result<Document> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(true);

    // Skip until we hit the root <svg>.
    loop {
        match reader.read_event()? {
            Event::Start(ref e) if local_name(e.name().as_ref()) == b"svg" => {
                let mut doc = document_from_start(e)?;
                parse_children(&mut reader, &mut doc.children, &mut doc.raw_style, b"svg")?;
                if let Some(css) = &doc.raw_style {
                    let (tokens, variables) = extract_root_declarations(css);
                    doc.tokens = tokens;
                    doc.variables = variables;
                }
                return Ok(doc);
            }
            Event::Empty(ref e) if local_name(e.name().as_ref()) == b"svg" => {
                return document_from_start(e);
            }
            Event::Eof => bail!("no <svg> root element found"),
            _ => continue,
        }
    }
}

/// Serialize a `Document` to canonical SVG.
pub fn to_svg(doc: &Document) -> String {
    let mut out = String::new();
    out.push_str(XML_DECL);
    out.push_str(LINE_ENDING);

    let mut attrs: BTreeMap<String, String> = doc.extra_attrs.clone();
    if let Some(vb) = doc.view_box {
        attrs.insert(
            "viewBox".into(),
            format!(
                "{} {} {} {}",
                fmt_num(vb.min_x),
                fmt_num(vb.min_y),
                fmt_num(vb.width),
                fmt_num(vb.height),
            ),
        );
    }
    if let Some(w) = doc.width {
        attrs.insert("width".into(), fmt_num(w));
    }
    if let Some(h) = doc.height {
        attrs.insert("height".into(), fmt_num(h));
    }
    // Ensure the default SVG namespace and our extension prefix are always
    // present in canonical output, even if the source didn't declare them.
    attrs
        .entry("xmlns".into())
        .or_insert_with(|| "http://www.w3.org/2000/svg".into());
    if has_any_youeye_usage(doc) {
        attrs
            .entry("xmlns:youeye".into())
            .or_insert_with(|| "https://youeye.app/ns".into());
    }

    let body_empty = doc.children.is_empty() && doc.raw_style.is_none();
    write_open_tag(&mut out, 0, "svg", &attrs, body_empty);
    if body_empty {
        return out;
    }

    if let Some(css) = &doc.raw_style {
        write_style(&mut out, 1, css);
    }
    for child in &doc.children {
        write_node(&mut out, 1, child);
    }
    write_close_tag(&mut out, 0, "svg");
    out
}

// ---- parser internals ----

fn document_from_start(e: &BytesStart<'_>) -> Result<Document> {
    let mut doc = Document::default();
    for attr in e.attributes().with_checks(false) {
        let a = attr?;
        let key = std::str::from_utf8(a.key.as_ref())?.to_string();
        let value = a.unescape_value()?.into_owned();
        match key.as_str() {
            "viewBox" => doc.view_box = parse_view_box(&value),
            "width" => doc.width = parse_length(&value),
            "height" => doc.height = parse_length(&value),
            _ => {
                doc.extra_attrs.insert(key, value);
            }
        }
    }
    Ok(doc)
}

fn parse_children(
    reader: &mut Reader<&[u8]>,
    out: &mut Vec<Node>,
    raw_style: &mut Option<String>,
    close_name: &[u8],
) -> Result<()> {
    loop {
        match reader.read_event()? {
            Event::Start(ref e) => {
                let name = local_name(e.name().as_ref()).to_vec();
                if name == b"style" {
                    *raw_style = Some(read_style_content(reader)?);
                    continue;
                }
                let base = base_from_attrs(e)?;
                match name.as_slice() {
                    b"g" => {
                        let mut children = Vec::new();
                        let mut nested_style: Option<String> = None;
                        parse_children(reader, &mut children, &mut nested_style, b"g")?;
                        out.push(Node::Group(Group { base, children }));
                    }
                    b"text" => {
                        let (x, y) = take_xy(&base);
                        let (font_family, font_size) = take_font(&base);
                        let content = read_text_content(reader)?;
                        out.push(Node::Text(Text {
                            base: strip_shape_attrs(base, &["x", "y", "font-family", "font-size"]),
                            x,
                            y,
                            content,
                            font_family,
                            font_size,
                        }));
                    }
                    other => bail!(
                        "unsupported element <{}> with children",
                        std::str::from_utf8(other)?
                    ),
                }
            }
            Event::Empty(ref e) => {
                let name = local_name(e.name().as_ref()).to_vec();
                let base = base_from_attrs(e)?;
                match name.as_slice() {
                    b"rect" => {
                        let x = parse_f64(base.extra_attrs.get("x")).unwrap_or(0.0);
                        let y = parse_f64(base.extra_attrs.get("y")).unwrap_or(0.0);
                        let width = parse_f64(base.extra_attrs.get("width")).unwrap_or(0.0);
                        let height = parse_f64(base.extra_attrs.get("height")).unwrap_or(0.0);
                        let rx = parse_f64(base.extra_attrs.get("rx")).unwrap_or(0.0);
                        let ry = parse_f64(base.extra_attrs.get("ry")).unwrap_or(0.0);
                        out.push(Node::Rect(Rect {
                            base: strip_shape_attrs(
                                base,
                                &["x", "y", "width", "height", "rx", "ry"],
                            ),
                            x,
                            y,
                            width,
                            height,
                            rx,
                            ry,
                        }));
                    }
                    b"ellipse" => {
                        let cx = parse_f64(base.extra_attrs.get("cx")).unwrap_or(0.0);
                        let cy = parse_f64(base.extra_attrs.get("cy")).unwrap_or(0.0);
                        let rx = parse_f64(base.extra_attrs.get("rx")).unwrap_or(0.0);
                        let ry = parse_f64(base.extra_attrs.get("ry")).unwrap_or(0.0);
                        out.push(Node::Ellipse(Ellipse {
                            base: strip_shape_attrs(base, &["cx", "cy", "rx", "ry"]),
                            cx,
                            cy,
                            rx,
                            ry,
                        }));
                    }
                    b"path" => {
                        let d = base.extra_attrs.get("d").map(String::as_str).unwrap_or("");
                        let data = kurbo::BezPath::from_svg(d).unwrap_or_default();
                        out.push(Node::Path(Path {
                            base: strip_shape_attrs(base, &["d"]),
                            data,
                        }));
                    }
                    b"g" => {
                        // Empty self-closing group — rare, but legal.
                        out.push(Node::Group(Group {
                            base,
                            children: Vec::new(),
                        }));
                    }
                    other => bail!(
                        "unsupported self-closing element <{}>",
                        std::str::from_utf8(other)?
                    ),
                }
            }
            Event::End(ref e) if local_name(e.name().as_ref()) == close_name => return Ok(()),
            Event::Eof => bail!(
                "unexpected EOF before </{}>",
                std::str::from_utf8(close_name)?
            ),
            _ => continue,
        }
    }
}

fn base_from_attrs(e: &BytesStart<'_>) -> Result<NodeBase> {
    let mut base = NodeBase::default();
    for attr in e.attributes().with_checks(false) {
        let a = attr?;
        let key = std::str::from_utf8(a.key.as_ref())?.to_string();
        let value = a.unescape_value()?.into_owned();
        match key.as_str() {
            "id" => base.id = Some(value),
            "fill" => base.fill = Some(parse_fill(&value)),
            "stroke" => {
                let existing = base.stroke.clone().unwrap_or_default();
                base.stroke = Some(Stroke {
                    paint: parse_paint(&value),
                    ..existing
                });
            }
            "stroke-width" => {
                let existing = base.stroke.clone().unwrap_or_default();
                base.stroke = Some(Stroke {
                    width: parse_f64(Some(&value)),
                    ..existing
                });
            }
            "fill-opacity" => {
                let existing = base.fill.clone().unwrap_or_default();
                base.fill = Some(Fill {
                    opacity: value.parse::<f32>().ok(),
                    ..existing
                });
            }
            "stroke-opacity" => {
                let existing = base.stroke.clone().unwrap_or_default();
                base.stroke = Some(Stroke {
                    opacity: value.parse::<f32>().ok(),
                    ..existing
                });
            }
            k if k.starts_with("youeye:") => {
                let bare = k.trim_start_matches("youeye:").to_string();
                base.youeye_attrs.insert(bare, value);
            }
            _ => {
                base.extra_attrs.insert(key, value);
            }
        }
    }
    Ok(base)
}

fn strip_shape_attrs(mut base: NodeBase, keys: &[&str]) -> NodeBase {
    for k in keys {
        base.extra_attrs.remove(*k);
    }
    base
}

fn take_xy(base: &NodeBase) -> (f64, f64) {
    (
        parse_f64(base.extra_attrs.get("x")).unwrap_or(0.0),
        parse_f64(base.extra_attrs.get("y")).unwrap_or(0.0),
    )
}

fn take_font(base: &NodeBase) -> (Option<String>, Option<f64>) {
    let family = base.extra_attrs.get("font-family").cloned();
    let size = parse_f64(base.extra_attrs.get("font-size"));
    (family, size)
}

fn read_text_content(reader: &mut Reader<&[u8]>) -> Result<String> {
    let mut buf = String::new();
    loop {
        match reader.read_event()? {
            Event::Text(t) => {
                let decoded = t.decode()?;
                buf.push_str(&unescape(&decoded)?);
            }
            Event::CData(t) => buf.push_str(std::str::from_utf8(&t)?),
            Event::End(ref e) if local_name(e.name().as_ref()) == b"text" => return Ok(buf),
            Event::Eof => bail!("unexpected EOF inside <text>"),
            _ => continue,
        }
    }
}

fn read_style_content(reader: &mut Reader<&[u8]>) -> Result<String> {
    let mut buf = String::new();
    loop {
        match reader.read_event()? {
            Event::Text(t) => {
                let decoded = t.decode()?;
                buf.push_str(&unescape(&decoded)?);
            }
            Event::CData(t) => buf.push_str(std::str::from_utf8(&t)?),
            Event::End(ref e) if local_name(e.name().as_ref()) == b"style" => return Ok(buf),
            Event::Eof => bail!("unexpected EOF inside <style>"),
            _ => continue,
        }
    }
}

fn parse_view_box(s: &str) -> Option<ViewBox> {
    let parts: Vec<f64> = s
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<f64>().ok())
        .collect();
    if parts.len() == 4 {
        Some(ViewBox {
            min_x: parts[0],
            min_y: parts[1],
            width: parts[2],
            height: parts[3],
        })
    } else {
        None
    }
}

fn parse_length(s: &str) -> Option<f64> {
    let trimmed = s.trim();
    let end = trimmed
        .find(|c: char| {
            !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')
        })
        .unwrap_or(trimmed.len());
    trimmed[..end].parse().ok()
}

fn parse_f64(s: Option<&String>) -> Option<f64> {
    s.and_then(|v| parse_length(v))
}

// ---- paint parsing ----

fn parse_fill(s: &str) -> Fill {
    Fill {
        paint: parse_paint(s),
        opacity: None,
    }
}

fn parse_paint(s: &str) -> Paint {
    let trimmed = s.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        return Paint::None;
    }
    if let Some(color) = parse_hex_color(trimmed) {
        return Paint::Solid(color);
    }
    if let Some(color) = parse_rgb_fn(trimmed) {
        return Paint::Solid(color);
    }
    Paint::Raw(trimmed.to_string())
}

fn parse_hex_color(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#')?;
    let (r, g, b, a) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            (r, g, b, 255u8)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b, 255u8)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            (r, g, b, a)
        }
        _ => return None,
    };
    Some(Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    })
}

fn parse_rgb_fn(s: &str) -> Option<Color> {
    let (prefix, rest) = if let Some(r) = s.strip_prefix("rgb(") {
        (false, r)
    } else if let Some(r) = s.strip_prefix("rgba(") {
        (true, r)
    } else {
        return None;
    };
    let rest = rest.strip_suffix(')')?;
    let parts: Vec<&str> = rest.split(',').map(str::trim).collect();
    let expected = if prefix { 4 } else { 3 };
    if parts.len() != expected {
        return None;
    }
    let r = parts[0].parse::<f32>().ok()? / 255.0;
    let g = parts[1].parse::<f32>().ok()? / 255.0;
    let b = parts[2].parse::<f32>().ok()? / 255.0;
    let a = if prefix {
        parts[3].parse::<f32>().ok()?
    } else {
        1.0
    };
    Some(Color { r, g, b, a })
}

// ---- CSS (minimal) ----

/// Extract `--token-*` and `--var-*` declarations from any `:root { ... }`
/// blocks in the CSS text. Anything else (selectors, `@media`, `@font-face`)
/// is ignored here — `raw_style` keeps the full text for round-trip.
pub fn extract_root_declarations(css: &str) -> (Tokens, Variables) {
    let mut tokens = Tokens::default();
    let mut variables = Variables::default();

    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if css[i..].trim_start().starts_with(":root") {
            let remainder = &css[i..];
            let skipped = remainder.len() - remainder.trim_start().len();
            let after_selector = &remainder[skipped + ":root".len()..];
            if let Some(open) = after_selector.find('{') {
                let body_start = skipped + ":root".len() + open + 1;
                if let Some(close) = css[i + body_start..].find('}') {
                    let body = &css[i + body_start..i + body_start + close];
                    for decl in body.split(';') {
                        let decl = decl.trim();
                        if decl.is_empty() {
                            continue;
                        }
                        if let Some((name, value)) = decl.split_once(':') {
                            let name = name.trim();
                            let value = value.trim().to_string();
                            if let Some(bare) = name.strip_prefix("--token-") {
                                tokens.insert(bare, value);
                            } else if let Some(bare) = name.strip_prefix("--var-") {
                                variables.insert(bare, value);
                            }
                        }
                    }
                    i = i + body_start + close + 1;
                    continue;
                }
            }
        }
        i += 1;
    }

    (tokens, variables)
}

// ---- writer ----

fn write_node(out: &mut String, depth: usize, node: &Node) {
    match node {
        Node::Group(g) => {
            let attrs = attrs_for_base(&g.base);
            if g.children.is_empty() {
                write_open_tag(out, depth, "g", &attrs, true);
            } else {
                write_open_tag(out, depth, "g", &attrs, false);
                for c in &g.children {
                    write_node(out, depth + 1, c);
                }
                write_close_tag(out, depth, "g");
            }
        }
        Node::Frame(f) => {
            // Frame is serialised as a plain <g> plus youeye namespace attrs
            // until Phase 3 wires up a dedicated representation.
            let mut attrs = attrs_for_base(&f.base);
            attrs.insert("youeye:frame".into(), "true".into());
            attrs.insert("youeye:x".into(), fmt_num(f.x));
            attrs.insert("youeye:y".into(), fmt_num(f.y));
            attrs.insert("youeye:width".into(), fmt_num(f.width));
            attrs.insert("youeye:height".into(), fmt_num(f.height));
            if f.children.is_empty() {
                write_open_tag(out, depth, "g", &attrs, true);
            } else {
                write_open_tag(out, depth, "g", &attrs, false);
                for c in &f.children {
                    write_node(out, depth + 1, c);
                }
                write_close_tag(out, depth, "g");
            }
        }
        Node::Rect(r) => {
            let mut attrs = attrs_for_base(&r.base);
            attrs.insert("x".into(), fmt_num(r.x));
            attrs.insert("y".into(), fmt_num(r.y));
            attrs.insert("width".into(), fmt_num(r.width));
            attrs.insert("height".into(), fmt_num(r.height));
            if r.rx != 0.0 {
                attrs.insert("rx".into(), fmt_num(r.rx));
            }
            if r.ry != 0.0 {
                attrs.insert("ry".into(), fmt_num(r.ry));
            }
            write_open_tag(out, depth, "rect", &attrs, true);
        }
        Node::Ellipse(e) => {
            let mut attrs = attrs_for_base(&e.base);
            attrs.insert("cx".into(), fmt_num(e.cx));
            attrs.insert("cy".into(), fmt_num(e.cy));
            attrs.insert("rx".into(), fmt_num(e.rx));
            attrs.insert("ry".into(), fmt_num(e.ry));
            write_open_tag(out, depth, "ellipse", &attrs, true);
        }
        Node::Path(p) => {
            let mut attrs = attrs_for_base(&p.base);
            attrs.insert("d".into(), p.data.to_svg());
            write_open_tag(out, depth, "path", &attrs, true);
        }
        Node::Text(t) => {
            let mut attrs = attrs_for_base(&t.base);
            attrs.insert("x".into(), fmt_num(t.x));
            attrs.insert("y".into(), fmt_num(t.y));
            if let Some(f) = &t.font_family {
                attrs.insert("font-family".into(), f.clone());
            }
            if let Some(s) = t.font_size {
                attrs.insert("font-size".into(), fmt_num(s));
            }
            push_indent(out, depth);
            out.push('<');
            out.push_str("text");
            write_sorted_attrs(out, &attrs);
            out.push('>');
            out.push_str(&escape_text(&t.content));
            out.push_str("</text>");
            out.push_str(LINE_ENDING);
        }
    }
}

fn attrs_for_base(base: &NodeBase) -> BTreeMap<String, String> {
    let mut attrs: BTreeMap<String, String> = base.extra_attrs.clone();
    if let Some(id) = &base.id {
        attrs.insert("id".into(), id.clone());
    }
    if let Some(fill) = &base.fill {
        attrs.insert("fill".into(), paint_to_string(&fill.paint));
        if let Some(op) = fill.opacity {
            attrs.insert("fill-opacity".into(), fmt_num(op as f64));
        }
    }
    if let Some(stroke) = &base.stroke {
        attrs.insert("stroke".into(), paint_to_string(&stroke.paint));
        if let Some(w) = stroke.width {
            attrs.insert("stroke-width".into(), fmt_num(w));
        }
        if let Some(op) = stroke.opacity {
            attrs.insert("stroke-opacity".into(), fmt_num(op as f64));
        }
    }
    for (k, v) in &base.youeye_attrs {
        attrs.insert(format!("youeye:{k}"), v.clone());
    }
    attrs
}

fn paint_to_string(p: &Paint) -> String {
    match p {
        Paint::None => "none".into(),
        Paint::Solid(c) => color_to_hex(*c),
        Paint::Raw(s) => s.clone(),
    }
}

fn color_to_hex(c: Color) -> String {
    let r = (c.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (c.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (c.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = (c.a.clamp(0.0, 1.0) * 255.0).round() as u8;
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

fn write_open_tag(
    out: &mut String,
    depth: usize,
    name: &str,
    attrs: &BTreeMap<String, String>,
    self_close: bool,
) {
    push_indent(out, depth);
    out.push('<');
    out.push_str(name);
    write_sorted_attrs(out, attrs);
    if self_close {
        out.push_str("/>");
    } else {
        out.push('>');
    }
    out.push_str(LINE_ENDING);
}

fn write_close_tag(out: &mut String, depth: usize, name: &str) {
    push_indent(out, depth);
    out.push_str("</");
    out.push_str(name);
    out.push('>');
    out.push_str(LINE_ENDING);
}

fn write_sorted_attrs(out: &mut String, attrs: &BTreeMap<String, String>) {
    for (k, v) in attrs {
        out.push(' ');
        out.push_str(k);
        out.push('=');
        out.push('"');
        out.push_str(&escape_attr(v));
        out.push('"');
    }
}

fn write_style(out: &mut String, depth: usize, css: &str) {
    push_indent(out, depth);
    out.push_str("<style>");
    // Use CDATA so CSS containing `<` or `&` round-trips without XML escapes.
    out.push_str("<![CDATA[");
    out.push_str(css);
    out.push_str("]]>");
    out.push_str("</style>");
    out.push_str(LINE_ENDING);
}

fn push_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str(INDENT);
    }
}

fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn fmt_num(n: f64) -> String {
    // Emit integers without trailing .0 so canonical output stays compact
    // and matches how most SVG authors write coordinates.
    if n == n.trunc() && n.abs() < 1e16 {
        format!("{}", n as i64)
    } else {
        // Trim trailing zeros so 1.50 -> 1.5; stable across parse/serialize.
        let s = format!("{n}");
        s
    }
}

fn has_any_youeye_usage(doc: &Document) -> bool {
    fn walk(n: &Node) -> bool {
        if !n.base().youeye_attrs.is_empty() {
            return true;
        }
        match n {
            Node::Group(g) => g.children.iter().any(walk),
            Node::Frame(_) => true,
            _ => false,
        }
    }
    doc.children.iter().any(walk)
        || doc
            .extra_attrs
            .keys()
            .any(|k| k.starts_with("xmlns:youeye"))
}

fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|b| *b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_canonical_stable(input: &str) -> String {
        let doc = from_svg(input).expect("first parse");
        let first = to_svg(&doc);
        let doc2 = from_svg(&first).expect("reparse");
        let second = to_svg(&doc2);
        assert_eq!(first, second, "canonical output was not byte-stable");
        first
    }

    #[test]
    fn empty_svg_has_declaration_and_root() {
        let out = to_svg(&Document::default());
        assert!(out.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
        assert!(out.contains("<svg"));
        assert!(out.contains("xmlns=\"http://www.w3.org/2000/svg\""));
    }

    #[test]
    fn rect_round_trips() {
        let input = r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 200" width="320" height="200">
  <rect fill="#0052cc" height="100" width="200" x="10" y="20"/>
</svg>"##;
        let out = assert_canonical_stable(input);
        assert!(out.contains(r##"<rect fill="#0052cc" height="100" width="200" x="10" y="20"/>"##));
    }

    #[test]
    fn ellipse_round_trips() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg"><ellipse cx="50" cy="60" rx="20" ry="30"/></svg>"#;
        let out = assert_canonical_stable(input);
        assert!(out.contains(r#"<ellipse cx="50" cy="60" rx="20" ry="30"/>"#));
    }

    #[test]
    fn path_round_trips() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0 L10 10 Z"/></svg>"#;
        let _ = assert_canonical_stable(input);
    }

    #[test]
    fn text_round_trips_with_content() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg"><text x="10" y="20" font-size="14">hello</text></svg>"#;
        let out = assert_canonical_stable(input);
        assert!(out.contains(">hello</text>"));
    }

    #[test]
    fn nested_groups_round_trip() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg">
  <g id="outer">
    <g id="inner">
      <rect x="0" y="0" width="10" height="10"/>
    </g>
  </g>
</svg>"#;
        let out = assert_canonical_stable(input);
        assert!(out.contains(r#"<g id="outer">"#));
        assert!(out.contains(r#"<g id="inner">"#));
    }

    #[test]
    fn youeye_attrs_preserved_and_prefix_sorted() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:youeye="https://youeye.app/ns">
  <g youeye:layout="flex">
    <rect x="0" y="0" width="10" height="10"/>
  </g>
</svg>"#;
        let out = assert_canonical_stable(input);
        assert!(out.contains(r#"youeye:layout="flex""#));
    }

    #[test]
    fn style_block_populates_tokens_and_variables() {
        let input = r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg">
  <style>:root { --token-brand-primary: #0052cc; --var-rhythm: 8px; }</style>
</svg>"##;
        let doc = from_svg(input).unwrap();
        assert_eq!(doc.tokens.get("brand-primary"), Some("#0052cc"));
        assert_eq!(doc.variables.get("rhythm"), Some("8px"));
        assert!(doc.raw_style.is_some());

        let out = to_svg(&doc);
        assert!(out.contains("<style>"));
        assert!(out.contains("--token-brand-primary"));
        assert_canonical_stable(input);
    }

    #[test]
    fn unknown_attrs_preserved_verbatim() {
        let input = r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg"><rect data-custom="x" x="0" y="0" width="1" height="1"/></svg>"#;
        let out = assert_canonical_stable(input);
        assert!(out.contains(r#"data-custom="x""#));
    }

    #[test]
    fn extract_root_declarations_basic() {
        let css = ":root { --token-foo: red; --var-bar: 4px; color: blue; }";
        let (tokens, variables) = extract_root_declarations(css);
        assert_eq!(tokens.get("foo"), Some("red"));
        assert_eq!(variables.get("bar"), Some("4px"));
        assert_eq!(tokens.len(), 1);
        assert_eq!(variables.len(), 1);
    }

    #[test]
    fn paint_hex_parses() {
        assert_eq!(
            parse_paint("#ff0000"),
            Paint::Solid(Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0
            })
        );
    }

    #[test]
    fn paint_none_parses() {
        assert_eq!(parse_paint("none"), Paint::None);
        assert_eq!(parse_paint("NONE"), Paint::None);
    }

    #[test]
    fn paint_unknown_is_raw() {
        let p = parse_paint("var(--token-brand)");
        assert_eq!(p, Paint::Raw("var(--token-brand)".into()));
    }
}
