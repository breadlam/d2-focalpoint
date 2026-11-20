// Focalpoint build rankings: a book of exact recipes, read from
// data/focalpoint.json, which `focalpoint site` exports (see docs/site.md).
//
// Build types are ranked by build effect; each listing publishes the
// best-ranked exact build of the type's best loadout (the "top recipe") and
// that loadout's other exact builds (near-identical variants), exactly as
// players equipped them. The pure functions (decoding, filtering,
// facet counts, ranking, recipe differences, URL state) are exported for
// tests/site/test.html; start() wires them to the page.

export const FORMAT = 5;
export const STATS = ['Weapons', 'Health', 'Class', 'Grenade', 'Super', 'Melee'];
export const ARMOR_SLOTS = ['Helmet', 'Gauntlets', 'Chest', 'Legs', 'Class item'];
export const WEAPON_SLOTS = ['Kinetic', 'Energy', 'Power'];
/** Value standing for "no exotic" in the exotic armor and weapon filters. */
export const NONE = '(none)';
/** Below this many captured runs, a scope's ranking is flagged as tentative. */
export const THIN_SCOPE = 500;
const PAGE = 30;

export const DEFAULTS = Object.freeze({
  scope: 'all', cls: '', element: '', armor: '', weapon: '', sup: '',
  aspect1: '', aspect2: '', has: '', min: 3, sort: 'effect',
});
const HASH_KEYS = {
  scope: 'activity', cls: 'class', element: 'element', armor: 'armor', weapon: 'weapon',
  sup: 'super', aspect1: 'aspect', aspect2: 'aspect2', has: 'has', min: 'min', sort: 'sort',
};
/** Sort keys: a function of a scope row. */
export const SORTS = {
  effect: { label: 'Best ranked', key: (r) => r[1] },
  top: { label: 'Top build', key: (r) => r[7][1] },
  raw: { label: 'Raw result', key: (r) => r[2] },
  players: { label: 'Players', key: (r) => r[4] },
  runs: { label: 'Runs', key: (r) => r[5] },
};
/** "Recipe includes" options, grouped by kind in this order. */
export const INCLUDE_KINDS = {
  fragment: 'Fragments', weapon: 'Weapons', weapon_perk: 'Weapon perks',
  armor_mod: 'Armor mods', exotic_perk: 'Exotic armor perks', set_bonus: 'Set bonuses',
  artifact: 'Artifact perks', ability: 'Abilities',
};

/** A log-scale effect as a signed percentage: 0.1 → "+11%". */
export function pct(x) {
  const v = Math.round((Math.exp(x) - 1) * 100);
  if (v === 0) return '±0%';
  return (v > 0 ? '+' : '−') + Math.abs(v) + '%';
}

/** Expand a positional recipe (see docs/site.md) into an object. */
export function decodeRecipe(r) {
  const [id, stats, kit, armor, weapons, sets, artifactItem, artifact, artifactUnlocked] = r;
  const [subclass, sup, classAbility, movement, melee, grenade, aspects, fragments, transcendence] = kit;
  const setEntries = sets.map(([set, pieces, bonuses]) => ({ set, pieces, bonuses }));
  return {
    id, stats,
    kit: { subclass, super: sup, classAbility, movement, melee, grenade, aspects, fragments, transcendence },
    armor: armor.map(([slot, item, exotic, archetype, tuning, statMods, mods, perks]) =>
      ({ slot, item, exotic: exotic === 1, archetype, tuning, statMods, mods, perks })),
    weapons: weapons.map(([slot, item, exotic, frame, perks, origin, mod]) =>
      ({ slot, item, exotic: exotic === 1, frame, perks, origin, mod })),
    sets: setEntries, setBonuses: setEntries.flatMap((e) => e.bonuses), artifactItem, artifact,
    artifactUnlocked: artifactUnlocked === 1,
  };
}

/** Index the exported data. Recipes are decoded on first use. */
export function prepare(data) {
  if (data.format !== FORMAT) {
    throw new Error(`data format ${data.format}, expected ${FORMAT}: re-export with this version of focalpoint`);
  }
  // Scopes start without listings; parts are added as they load.
  for (const s of data.scopes) { s.rows = []; s.loaded = new Set(); }
  const scopes = new Map(data.scopes.map((s) => [s.id, s]));
  const appearances = data.types.map(() => []);
  for (const s of data.scopes) {
    for (const [type, effect, players] of s.summary) appearances[type].push({ scope: s, effect, players });
  }
  const raw = [];
  const decoded = new Map();
  const recipe = (i) => {
    if (!decoded.has(i)) decoded.set(i, decodeRecipe(raw[i]));
    return decoded.get(i);
  };
  return { data, types: data.types, scopes, appearances, raw, recipe, includeCache: new Map(), rowIncludes: new WeakMap() };
}

/** Add a loaded part (`book/<scope>.json`) to its scope; picks are renumbered into the model's recipes. */
export function addPart(model, scope, file, part) {
  if (part.format !== FORMAT) throw new Error(`${file}: data format ${part.format}, expected ${FORMAT}`);
  if (scope.loaded.has(file)) return;
  const offset = model.raw.length;
  model.raw.push(...part.recipes);
  for (const r of part.rows) {
    r[7][0] += offset;
    for (const v of r[8]) v[0] += offset;
    scope.rows.push(r);
  }
  scope.loaded.add(file);
}

/** The parts of a scope that can hold listings with at least `min` players and are not loaded yet. */
export function missingParts(scope, min) {
  return scope.parts
    .filter(([, most, file, count]) => count > 0 && (most === null || min <= most) && !scope.loaded.has(file))
    .map(([, , file]) => file);
}

const name = (model, i) => (i === null || i === undefined ? null : model.data.dict[i]?.[0] ?? null);

/** The searchable components of a decoded recipe: Map(name → kind). */
export function componentsOf(model, r) {
  const m = new Map();
  const add = (i, kind) => { const n = name(model, i); if (n && !m.has(n)) m.set(n, kind); };
  r.kit.fragments.forEach((i) => add(i, 'fragment'));
  [r.kit.grenade, r.kit.melee, r.kit.classAbility, r.kit.movement].forEach((i) => add(i, 'ability'));
  for (const w of r.weapons) { add(w.item, 'weapon'); w.perks.forEach((i) => add(i, 'weapon_perk')); add(w.origin, 'weapon_perk'); }
  for (const a of r.armor) { a.mods.forEach((i) => add(i, 'armor_mod')); if (a.exotic) a.perks.forEach((i) => add(i, 'exotic_perk')); }
  r.setBonuses.forEach((i) => add(i, 'set_bonus'));
  r.artifact.forEach((i) => add(i, 'artifact'));
  return m;
}

/** The searchable components of one listed recipe (cached). */
export function recipeIncludes(model, index) {
  if (!model.includeCache.has(index)) model.includeCache.set(index, componentsOf(model, model.recipe(index)));
  return model.includeCache.get(index);
}

/** Recipes listed for a row: the top recipe, then its variants. */
export function rowRecipes(row) {
  return [row[7][0], ...row[8].map((v) => v[0])];
}

/** Names included by any recipe listed for a row. */
function includesOf(model, row) {
  if (!model.rowIncludes.has(row)) {
    const all = new Map();
    for (const i of rowRecipes(row)) for (const [n, k] of recipeIncludes(model, i)) if (!all.has(n)) all.set(n, k);
    model.rowIncludes.set(row, all);
  }
  return model.rowIncludes.get(row);
}

/** The values a row offers for one facet. */
function values(model, row, facet) {
  const t = model.types[row[0]];
  switch (facet) {
    case 'cls': return t.class ? [t.class] : [];
    case 'element': return t.element ? [t.element] : [];
    case 'armor': return [t.armor ?? NONE];
    case 'weapon': return [t.weapon ?? NONE];
    case 'sup': return t.super ? [t.super] : [];
    case 'aspect1':
    case 'aspect2': return t.aspects;
    case 'has': return [...includesOf(model, row).keys()];
    default: return [];
  }
}

/** Whether a row passes every filter except `skip`. */
export function matches(model, row, state, skip = null) {
  const t = model.types[row[0]];
  if (skip !== 'cls' && state.cls && t.class !== state.cls) return false;
  if (skip !== 'element' && state.element && t.element !== state.element) return false;
  if (skip !== 'armor' && state.armor && (t.armor ?? NONE) !== state.armor) return false;
  if (skip !== 'weapon' && state.weapon && (t.weapon ?? NONE) !== state.weapon) return false;
  if (skip !== 'sup' && state.sup && t.super !== state.sup) return false;
  if (skip !== 'aspect1' && state.aspect1 && !t.aspects.includes(state.aspect1)) return false;
  if (skip !== 'aspect2' && state.aspect2 && !t.aspects.includes(state.aspect2)) return false;
  if (skip !== 'has' && state.has && !includesOf(model, row).has(state.has)) return false;
  return true;
}

export function scopeOf(model, state) {
  return model.scopes.get(state.scope) ?? model.scopes.get('all');
}

/** Rows of the selected scope that pass the filters, best first. */
export function results(model, state) {
  const scope = scopeOf(model, state);
  const key = (SORTS[state.sort] ?? SORTS.effect).key;
  const rows = scope.rows.filter((r) => r[4] >= state.min && matches(model, r, state));
  rows.sort((a, b) => key(b) - key(a) || b[1] - a[1] || b[4] - a[4] || a[0] - b[0]);
  return { scope, rows };
}

/**
 * Options for one facet with the number of listings each would leave, given
 * every other filter: Map(value → count).
 */
export function facetOptions(model, state, facet) {
  const counts = new Map();
  for (const r of scopeOf(model, state).rows) {
    if (r[4] < state.min || !matches(model, r, state, facet)) continue;
    for (const v of values(model, r, facet)) counts.set(v, (counts.get(v) ?? 0) + 1);
  }
  return counts;
}

/** The kind of every "includes" option. */
export function includeKinds(model, state) {
  const kinds = new Map();
  for (const r of scopeOf(model, state).rows) for (const [n, k] of includesOf(model, r)) if (!kinds.has(n)) kinds.set(n, k);
  return kinds;
}

/** Multiset difference of two lists: [added, removed] (by value). */
function listDiff(from, to) {
  const count = new Map();
  for (const x of from) count.set(x, (count.get(x) ?? 0) + 1);
  const added = [];
  for (const x of to) {
    if ((count.get(x) ?? 0) > 0) count.set(x, count.get(x) - 1);
    else added.push(x);
  }
  const removed = [];
  for (const [x, c] of count) for (let i = 0; i < c; i++) removed.push(x);
  return [added, removed];
}

/**
 * How recipe `b` differs from recipe `a`, as readable lines:
 * [{ label, text }]. Empty if the recipes are the same in every listed part.
 */
export function recipeDiff(model, a, b) {
  const n = (i) => name(model, i);
  const lines = [];
  const change = (label, added, removed) => {
    if (added.length === 0 && removed.length === 0) return;
    const parts = [];
    if (added.length) parts.push(added.map((i) => `+${n(i)}`).join(', '));
    if (removed.length) parts.push(removed.map((i) => `−${n(i)}`).join(', '));
    lines.push({ label, text: parts.join('; ') });
  };
  const swap = (label, x, y) => { if (x !== y) lines.push({ label, text: `${n(x) ?? 'none'} → ${n(y) ?? 'none'}` }); };

  for (const [field, label] of [['subclass', 'Subclass'], ['super', 'Super'], ['classAbility', 'Class ability'],
    ['movement', 'Movement'], ['melee', 'Melee'], ['grenade', 'Grenade']]) swap(label, a.kit[field], b.kit[field]);
  change('Aspects', ...listDiff(a.kit.aspects, b.kit.aspects));
  change('Fragments', ...listDiff(a.kit.fragments, b.kit.fragments));
  change('Transcendence', ...listDiff(a.kit.transcendence, b.kit.transcendence));

  for (let slot = 0; slot < WEAPON_SLOTS.length; slot++) {
    const x = a.weapons.find((w) => w.slot === slot);
    const y = b.weapons.find((w) => w.slot === slot);
    const label = WEAPON_SLOTS[slot];
    if (!x || !y || x.item !== y.item) { swap(label, x?.item ?? null, y?.item ?? null); continue; }
    const [added, removed] = listDiff([x.frame, ...x.perks, x.origin, x.mod].filter((v) => v !== null),
      [y.frame, ...y.perks, y.origin, y.mod].filter((v) => v !== null));
    change(`${label} (${n(x.item)})`, added, removed);
  }
  for (let slot = 0; slot < ARMOR_SLOTS.length; slot++) {
    const x = a.armor.find((p) => p.slot === slot);
    const y = b.armor.find((p) => p.slot === slot);
    if (x?.item !== y?.item) swap(ARMOR_SLOTS[slot], x?.item ?? null, y?.item ?? null);
    if (x && y) {
      if (x.archetype !== y.archetype) swap(`${ARMOR_SLOTS[slot]} archetype`, x.archetype, y.archetype);
      if (x.tuning !== y.tuning) swap(`${ARMOR_SLOTS[slot]} tuning`, x.tuning, y.tuning);
      change(`${ARMOR_SLOTS[slot]} stat mods`, ...listDiff(x.statMods, y.statMods));
      change(`${ARMOR_SLOTS[slot]} perks`, ...listDiff(x.perks, y.perks));
    }
  }
  change('Armor mods', ...listDiff(a.armor.flatMap((p) => p.mods), b.armor.flatMap((p) => p.mods)));
  const setLabels = (r) => r.sets.filter((e) => e.pieces > 0).map((e) => `${n(e.set) ?? 'Armor set'} ×${e.pieces}`);
  const [setsAdded, setsRemoved] = listDiff(setLabels(a), setLabels(b));
  if (setsAdded.length || setsRemoved.length) {
    lines.push({ label: 'Armor sets', text: [...setsAdded.map((x) => `+${x}`), ...setsRemoved.map((x) => `−${x}`)].join(', ') });
  }
  change('Set bonuses', ...listDiff(a.setBonuses, b.setBonuses));
  change('Artifact perks', ...listDiff(a.artifact, b.artifact));
  if (a.stats && b.stats && a.stats.some((s, i) => s !== b.stats[i])) {
    lines.push({
      label: 'Stats',
      text: STATS.map((s, i) => (a.stats[i] === b.stats[i] ? null : `${s} ${a.stats[i]} → ${b.stats[i]}`)).filter(Boolean).join(', '),
    });
  }
  return lines;
}

export function parseHash(hash) {
  const p = new URLSearchParams(hash.replace(/^#/, ''));
  const state = { ...DEFAULTS };
  for (const [k, key] of Object.entries(HASH_KEYS)) {
    const v = p.get(key);
    if (v === null) continue;
    state[k] = k === 'min' ? (Number.parseInt(v, 10) > 0 ? Number.parseInt(v, 10) : DEFAULTS.min) : v;
  }
  if (!(state.sort in SORTS)) state.sort = DEFAULTS.sort;
  return state;
}

export function formatHash(state) {
  const p = new URLSearchParams();
  for (const [k, key] of Object.entries(HASH_KEYS)) {
    if (state[k] !== '' && state[k] !== DEFAULTS[k]) p.set(key, state[k]);
  }
  const s = p.toString();
  return s ? `#${s}` : '';
}

/** "Master Raid · Vault of Glass: Master", without repeating a prefix. */
export function scopeLabel(scope) {
  if (!scope.parent || scope.name.startsWith(scope.parent)) return scope.name;
  return `${scope.parent} · ${scope.name}`;
}

// ---------------------------------------------------------------------------
// DOM
// ---------------------------------------------------------------------------

/** Create an element: h('div', { class: 'x' }, child, 'text', ...). */
export function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === null || v === undefined || v === false) continue;
    if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (v === true) el.setAttribute(k, '');
    else el.setAttribute(k, v);
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

const fmt = new Intl.NumberFormat('en-US');
export const n = (x) => fmt.format(x);

export function img(model, path, cls) {
  return path ? h('img', { class: cls, src: model.data.icon_base + path, alt: '', loading: 'lazy', width: 40, height: 40 }) : null;
}

/** Icon for a name shown in a build type. */
function typeIcon(model, nameText, cls = 'icon') {
  return img(model, nameText && model.data.icons[nameText], cls);
}

/** "Whisper of Chains · Stasis Fragment": a dictionary entry's tooltip. */
export function tipOf(model, i) {
  const [label, , kind] = model.data.dict[i];
  return kind && kind !== label ? `${label} · ${kind}` : label;
}

/**
 * An icon tile with a tooltip (on hover, focus or tap) and an accessible
 * name; a blank tile when `i` is null, so columns stay aligned.
 */
export function tile(model, i, { mark = '', cls = '' } = {}) {
  if (i === null || i === undefined || !model.data.dict[i]) return h('span', { class: `tile blank ${cls}`, 'aria-hidden': 'true' });
  const [label, icon] = model.data.dict[i];
  const tip = tipOf(model, i);
  return h('span', {
    class: `tile ${cls}${mark && label === mark ? ' match' : ''}`, tabindex: '0', role: 'img', 'aria-label': tip, 'data-tip': tip,
  }, icon ? h('img', { src: model.data.icon_base + icon, alt: '', loading: 'lazy', decoding: 'async' }) : h('span', { class: 'tile-text' }, label.slice(0, 2)));
}

/** A tile for a glyph that is not in the dictionary (an element, a stat). */
function glyph(model, path, tip, cls = '') {
  return h('span', { class: `tile ${cls}`, tabindex: '0', role: 'img', 'aria-label': tip, 'data-tip': tip },
    path ? h('img', { src: model.data.icon_base + path, alt: '', loading: 'lazy', decoding: 'async' }) : h('span', { class: 'tile-text' }, tip.slice(0, 2)));
}

export function elementBadge(element) {
  return element ? h('span', { class: `badge el-${element.toLowerCase()}` }, element) : null;
}

export const plural = (x, word) => `${n(x)} ${word}${x === 1 ? '' : 's'}`;

/** An exact build's record: "Used in 2 clears by 1 player · last … · +9% for its players". */
export function pickLine(p) {
  const [, , runs, players, last] = p;
  return `Used in ${plural(Math.max(1, Math.round(runs)), 'clear')} by ${plural(players, 'player')} · last ${last}`;
}

/** The top recipe's record, with how often its loadout was used. */
export function topLine(row) {
  const loadout = Math.max(1, Math.round(row[10]));
  const builds = row[11];
  return `${pickLine(row[7])}. Its loadout (same kit and weapons) was used in ${plural(loadout, 'clear')}`
    + (builds > 1 ? ` across ${plural(builds, 'exact build')}.` : '.');
}

const pad = (list, count) => Array.from({ length: count }, (_, k) => list[k] ?? null);
const blank = () => h('span', { class: 'tile blank', 'aria-hidden': 'true' });

/** One gear row: the item's icon, its name and type, then plug tiles in fixed columns. */
function gearRow(model, item, cells, cols, mark) {
  const [label, , kind] = model.data.dict[item] ?? ['Unknown'];
  return h('div', { class: 'gear' },
    tile(model, item, { mark, cls: 'item' }),
    h('div', { class: 'gear-body' },
      h('div', { class: 'gear-name' },
        h('span', { class: `gear-title${mark && label === mark ? ' match' : ''}` }, label),
        kind ? h('span', { class: 'gear-type' }, ` · ${kind}`) : null),
      h('div', { class: 'tiles', style: `--cols:${cols}` }, cells)));
}

/** Weapons: element · frame · perks · origin trait · mod, aligned across weapons. */
function weaponsBlock(model, r, mark) {
  if (r.weapons.length === 0) return h('p', { class: 'muted' }, 'None captured');
  const perks = Math.max(0, ...r.weapons.map((w) => w.perks.length));
  return r.weapons.map((w) => {
    const damage = model.data.dict[w.item]?.[3];
    const element = damage ? glyph(model, model.data.elements?.[damage], damage, 'element') : blank();
    return gearRow(model, w.item, [
      element, tile(model, w.frame, { mark }),
      ...pad(w.perks, perks).map((x) => tile(model, x, { mark })),
      tile(model, w.origin, { mark }), tile(model, w.mod, { mark }),
    ], 2 + perks + 2, mark);
  });
}

/** Armor: archetype · stat mod · tuning · mods · exotic perks, aligned across pieces. */
function armorBlock(model, r, mark) {
  if (r.armor.length === 0) return h('p', { class: 'muted' }, 'None captured');
  const most = (f) => Math.max(0, ...r.armor.map(f));
  const stat = most((p) => p.statMods.length);
  const mods = most((p) => p.mods.length);
  const perks = most((p) => p.perks.length);
  return r.armor.map((p) => gearRow(model, p.item, [
    tile(model, p.archetype), ...pad(p.statMods, stat).map((x) => tile(model, x)), tile(model, p.tuning),
    ...pad(p.mods, mods).map((x) => tile(model, x, { mark })),
    ...pad(p.perks, perks).map((x) => tile(model, x, { mark, cls: 'exotic-perk' })),
  ], 1 + stat + 1 + mods + perks, mark));
}

function kitGroup(label, cells) {
  return h('div', { class: 'kit-group' }, h('span', { class: 'kit-label' }, label), h('div', { class: 'kit-tiles' }, cells));
}

/** Subclass: its name; Super beside Abilities; Aspects beside Fragments; Transcendence. */
function subclassBlock(model, r, mark) {
  const k = r.kit;
  const t = (i) => tile(model, i, { mark, cls: 'kit' });
  const present = (list) => list.filter((x) => x !== null && x !== undefined);
  return [
    k.subclass !== null ? h('p', { class: 'subclass-name' }, name(model, k.subclass)) : null,
    h('div', { class: 'kit-pair' },
      kitGroup('Super', present([k.super]).map(t)),
      kitGroup('Abilities', present([k.classAbility, k.movement, k.melee, k.grenade]).map(t))),
    h('div', { class: 'kit-pair' }, kitGroup('Aspects', k.aspects.map(t)), kitGroup('Fragments', k.fragments.map(t))),
    k.transcendence.length ? kitGroup('Transcendence', k.transcendence.map(t)) : null,
  ];
}

/** The in-game order of the stats (recipes store Weapons, Health, Class, Grenade, Super, Melee). */
const GAME_ORDER = [1, 5, 3, 4, 2, 0];

function statsBlock(model, r) {
  if (!r.stats) return h('p', { class: 'muted' }, 'Not captured');
  return h('ul', { class: 'stat-list' }, GAME_ORDER.map((i) => {
    const [label, icon] = model.data.stats?.[i] ?? [STATS[i], null];
    return h('li', {}, glyph(model, icon, label, 'stat-icon'),
      h('span', { class: 'stat-value' }, r.stats[i]), h('span', { class: 'stat-name' }, label));
  }));
}

/** Set bonuses: each set with the bonuses it activates (sets without one are left out). */
function setsBlock(model, r, mark) {
  const sets = r.sets.filter((e) => e.bonuses.length);
  if (!sets.length) return null;
  return h('section', { class: 'lo-sets' }, h('h4', {}, 'Set bonuses'),
    sets.map((e) => h('div', { class: 'set' },
      h('p', { class: 'set-name' }, name(model, e.set) ?? 'Armor set', h('span', { class: 'muted' }, ` · ${plural(e.pieces, 'piece')}`)),
      e.bonuses.map((b) => h('div', { class: 'set-bonus' }, tile(model, b, { mark, cls: 'small' }),
        h('span', { class: mark && name(model, b) === mark ? 'match' : '' }, name(model, b)))))));
}

function artifactBlock(model, r, mark) {
  if (r.artifactItem === null && !r.artifact.length) return null;
  return h('section', { class: 'lo-artifact' }, h('h4', {}, 'Artifact'),
    r.artifactItem !== null ? h('div', { class: 'gear' }, tile(model, r.artifactItem, { cls: 'item' }),
      h('div', { class: 'gear-body' },
        h('div', { class: 'gear-name' }, h('span', { class: 'gear-title' }, name(model, r.artifactItem))),
        h('p', { class: 'muted small' }, r.artifactUnlocked
          ? `Unlocked artifact perks (${r.artifact.length}); which were applied wasn’t captured`
          : `Applied artifact perks (${r.artifact.length})`))) : null,
    h('div', { class: 'kit-tiles' }, r.artifact.map((i) => tile(model, i, { mark }))));
}

/**
 * A recipe drawn the way Searchlight drew a build, after the in-game
 * loadout screen: weapons and subclass | stats and set bonuses | armor and
 * artifact (stacked on narrow screens).
 */
export function loadoutPanel(model, r, mark = '') {
  return h('div', { class: 'loadout' },
    h('div', { class: 'lo-col' },
      h('section', { class: 'lo-weapons' }, h('h4', {}, 'Weapons'), weaponsBlock(model, r, mark)),
      h('section', { class: 'lo-subclass' }, h('h4', {}, 'Subclass'), subclassBlock(model, r, mark))),
    h('div', { class: 'lo-col' },
      h('section', { class: 'lo-stats' }, h('h4', {}, 'Stats'), statsBlock(model, r)),
      setsBlock(model, r, mark)),
    h('div', { class: 'lo-col' },
      h('section', { class: 'lo-armor' }, h('h4', {}, 'Armor'), armorBlock(model, r, mark)),
      artifactBlock(model, r, mark)));
}

/**
 * A build's short id: the last 8 base-36 digits of its id, as the catalog
 * writes them (`short_id` in crates/db/src/catalog.rs).
 */
export function shortId(id) {
  return (BigInt.asUintN(64, BigInt(id)) % (36n ** 8n)).toString(36).padStart(8, '0');
}

/** A link to one build in the finder: its build type's names plus its short id. */
export function buildLink(t, id) {
  return `${findHash(t)}&build=${shortId(id)}`;
}

/** "Copy link": copies the page URL for `hash`, and says so. */
export function shareButton(hash) {
  const status = h('span', { class: 'share-status', role: 'status' });
  const url = () => `${location.origin}${location.pathname}${hash}`;
  const button = h('button', {
    type: 'button',
    class: 'button secondary share',
    onclick: async () => {
      try {
        await navigator.clipboard.writeText(url());
        status.textContent = 'Link copied';
      } catch {
        // No clipboard (insecure page, permissions): show the link to copy by hand.
        status.replaceChildren(h('input', { class: 'share-url', value: url(), readonly: true, 'aria-label': 'Link to this build' }));
        status.querySelector('input').select();
      }
    },
  }, 'Copy link to this build');
  return h('div', { class: 'share-row' }, button, status);
}

/**
 * The ranking numbers, kept out of the way: they compare players with their
 * own usual level, among already-elite players, so a strong build can read
 * slightly negative.
 */
function rankingDetails(effect, raw, skill) {
  return lazyDetails('Ranking details', () => h('div', { class: 'ranking-details' },
    h('p', {}, `Build effect ${pct(effect)} · raw result ${pct(raw)} · players’ skill ${pct(skill)}`),
    h('p', { class: 'muted' },
      'Build effect is how players did with this build type compared with their own other runs. Searchlight’s '
      + 'players were already among the best, so most builds sit near zero and a small negative number can still '
      + 'be a very good build; the list is ordered by it. Raw result is how its runs went against a typical clear; '
      + 'players’ skill is how good its players are.')));
}

/** A details element whose body is built on first open. */
export function lazyDetails(summary, build, cls = 'more') {
  const d = h('details', { class: cls }, h('summary', {}, summary));
  d.addEventListener('toggle', () => { if (d.open && d.childElementCount === 1) d.append(build()); });
  return d;
}

function variantItem(model, top, v, mark, t) {
  const r = model.recipe(v[0]);
  const diff = recipeDiff(model, top, r);
  const matched = mark && recipeIncludes(model, v[0]).has(mark);
  return h('li', { class: `variant${matched ? ' matched' : ''}` },
    h('p', { class: 'recipe-meta' }, pickLine(v), matched ? h('span', { class: 'match-badge' }, ` includes ${mark}`) : null),
    diff.length
      ? h('ul', { class: 'diff' }, diff.map((d) => h('li', {}, h('span', { class: 'diff-label' }, `${d.label}: `), d.text)))
      : h('p', { class: 'muted' }, 'Same as the top build in every listed part.'),
    lazyDetails('Full build', () => h('div', {}, loadoutPanel(model, r, mark), shareButton(buildLink(t, r.id)))));
}

function elsewhere(model, typeId, current, go) {
  const others = model.appearances[typeId].filter((a) => a.scope.id !== current.id && a.scope.kind !== 'all');
  if (others.length === 0) return null;
  others.sort((a, b) => b.effect - a.effect);
  return lazyDetails(`Ranked in ${others.length} other activit${others.length === 1 ? 'y' : 'ies'}`, () =>
    h('table', { class: 'elsewhere' },
      h('thead', {}, h('tr', {}, h('th', { scope: 'col' }, 'Activity'), h('th', { scope: 'col' }, 'Effect'), h('th', { scope: 'col' }, 'Players'))),
      h('tbody', {}, others.map((a) => h('tr', {},
        h('td', {}, h('button', { type: 'button', class: 'link', onclick: () => go(a.scope.id) }, scopeLabel(a.scope))),
        h('td', { class: a.effect >= 0 ? 'pos-text' : 'neg-text' }, pct(a.effect)),
        h('td', {}, n(a.players)))))));
}

function card(model, row, rank, scope, go, mark) {
  const t = model.types[row[0]];
  // [10], [11]: clears and exact builds of the top recipe's loadout.
  const [, effect, raw, skill, players, runs, last, top, variants, more] = row;
  const recipe = model.recipe(top[0]);
  const topMatched = mark && recipeIncludes(model, top[0]).has(mark);
  const total = variants.length + more;
  return h('li', { class: 'card' },
    h('div', { class: 'card-main' },
      h('span', { class: 'rank' }, `${rank}`),
      h('div', { class: 'identity' },
        h('h3', { class: 'exotics' },
          h('span', { class: 'exotic' }, typeIcon(model, t.armor), h('span', { class: t.armor ? '' : 'muted' }, t.armor ?? 'No exotic armor')),
          h('span', { class: 'exotic' }, typeIcon(model, t.weapon), h('span', { class: t.weapon ? '' : 'muted' }, t.weapon ?? 'No exotic weapon'))),
        h('p', { class: 'subtitle' }, elementBadge(t.element), ` ${t.class ?? ''}`, t.super ? ` · ${t.super}` : ''),
        h('ul', { class: 'aspects' }, t.aspects.map((a) => h('li', {}, typeIcon(model, a, 'chip-icon'), a)))),
      h('div', { class: 'metrics' },
        h('p', { class: 'usage' }, h('strong', {}, n(players)), ` player${players === 1 ? '' : 's'} · `,
          h('strong', {}, n(Math.round(runs))), ` run${Math.round(runs) === 1 ? '' : 's'}`),
        h('p', { class: 'secondary' }, `last used ${last}`))),
    h('section', { class: `recipe${topMatched ? ' matched' : ''}`, 'aria-label': 'Top build' },
      h('div', { class: 'recipe-head' },
        h('h4', { class: 'recipe-title' }, 'Top build'),
        h('p', { class: 'recipe-meta' }, topLine(row), topMatched ? h('span', { class: 'match-badge' }, ` includes ${mark}`) : null)),
      loadoutPanel(model, recipe, mark),
      shareButton(buildLink(t, recipe.id))),
    total ? lazyDetails(
      `${total} near-identical variant${total === 1 ? '' : 's'}${more ? ` (showing the best ${variants.length})` : ''}`,
      () => h('ol', { class: 'variants' }, variants.map((v) => variantItem(model, recipe, v, mark, t))),
      'more variants-details') : null,
    rankingDetails(effect, raw, skill),
    elsewhere(model, row[0], scope, go),
    h('a', { class: 'browse-link', href: findHash(t) }, 'Every collected build of this type →'));
}

export function fillSelect(select, options, value, anyLabel) {
  select.replaceChildren(h('option', { value: '' }, anyLabel), ...options);
  select.value = value;
  if (select.value !== value) {
    // The selection has no listings left; keep it visible so it can be undone.
    select.append(h('option', { value }, `${value === NONE ? 'None' : value} (0)`));
    select.value = value;
  }
}

export function optionList(counts, label = (v) => v) {
  return [...counts.entries()]
    .sort((a, b) => (a[0] === NONE ? -1 : b[0] === NONE ? 1 : a[0].localeCompare(b[0])))
    .map(([v, c]) => h('option', { value: v }, `${label(v)} (${c})`));
}

/** The finder's URL for one build type of the book (names as filters). */
export function findHash(t) {
  const p = new URLSearchParams({ view: 'find' });
  const set = (key, v) => { if (v) p.set(key, v); };
  set('class', t.class);
  set('element', t.element);
  set('armor', t.armor ?? NONE);
  set('weapon', t.weapon ?? NONE);
  set('super', t.super);
  set('aspect', t.aspects[0]);
  set('aspect2', t.aspects[1]);
  return `#${p}`;
}

/** The ranked book's URL for a build type (names as filters). */
export function bookHash(t) {
  const p = new URLSearchParams();
  const set = (key, v) => { if (v) p.set(key, v); };
  set('class', t.class);
  set('element', t.element);
  set('armor', t.armorName ?? NONE);
  set('weapon', t.weaponName ?? NONE);
  set('super', t.superName);
  set('aspect', t.aspectNames[0]);
  set('aspect2', t.aspectNames[1]);
  return `#${p}`;
}

/** Run the ranked book in `root`; returns `{ refresh }` for URL changes. */
export function startBook(root, data, load) {
  const model = prepare(data);
  let generation = 0;
  let state = parseHash(location.hash);
  let shown = PAGE;
  const $ = (id) => root.querySelector(`#${id}`);

  // Activity picker: all, then each target group and its activities.
  const scopeSelect = $('scope');
  const byParent = new Map();
  for (const s of data.scopes) if (s.kind === 'activity') {
    if (!byParent.has(s.parent)) byParent.set(s.parent, []);
    byParent.get(s.parent).push(s);
  }
  scopeSelect.replaceChildren(h('option', { value: 'all' }, 'All activities'),
    ...data.scopes.filter((s) => s.kind === 'group').map((s) => {
      const children = byParent.get(s.name) ?? [];
      return h('optgroup', { label: s.name },
        h('option', { value: s.id }, children.length ? `All ${s.name}` : s.name),
        children.map((c) => h('option', { value: c.id }, c.name)));
    }));

  const update = (patch, { resetPage = true } = {}) => {
    state = { ...state, ...patch };
    if (resetPage) shown = PAGE;
    const hash = formatHash(state);
    if (hash !== location.hash && !(hash === '' && location.hash === '')) {
      history.replaceState(null, '', hash || location.pathname + location.search);
    }
    render();
  };
  const go = (scopeId) => {
    update({ scope: scopeId });
    root.querySelector('#results-heading').scrollIntoView({ behavior: 'smooth', block: 'start' });
  };

  const facets = [
    ['element', 'Any element', (v) => v],
    ['armor', 'Any exotic armor', (v) => (v === NONE ? 'No exotic armor' : v)],
    ['weapon', 'Any exotic weapon', (v) => (v === NONE ? 'No exotic weapon' : v)],
    ['sup', 'Any super', (v) => v],
    ['aspect1', 'Any aspect', (v) => v],
    ['aspect2', 'Any aspect', (v) => v],
  ];

  function renderFilters() {
    scopeSelect.value = scopeOf(model, state).id;
    const clsCounts = facetOptions(model, state, 'cls');
    for (const input of root.querySelectorAll('input[name="class"]')) {
      input.checked = input.value === state.cls;
      const count = input.value === '' ? null : clsCounts.get(input.value) ?? 0;
      input.nextElementSibling.querySelector('.count').textContent = count === null ? '' : ` ${count}`;
    }
    for (const [facet, any, label] of facets) {
      fillSelect($(facet), optionList(facetOptions(model, state, facet), label), state[facet], any);
    }
    // "Recipe includes": grouped by kind.
    const kinds = includeKinds(model, state);
    const grouped = new Map();
    for (const [nameText, count] of facetOptions(model, state, 'has')) {
      const kind = kinds.get(nameText) ?? 'ability';
      if (!grouped.has(kind)) grouped.set(kind, new Map());
      grouped.get(kind).set(nameText, count);
    }
    const order = Object.keys(INCLUDE_KINDS);
    fillSelect($('has'), [...grouped.entries()]
      .sort((a, b) => order.indexOf(a[0]) - order.indexOf(b[0]))
      .map(([kind, counts]) => h('optgroup', { label: INCLUDE_KINDS[kind] ?? kind }, optionList(counts))),
    state.has, 'Anything');
    $('min').value = String(state.min);
    $('sort').value = state.sort;
  }

  function renderScopeNote(scope) {
    const note = $('scope-note');
    note.replaceChildren(
      `Ranked from ${n(scope.clears)} qualified clears by ${n(scope.players)} players, `
        + `${data.first_run} to ${data.last_run}; `,
      h('strong', {}, n(scope.build_runs)), ' of them had a captured build.');
    if (scope.build_runs < THIN_SCOPE) {
      const parent = scope.parent && data.scopes.find((s) => s.kind === 'group' && s.name === scope.parent);
      note.append(h('span', { class: 'warn' }, ' Few captured builds here, so these rankings are tentative',
        parent ? [' — ', h('button', { type: 'button', class: 'link', onclick: () => go(parent.id) }, `see all ${parent.name}`)] : '', '.'));
    }
  }

  async function render() {
    const mine = ++generation;
    const target = scopeOf(model, state);
    const missing = missingParts(target, state.min);
    if (missing.length) {
      $('summary').textContent = 'Loading…';
      const parts = await Promise.all(missing.map((f) => load(`data/book/${f}`).then((p) => [f, p])));
      for (const [f, p] of parts) addPart(model, target, f, p);
      if (mine !== generation) return; // a newer render took over
    }
    renderFilters();
    const { scope, rows } = results(model, state);
    renderScopeNote(scope);
    $('summary').textContent = rows.length === 0
      ? 'No builds match.'
      : `${n(rows.length)} build${rows.length === 1 ? '' : 's'} match · showing ${n(Math.min(shown, rows.length))}`;
    $('results').replaceChildren(...rows.slice(0, shown).map((r, i) => card(model, r, i + 1, scope, go, state.has)));
    $('empty').hidden = rows.length !== 0;
    $('more').hidden = rows.length <= shown;
  }

  scopeSelect.addEventListener('change', () => update({ scope: scopeSelect.value }));
  for (const input of root.querySelectorAll('input[name="class"]')) {
    input.addEventListener('change', () => update({ cls: input.value }));
  }
  for (const [facet] of facets) $(facet).addEventListener('change', (e) => update({ [facet]: e.target.value }));
  $('has').addEventListener('change', (e) => update({ has: e.target.value }));
  $('min').addEventListener('change', (e) => update({ min: Number.parseInt(e.target.value, 10) }));
  $('sort').addEventListener('change', (e) => update({ sort: e.target.value }));
  $('reset').addEventListener('click', () => update({ ...DEFAULTS, scope: state.scope }));
  $('more').addEventListener('click', () => { shown += PAGE; update({}, { resetPage: false }); });

  render();
  return { refresh: () => { state = parseHash(location.hash); shown = PAGE; render(); } };
}

const isFind = () => new URLSearchParams(location.hash.replace(/^#/, '')).get('view') === 'find';

async function fetchJson(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return response.json();
}

/**
 * Start the page: the ranked book or "Find any build", as the URL says.
 * Each view's data is fetched the first time the view is shown.
 */
export function boot(root) {
  const $ = (id) => root.querySelector(`#${id}`);
  let book = null;
  let finder = null;
  // A finder result links to its type's ranked listing.
  const bookLink = (t) => h('a', { class: 'browse-link', href: bookHash(t) }, 'Look for this build type in the ranked builds →');

  async function show() {
    const find = isFind();
    $('book-view').hidden = find;
    $('find-view').hidden = !find;
    $('tab-book').setAttribute('aria-selected', String(!find));
    $('tab-find').setAttribute('aria-selected', String(find));
    const note = $(find ? 'find-note' : 'scope-note');
    try {
      if (find) {
        if (finder) (await finder).refresh();
        else finder = import('./finder.js').then((m) => m.startFinder(root, bookLink));
        await finder;
      } else {
        if (book) (await book).refresh();
        else book = fetchJson('data/book/index.json').then((data) => startBook(root, data, fetchJson));
        await book;
      }
    } catch (error) {
      if (find) finder = null; else book = null;
      note.classList.add('error');
      note.textContent = location.protocol === 'file:'
        ? 'Open this page through a web server (for example: python3 -m http.server -d site); browsers block loading data from file:// pages.'
        : `Could not load the data: ${error.message}`;
    }
    root.classList.remove('loading');
  }

  // The tabs keep the shared filters (class, exotics, aspects, …).
  $('tab-find').addEventListener('click', () => {
    const p = new URLSearchParams(location.hash.replace(/^#/, ''));
    p.set('view', 'find');
    location.hash = p.toString();
  });
  $('tab-book').addEventListener('click', () => {
    const p = new URLSearchParams(location.hash.replace(/^#/, ''));
    p.delete('view');
    location.hash = p.toString();
  });
  window.addEventListener('hashchange', show);
  show();
}
