//! Flags shared by `search` and `rank`: build facets and run filters.

use searchlight_db::filter::{Facets, RunFilter};

#[derive(Debug, Clone, clap::Args)]
#[command(next_help_heading = "Build facets (combine with AND; repeat a flag to require several)")]
pub struct FacetArgs {
    /// Titan, Hunter or Warlock
    #[arg(long)]
    pub class: Option<String>,
    /// Arc, Solar, Void, Stasis, Strand or Prismatic
    #[arg(long)]
    pub element: Option<String>,
    #[arg(long, value_name = "NAME")]
    pub exotic_armor: Option<String>,
    #[arg(long, value_name = "NAME")]
    pub exotic_weapon: Option<String>,
    #[arg(long = "super", value_name = "NAME")]
    pub super_name: Option<String>,
    #[arg(long, value_name = "NAME")]
    pub aspect: Vec<String>,
    #[arg(long, value_name = "NAME")]
    pub fragment: Vec<String>,
    /// A weapon in any slot
    #[arg(long, value_name = "NAME")]
    pub weapon: Vec<String>,
    /// A weapon perk, trait, origin trait or frame
    #[arg(long, value_name = "NAME")]
    pub perk: Vec<String>,
    /// An armor mod (general, stat or tuning) or weapon mod
    #[arg(long = "mod", value_name = "NAME")]
    pub mods: Vec<String>,
    #[arg(long, value_name = "NAME")]
    pub artifact_perk: Vec<String>,
    /// An armor set with at least two pieces equipped
    #[arg(long = "set", value_name = "NAME")]
    pub sets: Vec<String>,
}

impl FacetArgs {
    pub fn to_facets(&self) -> Facets {
        Facets {
            class: self.class.clone(),
            element: self.element.clone(),
            exotic_armor: self.exotic_armor.clone(),
            exotic_weapon: self.exotic_weapon.clone(),
            super_name: self.super_name.clone(),
            aspects: self.aspect.clone(),
            fragments: self.fragment.clone(),
            weapons: self.weapon.clone(),
            perks: self.perk.clone(),
            mods: self.mods.clone(),
            artifact_perks: self.artifact_perk.clone(),
            armor_sets: self.sets.clone(),
        }
    }
}

#[derive(Debug, Clone, clap::Args)]
#[command(next_help_heading = "Run filters")]
pub struct RunArgs {
    /// Only runs whose activity name or target group contains TEXT
    #[arg(long, value_name = "TEXT")]
    pub activity: Option<String>,
    /// Only players whose Bungie or display name contains TEXT
    #[arg(long, value_name = "TEXT")]
    pub player: Option<String>,
    /// Only solo runs
    #[arg(long)]
    pub solo: bool,
    /// Only runs on or after this date (YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    pub since: Option<String>,
    /// Only runs on or before this date (YYYY-MM-DD)
    #[arg(long, value_name = "DATE")]
    pub until: Option<String>,
}

impl RunArgs {
    pub fn to_filter(&self) -> RunFilter {
        RunFilter {
            activity: self.activity.clone(),
            target_group: None,
            activity_name: None,
            player: self.player.clone(),
            solo: self.solo,
            since: self.since.clone(),
            until: self.until.clone(),
        }
    }
}
