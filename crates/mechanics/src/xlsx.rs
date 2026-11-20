//! Excel workbooks (`.xlsx`) as their sheets' displayed values (the cached
//! results of formulas), hyperlinks and notes.
//!
//! General spreadsheet readers give cell values only; the community's
//! workbooks keep much of their meaning in hyperlinks (shared loadouts)
//! and notes, so the workbook's XML is read directly.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use zip::ZipArchive;

/// A sheet: positions are `(row, column)`, both from 0.
#[derive(Debug, Default)]
pub struct Sheet {
    pub name: String,
    pub cells: BTreeMap<(u32, u32), String>,
    pub links: HashMap<(u32, u32), String>,
    pub notes: HashMap<(u32, u32), String>,
}

impl Sheet {
    #[must_use]
    pub fn get(&self, row: u32, col: u32) -> Option<&str> {
        self.cells.get(&(row, col)).map(String::as_str)
    }

    /// The number of rows (one past the last row with a value).
    #[must_use]
    pub fn rows(&self) -> u32 {
        self.cells.keys().next_back().map_or(0, |(r, _)| r + 1)
    }
}

/// `"AB28"` as `(27, 27)`: row and column from 0.
#[must_use]
pub fn cell_ref(s: &str) -> Option<(u32, u32)> {
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = s.split_at(split);
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let col = letters
        .bytes()
        .fold(0u32, |n, b| n * 26 + u32::from(b - b'A' + 1))
        - 1;
    let row = digits.parse::<u32>().ok()?.checked_sub(1)?;
    Some((row, col))
}

/// Read every sheet of a workbook.
pub fn read(path: &Path) -> Result<Vec<Sheet>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut zip = ZipArchive::new(file).with_context(|| format!("reading {}", path.display()))?;
    let mut part = |name: &str| -> Result<Option<String>> {
        let Ok(mut f) = zip.by_name(name) else {
            return Ok(None);
        };
        let mut s = String::new();
        f.read_to_string(&mut s)
            .with_context(|| format!("reading {name}"))?;
        Ok(Some(s))
    };
    let shared = part("xl/sharedStrings.xml")?
        .map(|x| shared_strings(&x))
        .transpose()?
        .unwrap_or_default();
    let workbook = part("xl/workbook.xml")?.context("no xl/workbook.xml")?;
    let book_rels = rels(
        &part("xl/_rels/workbook.xml.rels")?.unwrap_or_default(),
        "xl",
    )?;
    let mut out = Vec::new();
    for (name, id) in sheet_list(&workbook)? {
        let Some(rel) = book_rels.get(&id) else {
            continue;
        };
        let Some(xml) = part(&rel.target)? else {
            continue;
        };
        let (dir, file) = rel.target.rsplit_once('/').unwrap_or(("", &rel.target));
        let sheet_rels = rels(
            &part(&format!("{dir}/_rels/{file}.rels"))?.unwrap_or_default(),
            dir,
        )?;
        let mut sheet = parse_sheet(&xml, &shared, &sheet_rels)?;
        sheet.name = name;
        for rel in sheet_rels
            .values()
            .filter(|r| r.kind.ends_with("/comments"))
        {
            if let Some(x) = part(&rel.target)? {
                sheet.notes.extend(comments(&x)?);
            }
        }
        out.push(sheet);
    }
    Ok(out)
}

/// A relationship: its target (an archive path, or a URL when external)
/// and its type.
#[derive(Debug)]
struct Rel {
    target: String,
    kind: String,
}

fn attr(e: &BytesStart, key: &str) -> Result<Option<String>> {
    for a in e.attributes() {
        let a = a?;
        if a.key.as_ref() == key {
            return Ok(Some(
                a.normalized_value(XmlVersion::Implicit1_0)?.into_owned(),
            ));
        }
    }
    Ok(None)
}

/// Resolve `target` against the directory `base` of the part referring to it.
fn resolve(base: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn rels(xml: &str, base: &str) -> Result<HashMap<String, Rel>> {
    let mut out = HashMap::new();
    let mut r = Reader::from_str(xml);
    loop {
        match r.read_event()? {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == "Relationship" => {
                let (Some(id), Some(target)) = (attr(&e, "Id")?, attr(&e, "Target")?) else {
                    continue;
                };
                let external = attr(&e, "TargetMode")?.as_deref() == Some("External");
                let target = if external {
                    target
                } else {
                    resolve(base, &target)
                };
                out.insert(
                    id,
                    Rel {
                        target,
                        kind: attr(&e, "Type")?.unwrap_or_default(),
                    },
                );
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn sheet_list(xml: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut r = Reader::from_str(xml);
    loop {
        match r.read_event()? {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == "sheet" => {
                if let (Some(name), Some(id)) = (attr(&e, "name")?, attr(&e, "r:id")?) {
                    out.push((name, id));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

/// Append a text or entity-reference event to `buf`.
fn push_text(ev: &Event, buf: &mut String) -> Result<()> {
    match ev {
        Event::Text(t) => buf.push_str(&t.xml10_content()),
        Event::CData(t) => buf.push_str(&t.xml10_content()),
        Event::GeneralRef(g) => {
            if let Some(c) = g.resolve_char_ref()? {
                buf.push(c);
            } else {
                buf.push_str(match g.xml10_content().as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                });
            }
        }
        _ => {}
    }
    Ok(())
}

/// The text of `<t>` elements, phonetic runs (`<rPh>`) left out, per
/// `item` element.
fn texts(xml: &str, item: &str) -> Result<Vec<(Option<String>, String)>> {
    let mut out = Vec::new();
    let mut r = Reader::from_str(xml);
    let (mut current, mut key, mut in_t, mut in_phonetic) = (None::<String>, None, false, false);
    loop {
        let ev = r.read_event()?;
        match &ev {
            Event::Start(e) if e.name().as_ref() == item => {
                current = Some(String::new());
                key = attr(e, "ref")?;
            }
            Event::End(e) if e.name().as_ref() == item => {
                if let Some(s) = current.take() {
                    out.push((key.take(), s));
                }
            }
            Event::Empty(e) if e.name().as_ref() == item => {
                out.push((attr(e, "ref")?, String::new()));
            }
            Event::Start(e) if e.name().as_ref() == "t" => in_t = true,
            Event::End(e) if e.name().as_ref() == "t" => in_t = false,
            Event::Start(e) if e.name().as_ref() == "rPh" => in_phonetic = true,
            Event::End(e) if e.name().as_ref() == "rPh" => in_phonetic = false,
            Event::Eof => break,
            _ => {
                if in_t
                    && !in_phonetic
                    && let Some(s) = current.as_mut()
                {
                    push_text(&ev, s)?;
                }
            }
        }
    }
    Ok(out)
}

fn shared_strings(xml: &str) -> Result<Vec<String>> {
    Ok(texts(xml, "si")?.into_iter().map(|(_, s)| s).collect())
}

fn comments(xml: &str) -> Result<HashMap<(u32, u32), String>> {
    Ok(texts(xml, "comment")?
        .into_iter()
        .filter_map(|(r, s)| Some((cell_ref(&r?)?, s.trim().to_string())))
        .filter(|(_, s)| !s.is_empty())
        .collect())
}

fn parse_sheet(xml: &str, shared: &[String], rels: &HashMap<String, Rel>) -> Result<Sheet> {
    let mut sheet = Sheet::default();
    let mut r = Reader::from_str(xml);
    // The cell being read: position, type, and its <v> or inline text.
    let mut cell: Option<((u32, u32), Option<String>, String)> = None;
    let (mut in_v, mut in_t) = (false, false);
    loop {
        let ev = r.read_event()?;
        match &ev {
            Event::Start(e) if e.name().as_ref() == "c" => {
                cell = attr(e, "r")?
                    .as_deref()
                    .and_then(cell_ref)
                    .map(|p| (p, attr(e, "t").ok().flatten(), String::new()));
            }
            Event::End(e) if e.name().as_ref() == "c" => {
                if let Some((pos, kind, raw)) = cell.take() {
                    let value = match kind.as_deref() {
                        Some("s") => raw
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .and_then(|i| shared.get(i).cloned())
                            .unwrap_or_default(),
                        Some("b") => if raw.trim() == "1" { "TRUE" } else { "FALSE" }.to_string(),
                        _ => raw,
                    };
                    if !value.is_empty() {
                        sheet.cells.insert(pos, value);
                    }
                }
            }
            Event::Start(e) if e.name().as_ref() == "v" => in_v = true,
            Event::End(e) if e.name().as_ref() == "v" => in_v = false,
            Event::Start(e) if e.name().as_ref() == "t" => in_t = true,
            Event::End(e) if e.name().as_ref() == "t" => in_t = false,
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == "hyperlink" => {
                let pos = attr(e, "ref")?
                    .as_deref()
                    .and_then(|s| cell_ref(s.split(':').next().unwrap_or(s)));
                let target = attr(e, "r:id")?
                    .and_then(|id| rels.get(&id))
                    .map(|r| r.target.clone());
                if let (Some(pos), Some(target)) = (pos, target) {
                    sheet.links.insert(pos, target);
                }
            }
            Event::Eof => break,
            _ => {
                if (in_v || in_t)
                    && let Some((_, _, raw)) = cell.as_mut()
                {
                    push_text(&ev, raw)?;
                }
            }
        }
    }
    Ok(sheet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_references_count_from_zero() {
        assert_eq!(cell_ref("A1"), Some((0, 0)));
        assert_eq!(cell_ref("AB28"), Some((27, 27)));
        assert_eq!(cell_ref("Z3"), Some((2, 25)));
        assert_eq!(cell_ref("3"), None);
    }

    #[test]
    fn parts_resolve_against_the_referring_part() {
        assert_eq!(
            resolve("xl", "worksheets/sheet9.xml"),
            "xl/worksheets/sheet9.xml"
        );
        assert_eq!(
            resolve("xl/worksheets", "../comments1.xml"),
            "xl/comments1.xml"
        );
        assert_eq!(
            resolve("xl", "/xl/worksheets/sheet1.xml"),
            "xl/worksheets/sheet1.xml"
        );
    }

    #[test]
    fn sheets_give_shared_and_inline_strings_cached_results_links_and_notes() {
        let shared = shared_strings(
            "<sst><si><t>Name</t></si><si><r><t>Heirloom </t></r><r><t>AGS &amp; rings</t></r><rPh><t>x</t></rPh></si></sst>",
        )
        .unwrap();
        assert_eq!(shared, ["Name", "Heirloom AGS & rings"]);
        let rels = rels(
            r#"<Relationships><Relationship Id="rId1" Type="http://x/hyperlink" Target="https://dim.gg/abc/Void" TargetMode="External"/></Relationships>"#,
            "xl/worksheets",
        )
        .unwrap();
        let sheet = parse_sheet(
            r#"<worksheet><sheetData><row r="3">
                 <c r="A3" t="s"><v>1</v></c>
                 <c r="D3"><f>IF(1,2)</f><v>20544.94</v></c>
                 <c r="E3" t="str"><f>"/"</f><v>/</v></c>
                 <c r="F3" t="inlineStr"><is><t>line&#10;two</t></is></c>
                 <c r="G3" t="b"><v>1</v></c>
               </row></sheetData><hyperlinks><hyperlink ref="AC3" r:id="rId1"/></hyperlinks></worksheet>"#,
            &shared,
            &rels,
        )
        .unwrap();
        assert_eq!(sheet.get(2, 0), Some("Heirloom AGS & rings"));
        assert_eq!(
            sheet.get(2, 3),
            Some("20544.94"),
            "a formula's cached result"
        );
        assert_eq!(sheet.get(2, 4), Some("/"));
        assert_eq!(sheet.get(2, 5), Some("line\ntwo"));
        assert_eq!(sheet.get(2, 6), Some("TRUE"));
        assert_eq!(
            sheet.links.get(&(2, 28)).map(String::as_str),
            Some("https://dim.gg/abc/Void")
        );
        let notes = comments(r#"<comments><commentList><comment ref="AB28"><text><r><t>Graviton bug: https://dim.gg/m5wkity/Support</t></r></text></comment></commentList></comments>"#).unwrap();
        assert_eq!(
            notes[&(27, 27)],
            "Graviton bug: https://dim.gg/m5wkity/Support"
        );
    }
}
