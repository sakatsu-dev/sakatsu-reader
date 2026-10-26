//! Typed, inert KanjiVG geometry.
//!
//! Downloaded SVG is never handed to the webview. Setup parses the small, known subset used by
//! KanjiVG and writes this type as JSON; the Reference command reads that JSON lazily. Keeping the
//! trust boundary here means neither a future upstream edit nor a damaged archive can introduce
//! script, style, URL, event-handler, or foreign-object markup into the DOM.

use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KanjiDiagram {
    pub strokes: Vec<KanjiStroke>,
    pub numbers: Vec<StrokeNumber>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KanjiStroke {
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrokeNumber {
    pub number: u16,
    pub x: f32,
    pub y: f32,
}

/// Read the setup-produced diagram for one character.
pub fn load_diagram(dir: &Path, character: char) -> Option<KanjiDiagram> {
    let file = dir.join(format!("{:05x}.json", character as u32));
    let bytes = std::fs::read(file).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Parse the safe subset of one KanjiVG SVG into inert geometry.
pub fn parse_diagram(svg: &[u8]) -> Result<KanjiDiagram, String> {
    if svg.len() > 256 * 1024 {
        return Err("diagram is unexpectedly large".into());
    }
    let source = std::str::from_utf8(svg).map_err(|_| "diagram is not UTF-8")?;
    let mut reader = Reader::from_str(source);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut strokes = Vec::new();
    let mut numbers = Vec::new();
    let mut number_at: Option<(f32, f32)> = None;
    let mut number_text = String::new();

    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                let qualified = element.name();
                let name = local_name(qualified.as_ref());
                match name {
                    "svg" | "g" => reject_external_attributes(&element)?,
                    "path" => {
                        reject_external_attributes(&element)?;
                        let path = attribute(&element, "d")?
                            .ok_or_else(|| "stroke path has no geometry".to_string())?;
                        validate_path(&path)?;
                        strokes.push(KanjiStroke { path });
                    }
                    "text" => {
                        reject_external_attributes(&element)?;
                        let (x, y) = number_coordinates(&element)?;
                        number_at = Some((x, y));
                        number_text.clear();
                    }
                    other => return Err(format!("unexpected SVG element {other}")),
                }
            }
            Ok(Event::Text(text)) if number_at.is_some() => {
                number_text.push_str(&text);
            }
            Ok(Event::End(element)) if local_name(element.name().as_ref()) == "text" => {
                let (x, y) = number_at
                    .take()
                    .ok_or_else(|| "stroke number ended without coordinates".to_string())?;
                let number = number_text
                    .trim()
                    .parse::<u16>()
                    .map_err(|_| "stroke number is not an integer".to_string())?;
                numbers.push(StrokeNumber { number, x, y });
            }
            // **KanjiVG ships every diagram with a DOCTYPE**: the SVG 1.0 public identifier and an
            // internal subset that declares only its `kvg:` attributes. Refusing every DOCTYPE
            // refused the whole pinned release, and the essentials install with it (found in a
            // desktop check, 2026-10-13: all 6,702 diagrams carry one). quick-xml neither fetches
            // a DTD nor expands entities a DTD declares, but a diagram that declares an entity is
            // still refused — nothing legitimate needs one.
            Ok(Event::DocType(declaration)) => {
                if declaration.contains("<!ENTITY") {
                    return Err("entity declarations are not allowed inside a diagram".into());
                }
            }
            Ok(Event::PI(_)) => {
                return Err("declarations are not allowed inside a diagram".into());
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(error.to_string()),
            _ => {}
        }
    }

    if strokes.is_empty() || strokes.len() != numbers.len() {
        return Err(UNNUMBERED.into());
    }
    for (index, number) in numbers.iter().enumerate() {
        if number.number as usize != index + 1 {
            return Err(NOT_CONSECUTIVE.into());
        }
    }
    Ok(KanjiDiagram { strokes, numbers })
}

const UNNUMBERED: &str = "diagram must number every stroke exactly once";
const NOT_CONSECUTIVE: &str = "stroke numbers are not consecutive";

/// Whether a refusal is the diagram's own drawing being inconsistent — strokes and numbers out of
/// step — rather than anything unsafe in it.
///
/// An importer may skip such a diagram and keep the rest: KanjiVG's r20250816 release draws 參
/// (053c3) with strokes 9 and 10 repeating 7 and 8, so 13 paths carry 11 numbers, and refusing
/// the whole archive for it left the "Kanji details" essential uninstallable. (r20260714 fixed 參
/// upstream and imports without a skip; the allowance stays for the next release.) Every other refusal
/// — active content, external references, entity declarations, oversized or non-UTF-8 input —
/// still refuses the archive.
pub fn is_drawing_defect(error: &str) -> bool {
    error == UNNUMBERED || error == NOT_CONSECUTIVE
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn attribute(
    element: &quick_xml::events::BytesStart<'_>,
    wanted: &str,
) -> Result<Option<String>, String> {
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute.map_err(|error| error.to_string())?;
        if local_name(attribute.key.as_ref()) == wanted {
            return Ok(Some(attribute.value.into_owned()));
        }
    }
    Ok(None)
}

fn reject_external_attributes(element: &quick_xml::events::BytesStart<'_>) -> Result<(), String> {
    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute.map_err(|error| error.to_string())?;
        let raw_key = attribute.key.as_ref();
        if raw_key == "xmlns" || raw_key.starts_with("xmlns:") {
            continue;
        }
        let key = local_name(raw_key);
        let value = &attribute.value;
        if key.eq_ignore_ascii_case("href")
            || key.starts_with("on")
            || value.to_ascii_lowercase().contains("url(")
            || value.contains("://")
        {
            return Err("diagram contains an external or active reference".into());
        }
    }
    Ok(())
}

fn number_coordinates(element: &quick_xml::events::BytesStart<'_>) -> Result<(f32, f32), String> {
    match (attribute(element, "x")?, attribute(element, "y")?) {
        (Some(x), Some(y)) => Ok((checked_coordinate(&x)?, checked_coordinate(&y)?)),
        (None, None) => {
            // KanjiVG's stable archive positions stroke numbers with an SVG matrix rather than
            // x/y attributes. Only the translation terms affect a text anchor; accepting the
            // fixed six-number form keeps the parser narrow and does not preserve raw markup.
            let transform = attribute(element, "transform")?
                .ok_or_else(|| "stroke number has no coordinates".to_string())?;
            let arguments = transform
                .strip_prefix("matrix(")
                .and_then(|value| value.strip_suffix(')'))
                .ok_or_else(|| "stroke-number transform is not a matrix".to_string())?;
            let values: Vec<&str> = arguments
                .split(|character: char| character.is_ascii_whitespace() || character == ',')
                .filter(|value| !value.is_empty())
                .collect();
            if values.len() != 6 {
                return Err("stroke-number matrix does not have six values".into());
            }
            Ok((
                checked_coordinate(values[4])?,
                checked_coordinate(values[5])?,
            ))
        }
        _ => Err("stroke number has incomplete coordinates".into()),
    }
}

fn checked_coordinate(value: &str) -> Result<f32, String> {
    let value = value
        .parse::<f32>()
        .map_err(|_| "stroke-number coordinate is not numeric".to_string())?;
    if !value.is_finite() || !(-1000.0..=1000.0).contains(&value) {
        return Err("stroke-number coordinate is outside the diagram".into());
    }
    Ok(value)
}

fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.len() > 16 * 1024 {
        return Err("stroke path has an invalid length".into());
    }
    let allowed = |character: char| {
        character.is_ascii_digit()
            || character.is_ascii_whitespace()
            || matches!(
                character,
                'M' | 'm'
                    | 'L'
                    | 'l'
                    | 'H'
                    | 'h'
                    | 'V'
                    | 'v'
                    | 'C'
                    | 'c'
                    | 'S'
                    | 's'
                    | 'Q'
                    | 'q'
                    | 'T'
                    | 't'
                    | 'A'
                    | 'a'
                    | 'Z'
                    | 'z'
                    | '.'
                    | ','
                    | '-'
                    | '+'
                    | 'e'
                    | 'E'
            )
    };
    if !path.chars().all(allowed) {
        return Err("stroke path contains non-path data".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_becomes_typed_strokes_and_numbers() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><g><path d="M1,2 C3,4 5,6 7,8"/></g><text transform="matrix(1 0 0 1 9 10)">1</text></svg>"#;
        let diagram = parse_diagram(svg).expect("valid diagram");
        assert_eq!(diagram.strokes[0].path, "M1,2 C3,4 5,6 7,8");
        assert_eq!(
            diagram.numbers[0],
            StrokeNumber {
                number: 1,
                x: 9.0,
                y: 10.0
            }
        );
    }

    #[test]
    fn explicit_number_coordinates_remain_supported() {
        let svg = br#"<svg><path d="M0 0L1 1"/><text x="4.5" y="6">1</text></svg>"#;
        let diagram = parse_diagram(svg).expect("explicit coordinates");
        assert_eq!(diagram.numbers[0].x, 4.5);
        assert_eq!(diagram.numbers[0].y, 6.0);
    }

    /// The prolog KanjiVG's r20250816 release puts on every diagram, verbatim up to the subset's
    /// closing bracket (`kanjivg-20250816-main.zip`, CC BY-SA 3.0, Ulrich Apel). r20260714's is
    /// byte-identical.
    const KANJIVG_PROLOG: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.0//EN" "http://www.w3.org/TR/2001/REC-SVG-20010904/DTD/svg10.dtd" [
<!ATTLIST g
xmlns:kvg CDATA #FIXED "http://kanjivg.tagaini.net"
kvg:element CDATA #IMPLIED
kvg:variant CDATA #IMPLIED
kvg:partial CDATA #IMPLIED
kvg:original CDATA #IMPLIED
kvg:part CDATA #IMPLIED
kvg:number CDATA #IMPLIED
kvg:tradForm CDATA #IMPLIED
kvg:radicalForm CDATA #IMPLIED
kvg:position CDATA #IMPLIED
kvg:radical CDATA #IMPLIED
kvg:phon CDATA #IMPLIED >
<!ATTLIST path
xmlns:kvg CDATA #FIXED "http://kanjivg.tagaini.net"
kvg:type CDATA #IMPLIED >
]>
"#;

    #[test]
    fn kanjivgs_own_doctype_is_accepted() {
        let svg = format!(
            "{KANJIVG_PROLOG}<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0L1 1\"/>\
             <text x=\"1\" y=\"2\">1</text></svg>"
        );
        assert!(parse_diagram(svg.as_bytes()).is_ok());
    }

    #[test]
    fn a_doctype_that_declares_an_entity_is_still_refused() {
        let svg = br#"<!DOCTYPE svg [<!ENTITY a "aaaaaaaa">]><svg><path d="M0 0L1 1"/><text x="1" y="2">1</text></svg>"#;
        assert_eq!(
            parse_diagram(svg).err().as_deref(),
            Some("entity declarations are not allowed inside a diagram")
        );
        let pi = br#"<?xml-stylesheet href="x.css"?><svg><path d="M0 0L1 1"/><text x="1" y="2">1</text></svg>"#;
        assert!(parse_diagram(pi).is_err());
    }

    #[test]
    fn only_a_drawing_defect_may_be_skipped_by_an_importer() {
        let unnumbered = parse_diagram(br#"<svg><path d="M0 0"/></svg>"#).unwrap_err();
        assert!(is_drawing_defect(&unnumbered));
        let unsafe_reasons = [
            parse_diagram(br#"<svg><script>alert(1)</script></svg>"#).unwrap_err(),
            parse_diagram(br#"<!DOCTYPE svg [<!ENTITY a "a">]><svg/>"#).unwrap_err(),
            parse_diagram(br#"<svg><path d="url(https://bad)"/><text x="1" y="2">1</text></svg>"#)
                .unwrap_err(),
        ];
        for reason in unsafe_reasons {
            assert!(!is_drawing_defect(&reason), "{reason}");
        }
    }

    #[test]
    fn active_or_malformed_svg_is_rejected() {
        assert!(parse_diagram(br#"<svg><script>alert(1)</script></svg>"#).is_err());
        assert!(parse_diagram(
            br#"<svg><path d="url(https://bad)"/><text x="1" y="2">1</text></svg>"#
        )
        .is_err());
        assert!(parse_diagram(br#"<svg><path d="M0 0"/></svg>"#).is_err());
    }
}
