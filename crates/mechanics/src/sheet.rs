//! Google Sheets tabs saved as their static HTML (`htmlview/sheet`), read
//! as grids.
//!
//! The HTML holds exactly what the sheet displays, merged cells included;
//! the CSV export infers a type per column and drops cells that do not
//! fit it. A merged cell is stored once, at its top-left position (its
//! *anchor*); the positions it covers point back at it.

use scraper::{ElementRef, Html, Node, Selector};

/// A cell's text (lines joined by `\n`), the runs of it the sheet colours,
/// and its span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    /// `(text, #rrggbb)` for every non-empty run set in a colour.
    pub marks: Vec<(String, String)>,
    pub rows: usize,
    pub cols: usize,
}

/// A tab as a grid of anchored cells.
#[derive(Debug, Default)]
pub struct Grid {
    cells: Vec<(usize, usize, Cell)>,
    at: Vec<Vec<Option<usize>>>,
}

impl Grid {
    /// Parse a saved `htmlview/sheet` page — its rows may be split over
    /// several tables. Row headers and the frozen-pane handle row are left
    /// out.
    #[must_use]
    pub fn parse(html: &str) -> Self {
        let doc = Html::parse_document(html);
        let table = Selector::parse("table.waffle").expect("valid selector");
        let row_sel = Selector::parse("tbody > tr").expect("valid selector");
        let mut grid = Self::default();
        let mut r = 0;
        // Long tabs are split into consecutive tables of rows.
        for tr in doc.select(&table).flat_map(|t| t.select(&row_sel)) {
            let tds: Vec<ElementRef> = tr
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|e| e.value().name() == "td")
                .collect();
            if tds.is_empty() || tds.iter().any(|td| has_class(*td, "freezebar-cell")) {
                continue;
            }
            let mut c = 0;
            for td in tds {
                while grid.covered(r, c) {
                    c += 1;
                }
                let span = |name: &str| {
                    td.value()
                        .attr(name)
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1)
                        .max(1)
                };
                let (rows, cols) = (span("rowspan"), span("colspan"));
                let mut text = String::new();
                let mut marks = Vec::new();
                collect(td, None, &mut text, &mut marks);
                grid.place(
                    r,
                    c,
                    Cell {
                        text: tidy(&text),
                        marks: marks
                            .into_iter()
                            .map(|(t, m)| (tidy(&t), m))
                            .filter(|(t, _)| !t.is_empty())
                            .collect(),
                        rows,
                        cols,
                    },
                );
                c += cols;
            }
            r += 1;
        }
        grid
    }

    fn covered(&self, r: usize, c: usize) -> bool {
        self.at
            .get(r)
            .and_then(|row| row.get(c))
            .is_some_and(Option::is_some)
    }

    fn place(&mut self, r: usize, c: usize, cell: Cell) {
        let id = self.cells.len();
        for dr in 0..cell.rows {
            if self.at.len() <= r + dr {
                self.at.resize(r + dr + 1, Vec::new());
            }
            let row = &mut self.at[r + dr];
            if row.len() < c + cell.cols {
                row.resize(c + cell.cols, None);
            }
            for slot in &mut row[c..c + cell.cols] {
                *slot = Some(id);
            }
        }
        self.cells.push((r, c, cell));
    }

    #[must_use]
    pub fn rows(&self) -> usize {
        self.at.len()
    }

    #[must_use]
    pub fn cols(&self) -> usize {
        self.at.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// The anchor id at a position, merged positions included.
    #[must_use]
    pub fn id_at(&self, r: usize, c: usize) -> Option<usize> {
        self.at.get(r)?.get(c).copied().flatten()
    }

    /// An anchored cell and its position.
    #[must_use]
    pub fn anchor(&self, id: usize) -> (usize, usize, &Cell) {
        let (r, c, cell) = &self.cells[id];
        (*r, *c, cell)
    }

    /// Every anchored cell with text, in reading order: `(id, row, col, cell)`.
    pub fn anchors(&self) -> impl Iterator<Item = (usize, usize, usize, &Cell)> {
        self.cells
            .iter()
            .enumerate()
            .filter(|(_, (_, _, cell))| !cell.text.is_empty())
            .map(|(id, (r, c, cell))| (id, *r, *c, cell))
    }
}

fn has_class(e: ElementRef, class: &str) -> bool {
    e.value()
        .attr("class")
        .is_some_and(|c| c.split_whitespace().any(|x| x == class))
}

/// The colour a `style` attribute sets, as `#rrggbb`.
fn colour(style: &str) -> Option<String> {
    style.split(';').find_map(|decl| {
        let (k, v) = decl.split_once(':')?;
        (k.trim() == "color").then(|| v.trim().to_ascii_lowercase())
    })
}

/// Append an element's text (`<br>` as a newline) and its coloured runs.
fn collect(
    e: ElementRef,
    inherited: Option<&str>,
    text: &mut String,
    marks: &mut Vec<(String, String)>,
) {
    let own = e.value().attr("style").and_then(colour);
    let current = own.as_deref().or(inherited);
    for child in e.children() {
        match child.value() {
            Node::Text(t) => {
                text.push_str(t);
                if let Some(m) = current {
                    match marks.last_mut() {
                        Some((run, mark)) if mark == m && own.is_none() => run.push_str(t),
                        _ => marks.push((t.to_string(), m.to_string())),
                    }
                }
            }
            Node::Element(el) if el.name() == "br" => text.push('\n'),
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(child) {
                    collect(child, current, text, marks);
                }
            }
            _ => {}
        }
    }
}

/// Non-breaking spaces as spaces, lines trimmed at the end, the whole trimmed.
fn tidy(s: &str) -> String {
    s.replace('\u{a0}', " ")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<html><body><table class="waffle"><tbody>
        <tr><th>1</th><td colspan="3">Armor Set Bonuses</td></tr>
        <tr><th></th><td class="freezebar-cell"></td><td class="freezebar-cell"></td></tr>
        <tr><th>2</th><td rowspan="2">AION Adapter<br><br><span style="font-style:italic;">Kepler</span></td><td></td>
            <td><span style="color:#81cf82;">2 Piece | Force Absorption<br></span>Upon a <span style="color:#A481E1;">Rocket Launcher</span> kill</td></tr>
        <tr><th>3</th><td></td><td>4 Piece&nbsp;| Reactive Shock</td></tr>
    </tbody></table></body></html>"#;

    #[test]
    fn merged_cells_are_anchored_once_and_cover_their_span() {
        let g = Grid::parse(PAGE);
        assert_eq!(
            (g.rows(), g.cols()),
            (3, 3),
            "the frozen-pane row is skipped"
        );
        let set = g.id_at(1, 0).unwrap();
        assert_eq!(g.id_at(2, 0), Some(set), "the set name spans two rows");
        let (r, c, cell) = g.anchor(set);
        assert_eq!((r, c, cell.text.as_str()), (1, 0, "AION Adapter\n\nKepler"));
        let (_, _, bonus) = g.anchor(g.id_at(1, 2).unwrap());
        assert_eq!(
            bonus.text,
            "2 Piece | Force Absorption\nUpon a Rocket Launcher kill"
        );
        assert_eq!(
            bonus.marks,
            [
                (
                    "2 Piece | Force Absorption".to_string(),
                    "#81cf82".to_string()
                ),
                ("Rocket Launcher".to_string(), "#a481e1".to_string())
            ]
        );
        let (_, c, four) = g.anchor(g.id_at(2, 2).unwrap());
        assert_eq!((c, four.text.as_str()), (2, "4 Piece | Reactive Shock"));
        assert_eq!(g.anchors().count(), 4, "empty cells are not listed");
    }

    #[test]
    fn rows_continue_across_the_tables_a_long_tab_is_split_into() {
        let g = Grid::parse(
            r#"<table class="waffle"><tbody><tr><td>Slideshot</td><td>Refills 15%</td></tr></tbody></table>
               <table class="waffle"><tbody><tr><td>Slideways</td><td>Refills 15% of Magazine</td></tr></tbody></table>"#,
        );
        assert_eq!(g.rows(), 2);
        assert_eq!(g.anchor(g.id_at(1, 0).unwrap()).2.text, "Slideways");
    }
}
