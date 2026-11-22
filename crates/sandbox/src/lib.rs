//! The build model (`docs/build-model.md`): Destiny 2's build mechanics as
//! rules over a closed vocabulary.
//!
//! * [`ontology`]: the vocabulary of events, states and the core verbs'
//!   parameters (`data/sandbox/ontology.toml`);
//! * [`rules`]: the rule base, the rules and modifiers build parts grant
//!   (`data/sandbox/rules/`; format in `docs/sandbox.md`);
//! * [`check`]: the rule base checked against the vocabulary, and its
//!   coverage of the parts recorded builds use;
//! * [`economy`]: ability cooldowns, stat curves and what a chunk of
//!   energy is worth (from `focalpoint mechanics import`);
//! * [`scenario`]: the environment a build is evaluated in: packs, ranks,
//!   health and durability;
//! * [`compile`]: a loadout's records and parameters;
//! * [`fluid`]: the mean-field analysis: generations, loop gains and
//!   thresholds;
//! * [`variables`]: the values no source gives, as variables with guesses
//!   (`data/sandbox/variables.toml`).

pub mod check;
pub mod compile;
pub mod economy;
pub mod fluid;
pub mod ontology;
pub mod rules;
pub mod scenario;
pub mod variables;

use rules::RuleBase;

include!(concat!(env!("OUT_DIR"), "/rule_files.rs"));

/// The built-in rule base.
pub fn builtin_rules() -> anyhow::Result<RuleBase> {
    let mut base = RuleBase::default();
    for (file, text) in RULE_FILES {
        base.add(file, text)?;
    }
    Ok(base)
}

/// The rule base in a directory's `*.toml` files.
pub fn rules_in(dir: &std::path::Path) -> anyhow::Result<RuleBase> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    let mut base = RuleBase::default();
    for f in files {
        let name = f
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        base.add(&name, &std::fs::read_to_string(&f)?)?;
    }
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_rules_pass_their_checks() {
        let o = ontology::Ontology::builtin().unwrap();
        let base = builtin_rules().unwrap();
        assert!(!base.rules.is_empty());
        let vars = variables::Variables::builtin().unwrap();
        let report = check::check(&o, &base, &vars);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
    }
}
