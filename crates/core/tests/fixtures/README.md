# Test fixtures

Real data captured by the production Searchlight service, trimmed to what the
tests need. Regenerate only if the source formats change.

| File | Source |
|---|---|
| `loadout_prismatic_exotic_class_item.json` | One `build_segment.loadout_state` from the 2026-09-20 production DB (instance 16892402723): Prismatic Warlock, Solipsism exotic class item, two 2-piece armor sets, duplicate armor mods. |
| `manifest_items_subset.json` | `DestinyInventoryItemDefinition` entries for every item/plug hash in the loadout above, reduced to the fields `catalog::parse_items` reads. |
| `manifest_item_sets_subset.json` | `DestinyEquipableItemSetDefinition` entries for the armor sets in the loadout. |
| `pgcr_enveloped_gm_conquest.json` | A full `GetPostGameCarnageReport` response (with envelope), copied from the production repo's own test fixtures. |
| `pgcr_archive_bare.json.gz` | One `pgcr_raw.raw` blob (gzip, bare report, no envelope) as the archive stores it. |
