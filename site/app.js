// Focalpoint build rankings: a book of exact recipes, read from
// data/focalpoint.json, which `focalpoint site` exports (see docs/site.md).
//
// Build types are ranked by build effect; each listing publishes the
// best-ranked exact build of the type's best loadout (the "top recipe") and
// that loadout's other exact builds (near-identical variants), exactly as
// players equipped them. The pure functions (decoding, filtering,
// facet counts, ranking, recipe differences, URL state) are exported for
// tests/site/test.html; start() wires them to the page.

export const FORMAT = 2;
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
  effect: { label: 'Build effect', key: (r) => r[1] },
  recipe: { label: 'Top recipe', key: (r) => r[7][1] },
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
  const [id, stats, kit, armor, weapons, sets, setBonuses, artifact] = r;
  const [subclass, sup, classAbility, movement, melee, grenade, aspects, fragments, transcendence] = kit;
  return {
    id, stats,
    kit: { subclass, super: sup, classAbility, movement, melee, grenade, aspects, fragments, transcendence },
    armor: armor.map(([slot, item, exotic, archetype, tuning, statMods, mods, perks]) =>
      ({ slot, item, exotic: exotic === 1, archetype, tuning, statMods, mods, perks })),
    weapons: weapons.map(([slot, item, exotic, frame, perks, origin, mod]) =>
      ({ slot, item, exotic: exotic === 1, frame, perks, origin, mod })),
    sets, setBonuses, artifact,
  };
}

/** Index the exported data. Recipes are decoded on first use. */
export function prepare(data) {
  if (data.format !== FORMAT) {
    throw new Error(`data format ${data.format}, expected ${FORMAT}: re-export with this version of focalpoint`);
  }
  const scopes = new Map(data.scopes.map((s) => [s.id, s]));
  const appearances = data.types.map(() => []);
  for (const s of data.scopes) for (const r of s.rows) appearances[r[0]].push({ scope: s, row: r });
  const decoded = new Map();
  const recipe = (i) => {
    if (!decoded.has(i)) decoded.set(i, decodeRecipe(data.recipes[i]));
    return decoded.get(i);
  };
  return { data, types: data.types, scopes, appearances, recipe, includeCache: new Map(), rowIncludes: new WeakMap() };
}

const name = (model, i) => (i === null || i === undefined ? null : model.data.dict[i][0]);

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

/** A dictionary entry as an icon and its name; `mark` highlights a filter match. */
export function thing(model, i, { cls = 'thing', count = 1, mark = '' } = {}) {
  if (i === null || i === undefined) return null;
  const [label, icon] = model.data.dict[i];
  return h('span', { class: `${cls}${mark && label === mark ? ' match' : ''}` },
    img(model, icon, 'chip-icon'), h('span', {}, label), count > 1 ? h('span', { class: 'times' }, `×${count}`) : null);
}

/** Things with repeats collapsed to ×N, in first-seen order. */
export function things(model, list, opts) {
  const counts = new Map();
  for (const i of list) counts.set(i, (counts.get(i) ?? 0) + 1);
  return [...counts].map(([i, c]) => thing(model, i, { ...opts, count: c }));
}

export function elementBadge(element) {
  return element ? h('span', { class: `badge el-${element.toLowerCase()}` }, element) : null;
}

/** A diverging bar for a log-scale effect (±50% fills one side). */
function effectBar(x) {
  const v = Math.max(-1, Math.min(1, (Math.exp(x) - 1) / 0.5));
  return h('span', { class: 'bar', 'aria-hidden': 'true' },
    h('span', { class: `fill ${v >= 0 ? 'pos' : 'neg'}`, style: `width:${(Math.abs(v) * 50).toFixed(1)}%` }));
}

export const plural = (x, word) => `${n(x)} ${word}${x === 1 ? '' : 's'}`;

/** An exact build's record: "Used in 2 clears by 1 player · last … · +9% for its players". */
export function pickLine(p) {
  const [, effect, runs, players, last] = p;
  return `Used in ${plural(Math.max(1, Math.round(runs)), 'clear')} by ${plural(players, 'player')} · `
    + `last ${last} · ${pct(effect)} for its players`;
}

/** The top recipe's record, with how often its loadout was used. */
export function topLine(row) {
  const loadout = Math.max(1, Math.round(row[10]));
  const builds = row[11];
  return `${pickLine(row[7])}. Its loadout (same kit and weapons) was used in ${plural(loadout, 'clear')}`
    + (builds > 1 ? ` across ${plural(builds, 'exact build')}.` : '.');
}

export function statLine(stats) {
  if (!stats) return null;
  return h('ul', { class: 'stat-line', 'aria-label': 'Stats' },
    STATS.map((s, i) => h('li', {}, h('span', { class: 'stat-name' }, s), h('span', { class: 'stat-value' }, stats[i]))));
}

export function weaponRow(model, w, mark) {
  return h('li', { class: 'weapon' },
    h('span', { class: 'slot-label' }, WEAPON_SLOTS[w.slot]),
    h('div', { class: 'weapon-body' },
      thing(model, w.item, { cls: 'thing strong', mark }),
      h('div', { class: 'perks' },
        thing(model, w.frame, { cls: 'perk', mark }),
        w.perks.map((p) => thing(model, p, { cls: 'perk', mark })),
        thing(model, w.origin, { cls: 'perk', mark }),
        thing(model, w.mod, { cls: 'perk', mark }))));
}

/** The always-visible part of a recipe: abilities, kit, weapons, stats. */
export function recipeSummary(model, r, mark) {
  const k = r.kit;
  return h('div', { class: 'recipe-summary' },
    h('div', { class: 'recipe-block' },
      h('h4', {}, 'Abilities'),
      h('div', { class: 'row-things' }, [k.super, k.classAbility, k.movement, k.melee, k.grenade].map((i) => thing(model, i, { mark })))),
    h('div', { class: 'recipe-block' },
      h('h4', {}, 'Aspects and fragments'),
      h('div', { class: 'row-things' }, k.aspects.map((i) => thing(model, i, { mark })), things(model, k.fragments, { mark }))),
    h('div', { class: 'recipe-block wide' },
      h('h4', {}, 'Weapons'),
      h('ul', { class: 'weapons' }, r.weapons.map((w) => weaponRow(model, w, mark)))),
    r.stats ? h('div', { class: 'recipe-block wide' }, h('h4', {}, 'Stats'), statLine(r.stats)) : null);
}

/** The expandable rest of a recipe: armor, sets, artifact, transcendence. */
export function recipeRest(model, r, mark) {
  return h('div', { class: 'recipe-rest' },
    h('div', { class: 'recipe-block wide' },
      h('h4', {}, 'Armor'),
      h('div', { class: 'table-scroll' },
        h('table', { class: 'armor' },
          h('thead', {}, h('tr', {}, ['Slot', 'Piece', 'Archetype', 'Tuning', 'Stat mods', 'Mods'].map((c) => h('th', { scope: 'col' }, c)))),
          h('tbody', {}, r.armor.map((p) => h('tr', {},
            h('td', { class: 'slot-label' }, ARMOR_SLOTS[p.slot]),
            h('td', {}, thing(model, p.item, { cls: p.exotic ? 'thing strong' : 'thing', mark }),
              p.perks.length ? h('div', { class: 'perks' }, p.perks.map((i) => thing(model, i, { cls: 'perk', mark }))) : null),
            h('td', {}, thing(model, p.archetype)),
            h('td', {}, thing(model, p.tuning)),
            h('td', {}, things(model, p.statMods)),
            h('td', {}, h('div', { class: 'stack' }, things(model, p.mods, { mark }))))))))),
    r.sets.length || r.setBonuses.length ? h('div', { class: 'recipe-block' },
      h('h4', {}, 'Armor sets'),
      h('div', { class: 'row-things' },
        r.sets.map(([s, pieces]) => h('span', { class: 'thing' }, `${model.data.dict[s][0]} (${plural(pieces, 'piece')})`)),
        r.setBonuses.map((i) => thing(model, i, { mark })))) : null,
    r.kit.transcendence.length ? h('div', { class: 'recipe-block' },
      h('h4', {}, 'Transcendence'), h('div', { class: 'row-things' }, r.kit.transcendence.map((i) => thing(model, i)))) : null,
    r.artifact.length ? h('div', { class: 'recipe-block wide' },
      h('h4', {}, 'Artifact perks'), h('div', { class: 'row-things' }, r.artifact.map((i) => thing(model, i, { mark })))) : null);
}

/** A details element whose body is built on first open. */
export function lazyDetails(summary, build, cls = 'more') {
  const d = h('details', { class: cls }, h('summary', {}, summary));
  d.addEventListener('toggle', () => { if (d.open && d.childElementCount === 1) d.append(build()); });
  return d;
}

function variantItem(model, top, v, mark) {
  const r = model.recipe(v[0]);
  const diff = recipeDiff(model, top, r);
  const matched = mark && recipeIncludes(model, v[0]).has(mark);
  return h('li', { class: `variant${matched ? ' matched' : ''}` },
    h('p', { class: 'recipe-meta' }, pickLine(v), matched ? h('span', { class: 'match-badge' }, ` includes ${mark}`) : null),
    diff.length
      ? h('ul', { class: 'diff' }, diff.map((d) => h('li', {}, h('span', { class: 'diff-label' }, `${d.label}: `), d.text)))
      : h('p', { class: 'muted' }, 'Same as the top recipe in every listed part.'),
    lazyDetails('Full recipe', () => h('div', {}, recipeSummary(model, r, mark), recipeRest(model, r, mark))));
}

function elsewhere(model, typeId, current, go) {
  const others = model.appearances[typeId].filter((a) => a.scope.id !== current.id && a.scope.kind !== 'all');
  if (others.length === 0) return null;
  others.sort((a, b) => b.row[1] - a.row[1]);
  return lazyDetails(`Ranked in ${others.length} other activit${others.length === 1 ? 'y' : 'ies'}`, () =>
    h('table', { class: 'elsewhere' },
      h('thead', {}, h('tr', {}, h('th', { scope: 'col' }, 'Activity'), h('th', { scope: 'col' }, 'Effect'), h('th', { scope: 'col' }, 'Players'))),
      h('tbody', {}, others.map((a) => h('tr', {},
        h('td', {}, h('button', { type: 'button', class: 'link', onclick: () => go(a.scope.id) }, scopeLabel(a.scope))),
        h('td', { class: a.row[1] >= 0 ? 'pos-text' : 'neg-text' }, pct(a.row[1])),
        h('td', {}, n(a.row[4])))))));
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
        h('div', { class: 'effect' },
          h('span', { class: `effect-value ${effect >= 0 ? 'pos-text' : 'neg-text'}` }, pct(effect)),
          h('span', { class: 'effect-label' }, 'build effect')),
        effectBar(effect),
        h('p', { class: 'secondary' }, `raw ${pct(raw)} · players’ skill ${pct(skill)}`),
        h('p', { class: 'secondary' }, `${n(players)} players · ${n(Math.round(runs))} runs`),
        h('p', { class: 'secondary' }, `last used ${last}`))),
    h('section', { class: `recipe${topMatched ? ' matched' : ''}`, 'aria-label': 'Top recipe' },
      h('div', { class: 'recipe-head' },
        h('h4', { class: 'recipe-title' }, 'Top recipe'),
        h('p', { class: 'recipe-meta' }, topLine(row), topMatched ? h('span', { class: 'match-badge' }, ` includes ${mark}`) : null)),
      recipeSummary(model, recipe, mark),
      lazyDetails('Armor, mods and artifact', () => recipeRest(model, recipe, mark))),
    total ? lazyDetails(
      `${total} near-identical variant${total === 1 ? '' : 's'}${more ? ` (showing the best ${variants.length})` : ''}`,
      () => h('ol', { class: 'variants' }, variants.map((v) => variantItem(model, recipe, v, mark))),
      'more variants-details') : null,
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
export function startBook(root, data) {
  const model = prepare(data);
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

  function render() {
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
  const bookLink = (t) => h('a', { class: 'browse-link', href: bookHash(t) }, 'Look for this build type in the ranked book →');

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
        else book = fetchJson('data/focalpoint.json').then((data) => startBook(root, data));
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
