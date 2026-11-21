//! Builds the repository's part cards into the binary: every
//! `data/sandbox/parts/*.toml`, in name order.

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/sandbox/parts");
    println!("cargo::rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut out = String::from(
        "/// The built-in part card files: `(file name, contents)`.\npub const CARD_FILES: &[(&str, &str)] = &[\n",
    );
    for f in &files {
        println!("cargo::rerun-if-changed={}", f.display());
        let name = f.file_name().unwrap().to_string_lossy();
        let path = f.canonicalize().unwrap();
        writeln!(
            out,
            "    ({name:?}, include_str!({:?})),",
            path.display().to_string()
        )
        .unwrap();
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("cards.rs");
    std::fs::write(dest, out).unwrap();
}
