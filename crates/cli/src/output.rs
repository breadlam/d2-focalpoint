//! Tabular output helpers.

use comfy_table::presets::UTF8_FULL_CONDENSED;
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table};

/// Output format for tabular results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Format {
    /// Aligned table for terminals
    #[default]
    Table,
    /// Tab-separated values (header row first)
    Tsv,
}

/// Render rows with a header in the chosen format.
#[must_use]
pub fn render(format: Format, header: &[String], rows: &[Vec<String>]) -> String {
    match format {
        Format::Table => {
            let mut t = Table::new();
            t.load_style(UTF8_FULL_CONDENSED)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_header(header);
            for r in rows {
                t.add_row(r.iter().map(|v| {
                    let numeric = !v.is_empty() && v.parse::<f64>().is_ok();
                    let c = Cell::new(v);
                    if numeric {
                        c.set_alignment(CellAlignment::Right)
                    } else {
                        c
                    }
                }));
            }
            t.to_string()
        }
        Format::Tsv => {
            let esc = |s: &String| s.replace(['\t', '\n'], " ");
            let mut out = header.iter().map(esc).collect::<Vec<_>>().join("\t");
            for r in rows {
                out.push('\n');
                out.push_str(&r.iter().map(esc).collect::<Vec<_>>().join("\t"));
            }
            out
        }
    }
}

/// Format an integer with thousands separators.
#[must_use]
pub fn thousands(n: i64) -> String {
    let s = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(-12_345_678), "-12,345,678");
    }

    #[test]
    fn tsv_escapes_separators() {
        let out = render(
            Format::Tsv,
            &["a".into(), "b".into()],
            &[vec!["x\ty".into(), "1".into()]],
        );
        assert_eq!(out, "a\tb\nx y\t1");
    }

    #[test]
    fn table_contains_values() {
        let out = render(
            Format::Table,
            &["name".into()],
            &[vec!["Nothing Manacles".into()]],
        );
        assert!(out.contains("Nothing Manacles"));
    }
}
