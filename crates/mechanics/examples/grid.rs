//! Print a saved Google Sheets tab as the grid the importer reads:
//! `cargo run -p focalpoint-mechanics --example grid -- TAB.html [FROM_ROW [ROWS]]`.
//! Each anchored cell prints as `[col]text` (first 50 characters); `^` marks a
//! position covered by a merged cell.

use focalpoint_mechanics::sheet::Grid;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .ok_or_else(|| anyhow::anyhow!("usage: grid TAB.html [FROM_ROW [ROWS]]"))?;
    let from: usize = args.get(2).map_or(Ok(0), |s| s.parse())?;
    let rows: usize = args.get(3).map_or(Ok(40), |s| s.parse())?;
    let grid = Grid::parse(&std::fs::read_to_string(path)?);
    println!("{} rows, {} columns", grid.rows(), grid.cols());
    for r in from..(from + rows).min(grid.rows()) {
        let mut line = Vec::new();
        for c in 0..grid.cols() {
            let Some(id) = grid.id_at(r, c) else { continue };
            let (ar, ac, cell) = grid.anchor(id);
            if cell.text.is_empty() {
                continue;
            }
            if (ar, ac) == (r, c) {
                let text: String = cell.text.replace('\n', "⏎").chars().take(50).collect();
                line.push(format!("[{c}]{text}"));
            } else {
                line.push(format!("[{c}]^"));
            }
        }
        println!("{r} {}", line.join(" "));
    }
    Ok(())
}
