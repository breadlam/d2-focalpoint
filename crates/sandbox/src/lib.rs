//! The build model (`docs/build-model.md`): Destiny 2's build mechanics as
//! rules over a closed vocabulary.
//!
//! * [`ontology`] — the vocabulary: events, states, the core verbs'
//!   parameters (`data/sandbox/ontology.toml`);
//! * [`card`] — part cards, each part's rules, modifiers and counters
//!   (`data/sandbox/parts/`; format in `docs/sandbox.md`);
//! * [`check`] — cards checked against the vocabulary, and their coverage of
//!   the parts recorded builds use.

pub mod card;
pub mod check;
pub mod ontology;

include!(concat!(env!("OUT_DIR"), "/cards.rs"));

/// The built-in cards, with the files they came from.
pub fn builtin_cards() -> anyhow::Result<Vec<check::Located>> {
    let mut out = Vec::new();
    for (file, text) in CARD_FILES {
        let cards = card::parse(text).map_err(|e| anyhow::anyhow!("{file}: {e}"))?;
        out.extend(cards.into_iter().map(|card| check::Located {
            file: (*file).to_string(),
            card,
        }));
    }
    Ok(out)
}

/// The cards in a directory's `*.toml` files.
pub fn cards_in(dir: &std::path::Path) -> anyhow::Result<Vec<check::Located>> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f)?;
        let name = f
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let cards = card::parse(&text).map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        out.extend(cards.into_iter().map(|card| check::Located {
            file: name.clone(),
            card,
        }));
    }
    Ok(out)
}
