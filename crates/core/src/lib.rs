//! Pure domain logic for `searchlight-cli`.
//!
//! This crate has no I/O. It knows how to:
//!
//! * read the source formats Searchlight persisted ([`loadout`], [`pgcr`]);
//! * model the parts of the Bungie manifest we use ([`catalog`]);
//! * classify socket plugs ([`classify`]) and project a loadout to its
//!   gameplay-relevant [`build`] with a stable content fingerprint.
//!
//! Everything that touches files or databases lives in `searchlight-import`
//! and `searchlight-db`.

pub mod build;
pub mod catalog;
pub mod class;
pub mod classify;
pub mod fingerprint;
pub mod loadout;
pub mod pgcr;
pub mod slot;

pub use build::{Build, Component, ComponentKind, extract_build, loadout_id};
pub use catalog::{Catalog, ItemDef};
pub use class::{Element, GuardianClass};
pub use classify::{PlugKind, classify_plug};
pub use loadout::LoadoutState;
pub use slot::{Slot, SlotGroup};
