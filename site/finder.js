// Find any build: the catalog of every collected build (data/catalog/),
// exported by `focalpoint site` (layout in docs/site.md).
//
// The index (names, activities, one line per build type) is loaded once.
// Shards of builds are fetched only for the build types the filters select,
// and only once the selection is small enough to search in the browser.
// The pure functions are exported for tests/site/test.html.

import {
  INCLUDE_KINDS, NONE, componentsOf, elementBadge, fillSelect, h, loadoutPanel, n, optionList, plural, shareButton,
} from './app.js';

export const CATALOG_FORMAT = 3;
/** The most builds the page downloads to search their recipes. */
export const SEARCH_LIMIT = 45000;
const PAGE = 30;

export const FIND_DEFAULTS = Object.freeze({
  cls: '', element: '', armor: '', weapon: '', sup: '', aspect1: '', aspect2: '', act: '', has: [], sort: 'rank', build: '',
});
const FIND_KEYS = {
  cls: 'class', element: 'element', armor: 'armor', weapon: 'weapon', sup: 'super',
  aspect1: 'aspect', aspect2: 'aspect2', act: 'act', sort: 'sort', build: 'build',
};
export const FIND_SORTS = {
  rank: 'Best ranked, then clears',
  clears: 'Clears',
  runs: 'Runs',
  recent: 'Most recent',
};

const num = (s) => (s === '-' ? null : Number.parseInt(s, 36));
const nums = (s) => (s === '-' ? [] : s.split(',').map((x) => Number.parseInt(x, 36)));

function header(text, kind) {
  const first = text.slice(0, text.indexOf('\n'));
  const [magic, version] = first.split(' ');
  if (magic !== kind) throw new Error(`not a ${kind} file`);
  if (Number(version) !== CATALOG_FORMAT) {
    throw new Error(`catalog format ${version}, expected ${CATALOG_FORMAT}: re-export with this version of focalpoint`);
  }
}

/** Parse `catalog/index.txt`. */
export function parseIndex(text) {
  header(text, 'focalpoint-catalog');
  const meta = {}; const dict = []; const activities = []; const types = []; const stats = []; const elements = {};
  let section = '';
  for (const line of text.split('\n').slice(1)) {
    if (!line) continue;
    if (line[0] === '@') { section = line.slice(1); continue; }
    const f = line.split('\t');
    if (section === 'meta') meta[f[0]] = f[1];
    else if (section === 'dict') dict.push([f[0], f[1] || null, f[2] || null, f[3] || null]);
    else if (section === 'stats') stats.push([f[0], f[1] || null]);
    else if (section === 'elements') elements[f[0]] = f[1];
    else if (section === 'activities') activities.push({ name: f[0], group: f[1] || '' });
    else if (section === 'types') {
      types.push({
        id: types.length, class: f[0] || null, element: f[1] || null,
        armor: num(f[2]), weapon: num(f[3]), super: num(f[4]), aspects: nums(f[5]),
        builds: num(f[6]), runs: num(f[7]), players: num(f[8]), shards: nums(f[9]), activities: nums(f[10]),
      });
    }
  }
  const nameOf = (i) => (i === null ? null : dict[i][0]);
  for (const t of types) {
    t.armorName = nameOf(t.armor);
    t.weaponName = nameOf(t.weapon);
    t.superName = nameOf(t.super);
    t.aspectNames = t.aspects.map(nameOf);
  }
  return { meta, dict, activities, types, data: { dict, stats, elements, icon_base: meta.icon_base } };
}

/** Parse one shard into builds with decoded recipes (same shape as the book's). */
export function parseShard(text) {
  header(text, 'focalpoint-shard');
  const kits = []; const weapons = []; const armor = []; const artifact = []; const sets = []; const builds = [];
  let section = '';
  for (const line of text.split('\n').slice(1)) {
    if (!line) continue;
    if (line[0] === '@') { section = line.slice(1); continue; }
    const f = line.split(' ');
    switch (section) {
      case 'kits':
        kits.push({
          subclass: num(f[0]), super: num(f[1]), classAbility: num(f[2]), movement: num(f[3]),
          melee: num(f[4]), grenade: num(f[5]), aspects: nums(f[6]), fragments: nums(f[7]), transcendence: nums(f[8]),
        });
        break;
      case 'weapons':
        weapons.push({
          slot: Number(f[0]), item: num(f[1]), exotic: f[2] === '1', frame: num(f[3]),
          origin: num(f[4]), mod: num(f[5]), perks: nums(f[6]),
        });
        break;
      case 'armor':
        armor.push({
          slot: Number(f[0]), item: num(f[1]), exotic: f[2] === '1', archetype: num(f[3]), tuning: num(f[4]),
          statMods: nums(f[5]), mods: nums(f[6]), perks: nums(f[7]),
        });
        break;
      case 'artifact': artifact.push({ item: num(f[0]), perks: nums(f[1]), unlocked: f[2] === '1' }); break;
      case 'sets': {
        const entries = f[0] === '-' ? [] : f[0].split(',').map((x) => {
          const [s, p, b] = x.split(':');
          return { set: Number.parseInt(s, 36), pieces: Number(p), bonuses: b ? b.split('.').map((y) => Number.parseInt(y, 36)) : [] };
        });
        sets.push({ entries, bonuses: entries.flatMap((e) => e.bonuses) });
        break;
      }
      case 'builds': {
        const set = sets[num(f[6])];
        builds.push({
          id: f[0], type: num(f[1]),
          recipe: {
            id: f[0],
            stats: f[7] === '-' ? null : f[7].split(',').map((x) => Number.parseInt(x, 36)),
            kit: kits[num(f[2])],
            weapons: nums(f[3]).map((i) => weapons[i]),
            armor: nums(f[4]).map((i) => armor[i]),
            sets: set.entries, setBonuses: set.bonuses,
            artifactItem: artifact[num(f[5])].item, artifact: artifact[num(f[5])].perks,
            artifactUnlocked: artifact[num(f[5])].unlocked,
          },
          runs: num(f[8]), players: num(f[9]), clears: num(f[10]), qualified: num(f[11]),
          lastDay: num(f[12]), effect: f[13] === '-' ? null : Number.parseInt(f[13], 36) / 1000,
          activities: nums(f[14]),
        });
        break;
      }
      default: break;
    }
  }
  return builds;
}

/** "Master Raid" or "Master Raid|Vault of Glass: Master" against activity ids. */
function activityMatches(cat, ids, act) {
  if (!act) return true;
  const [group, name] = act.split('|');
  return ids.some((i) => cat.activities[i].group === group && (name === undefined || cat.activities[i].name === name));
}

/** Whether a build type passes every filter except `skip`. */
export function typeMatches(cat, t, state, skip = null) {
  if (skip !== 'cls' && state.cls && t.class !== state.cls) return false;
  if (skip !== 'element' && state.element && t.element !== state.element) return false;
  if (skip !== 'armor' && state.armor && (t.armorName ?? NONE) !== state.armor) return false;
  if (skip !== 'weapon' && state.weapon && (t.weaponName ?? NONE) !== state.weapon) return false;
  if (skip !== 'sup' && state.sup && t.superName !== state.sup) return false;
  if (skip !== 'aspect1' && state.aspect1 && !t.aspectNames.includes(state.aspect1)) return false;
  if (skip !== 'aspect2' && state.aspect2 && !t.aspectNames.includes(state.aspect2)) return false;
  if (skip !== 'act' && !activityMatches(cat, t.activities, state.act)) return false;
  return true;
}

/** Build types passing the filters. */
export function matchingTypes(cat, state) {
  return cat.types.filter((t) => typeMatches(cat, t, state));
}

/** For one facet: Map(value → builds) over types passing the other filters. */
export function typeFacet(cat, state, facet) {
  const counts = new Map();
  const values = (t) => {
    switch (facet) {
      case 'cls': return t.class ? [t.class] : [];
      case 'element': return t.element ? [t.element] : [];
      case 'armor': return [t.armorName ?? NONE];
      case 'weapon': return [t.weaponName ?? NONE];
      case 'sup': return t.superName ? [t.superName] : [];
      case 'aspect1': case 'aspect2': return t.aspectNames;
      default: return [];
    }
  };
  for (const t of cat.types) {
    if (!typeMatches(cat, t, state, facet)) continue;
    for (const v of values(t)) counts.set(v, (counts.get(v) ?? 0) + t.builds);
  }
  return counts;
}

/** Activity options: Map("group" or "group|name" → builds) over types passing the other filters. */
export function activityFacet(cat, state) {
  const counts = new Map();
  for (const t of cat.types) {
    if (!typeMatches(cat, t, state, 'act')) continue;
    const seen = new Set();
    for (const i of t.activities) {
      const a = cat.activities[i];
      if (!a.group) continue;
      seen.add(a.group);
      seen.add(`${a.group}|${a.name}`);
    }
    for (const k of seen) counts.set(k, (counts.get(k) ?? 0) + t.builds);
  }
  return counts;
}

/** Shards holding the given types. */
export function shardsOf(types) {
  return [...new Set(types.flatMap((t) => t.shards))].sort((a, b) => a - b);
}

/** Searchable components of a catalog build (cached on the build). */
function components(cat, b) {
  if (!b.components) b.components = componentsOf(cat, b.recipe);
  return b.components;
}

/** Builds of the matching types that pass the activity and "includes" filters, best first. */
export function matchingBuilds(cat, builds, types, state) {
  const ids = new Set(types.map((t) => t.id));
  const out = builds.filter((b) => ids.has(b.type)
    && (!state.build || b.id === state.build)
    && activityMatches(cat, b.activities, state.act)
    && state.has.every((x) => components(cat, b).has(x)));
  const effect = (b) => (b.effect === null ? -Infinity : b.effect);
  const by = {
    rank: (a, b) => effect(b) - effect(a) || b.qualified - a.qualified || b.clears - a.clears || b.runs - a.runs,
    clears: (a, b) => b.clears - a.clears || b.runs - a.runs,
    runs: (a, b) => b.runs - a.runs || b.clears - a.clears,
    recent: (a, b) => (b.lastDay ?? 0) - (a.lastDay ?? 0) || b.runs - a.runs,
  }[state.sort] ?? ((a, b) => b.runs - a.runs);
  return out.sort((a, b) => by(a, b) || (a.id < b.id ? -1 : 1));
}

/** "Includes" options over builds: Map(name → [kind, builds]). */
export function includeFacet(cat, builds) {
  const counts = new Map();
  for (const b of builds) {
    for (const [name, kind] of components(cat, b)) {
      const c = counts.get(name);
      if (c) c[1] += 1; else counts.set(name, [kind, 1]);
    }
  }
  return counts;
}

export function parseFindHash(hash) {
  const p = new URLSearchParams(hash.replace(/^#/, ''));
  const state = { ...FIND_DEFAULTS, has: p.getAll('has') };
  for (const [k, key] of Object.entries(FIND_KEYS)) {
    const v = p.get(key);
    if (v !== null) state[k] = v;
  }
  if (!(state.sort in FIND_SORTS)) state.sort = FIND_DEFAULTS.sort;
  return state;
}

export function formatFindHash(state) {
  const p = new URLSearchParams({ view: 'find' });
  for (const [k, key] of Object.entries(FIND_KEYS)) {
    if (state[k] && state[k] !== FIND_DEFAULTS[k]) p.set(key, state[k]);
  }
  for (const x of state.has) p.append('has', x);
  return `#${p}`;
}

/** Days since 1970 as YYYY-MM-DD. */
const day = (d) => (d === null ? '—' : new Date(d * 86400000).toISOString().slice(0, 10));

/** Fetch a text file; tolerate a server that sends it gzip-compressed without saying so. */
async function fetchText(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes[0] === 0x1f && bytes[1] === 0x8b) {
    const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip'));
    return new Response(stream).text();
  }
  return new TextDecoder().decode(bytes);
}

// ---------------------------------------------------------------------------
// DOM
// ---------------------------------------------------------------------------

function icon(cat, i, cls = 'icon') {
  const path = i === null || i === undefined ? null : cat.dict[i][1];
  return path ? h('img', { class: cls, src: cat.meta.icon_base + path, alt: '', loading: 'lazy', width: 40, height: 40 }) : null;
}

function buildRecord(b) {
  return `Used in ${plural(b.runs, 'run')} (${plural(b.clears, 'clear')}, ${n(b.qualified)} qualified) `
    + `by ${plural(b.players, 'player')} · last ${day(b.lastDay)}`;
}

/** The finder URL of one build: its type's names and its id. */
export function buildHash(t, id) {
  return formatFindHash({
    ...FIND_DEFAULTS, has: [], build: id,
    cls: t.class ?? '', element: t.element ?? '', armor: t.armorName ?? NONE, weapon: t.weaponName ?? NONE,
    sup: t.superName ?? '', aspect1: t.aspectNames[0] ?? '', aspect2: t.aspectNames[1] ?? '',
  });
}

function buildCard(cat, b, rank, has, book) {
  const mark = has[has.length - 1] ?? '';
  const t = cat.types[b.type];
  const r = b.recipe;
  const bookLink = book(t);
  return h('li', { class: 'card' },
    h('div', { class: 'card-main finder' },
      h('span', { class: 'rank' }, `${rank}`),
      h('div', { class: 'identity' },
        h('h3', { class: 'exotics' },
          h('span', { class: 'exotic' }, icon(cat, t.armor), h('span', { class: t.armor === null ? 'muted' : '' }, t.armorName ?? 'No exotic armor')),
          h('span', { class: 'exotic' }, icon(cat, t.weapon), h('span', { class: t.weapon === null ? 'muted' : '' }, t.weaponName ?? 'No exotic weapon'))),
        h('p', { class: 'subtitle' }, elementBadge(t.element), ` ${t.class ?? ''}`, t.superName ? ` · ${t.superName}` : ''),
        h('ul', { class: 'aspects' }, t.aspects.map((a, i) => h('li', {}, icon(cat, a, 'chip-icon'), t.aspectNames[i]))))),
    h('section', { class: 'recipe', 'aria-label': 'Build' },
      h('div', { class: 'recipe-head' },
        h('h4', { class: 'recipe-title' }, 'Build'),
        h('p', { class: 'recipe-meta' }, buildRecord(b),
          has.length ? h('span', { class: 'match-badge' }, ` · includes ${has.join(', ')}`) : null)),
      loadoutPanel(cat, r, mark),
      shareButton(buildHash(t, b.id))),
    bookLink);
}

function typeRow(cat, t, choose) {
  return h('li', { class: 'type-row' },
    h('button', { type: 'button', class: 'link', onclick: () => choose(t) },
      [t.armorName ?? 'No exotic armor', t.weaponName ?? 'No exotic weapon'].join(' + ')),
    h('span', { class: 'muted' }, ` ${t.element ?? ''} ${t.class ?? ''} · ${t.superName ?? '—'} · ${t.aspectNames.join(', ') || 'no aspects'} · ${plural(t.builds, 'build')}`));
}

/**
 * Run the finder in `root` (the page). `goBook(type)` returns a link to the
 * type's listing in the ranked book, or null.
 */
export async function startFinder(root, goBook) {
  const $ = (id) => root.querySelector(`#${id}`);
  const note = $('find-note');
  note.textContent = 'Loading the catalog index…';
  const cat = parseIndex(await fetchText('data/catalog/index.txt'));
  const shards = new Map(); // shard id → Promise of builds
  const loadShard = (i) => {
    if (!shards.has(i)) shards.set(i, fetchText(`data/catalog/s${String(i).padStart(3, '0')}.txt`).then(parseShard));
    return shards.get(i);
  };
  let state = parseFindHash(location.hash);
  let shown = PAGE;
  let generation = 0;

  const update = (patch, { resetPage = true } = {}) => {
    // A shared link pins one build; changing anything else lets it go.
    state = { ...state, build: '', ...patch };
    if (resetPage) shown = PAGE;
    const hash = formatFindHash(state);
    if (hash !== location.hash) history.replaceState(null, '', hash);
    render();
  };
  const chooseType = (t) => update({
    cls: t.class ?? '', element: t.element ?? '', armor: t.armorName ?? NONE, weapon: t.weaponName ?? NONE,
    sup: t.superName ?? '', aspect1: t.aspectNames[0] ?? '', aspect2: t.aspectNames[1] ?? '',
  });

  const facets = [
    ['element', 'Any element', (v) => v],
    ['armor', 'Any exotic armor', (v) => (v === NONE ? 'No exotic armor' : v)],
    ['weapon', 'Any exotic weapon', (v) => (v === NONE ? 'No exotic weapon' : v)],
    ['sup', 'Any super', (v) => v],
    ['aspect1', 'Any aspect', (v) => v],
    ['aspect2', 'Any aspect', (v) => v],
  ];

  function renderFilters() {
    const cls = typeFacet(cat, state, 'cls');
    for (const input of root.querySelectorAll('input[name="f-class"]')) {
      input.checked = input.value === state.cls;
      const count = input.value === '' ? null : cls.get(input.value) ?? 0;
      input.nextElementSibling.querySelector('.count').textContent = count === null ? '' : ` ${n(count)}`;
    }
    for (const [facet, any, label] of facets) {
      fillSelect($(`f-${facet}`), optionList(typeFacet(cat, state, facet), label), state[facet], any);
    }
    // Activities: groups, then their activities.
    const acts = activityFacet(cat, state);
    const groups = [...new Set(cat.activities.map((a) => a.group).filter(Boolean))].sort();
    fillSelect($('f-act'), groups.filter((g) => acts.has(g)).map((g) => h('optgroup', { label: g },
      h('option', { value: g }, `All ${g} (${n(acts.get(g))})`),
      [...new Set(cat.activities.filter((a) => a.group === g).map((a) => a.name))].sort()
        .filter((nm) => acts.has(`${g}|${nm}`))
        .map((nm) => h('option', { value: `${g}|${nm}` }, `${nm} (${n(acts.get(`${g}|${nm}`))})`)))),
    state.act, 'Any activity');
    $('f-sort').value = state.sort;
    $('f-chips').replaceChildren(...state.has.map((x) => h('li', { class: 'chip-filter' }, x,
      h('button', { type: 'button', 'aria-label': `Remove ${x}`, onclick: () => update({ has: state.has.filter((y) => y !== x) }) }, '×'))));
  }

  async function render() {
    const mine = ++generation;
    renderFilters();
    const types = matchingTypes(cat, state);
    const total = types.reduce((s, t) => s + t.builds, 0);
    const list = $('find-results');
    const typesBox = $('find-types');
    $('find-more').hidden = true;
    if (total === 0) {
      note.textContent = '';
      $('find-summary').textContent = 'No builds match.';
      list.replaceChildren();
      typesBox.replaceChildren();
      fillSelect($('f-has'), [], '', 'Add a component…');
      return;
    }
    if (total > SEARCH_LIMIT) {
      note.textContent = `${n(total)} builds in ${n(types.length)} build types match. Add a class, subclass, exotic, super or aspect `
        + `(or pick one of the build types below) to search their exact builds.`;
      $('find-summary').textContent = `${n(types.length)} build types`;
      list.replaceChildren();
      fillSelect($('f-has'), [], '', 'Narrow the build types first');
      $('f-has').disabled = true;
      const top = [...types].sort((a, b) => b.builds - a.builds).slice(0, 40);
      typesBox.replaceChildren(h('h3', { class: 'types-title' }, 'Most collected build types'),
        h('ul', { class: 'type-list' }, top.map((t) => typeRow(cat, t, chooseType))));
      return;
    }
    typesBox.replaceChildren();
    $('f-has').disabled = false;
    const ids = shardsOf(types);
    note.textContent = `Searching ${plural(total, 'build')} in ${plural(ids.length, 'shard')}…`;
    const builds = (await Promise.all(ids.map(loadShard))).flat();
    if (mine !== generation) return; // a newer render took over
    const typeIds = new Set(types.map((t) => t.id));
    const base = builds.filter((b) => typeIds.has(b.type));
    const found = matchingBuilds(cat, base, types, state);
    if (state.build) {
      note.replaceChildren(found.length
        ? 'A shared build. '
        : 'The shared build is not in this data (the link may come from an earlier export). ',
      h('button', { type: 'button', class: 'link', onclick: () => update({ build: '' }) },
        `Show all ${plural(total, 'build')} of this build type`));
    } else {
      note.textContent = `Every collected build of ${plural(types.length, 'build type')}: ${plural(total, 'build')}. `
        + 'Ranked builds come first; builds without qualified clears are listed by use.';
    }
    // "Includes" options narrow the current matches.
    const opts = includeFacet(cat, found);
    const grouped = new Map();
    for (const [nm, [kind, count]] of opts) {
      if (state.has.includes(nm)) continue;
      if (!grouped.has(kind)) grouped.set(kind, new Map());
      grouped.get(kind).set(nm, count);
    }
    const order = Object.keys(INCLUDE_KINDS);
    fillSelect($('f-has'), [...grouped.entries()]
      .sort((a, b) => order.indexOf(a[0]) - order.indexOf(b[0]))
      .map(([kind, counts]) => h('optgroup', { label: INCLUDE_KINDS[kind] ?? kind }, optionList(counts))),
    '', 'Add a component…');
    $('find-summary').textContent = found.length === 0
      ? 'No builds match.'
      : `${n(found.length)} build${found.length === 1 ? '' : 's'} match · showing ${n(Math.min(shown, found.length))}`;
    list.replaceChildren(...found.slice(0, shown).map((b, i) => buildCard(cat, b, i + 1, state.has, goBook)));
    $('find-more').hidden = found.length <= shown;
  }

  for (const input of root.querySelectorAll('input[name="f-class"]')) {
    input.addEventListener('change', () => update({ cls: input.value }));
  }
  for (const [facet] of facets) $(`f-${facet}`).addEventListener('change', (e) => update({ [facet]: e.target.value }));
  $('f-act').addEventListener('change', (e) => update({ act: e.target.value }));
  $('f-sort').addEventListener('change', (e) => update({ sort: e.target.value }));
  $('f-has').addEventListener('change', (e) => {
    if (e.target.value && !state.has.includes(e.target.value)) update({ has: [...state.has, e.target.value] });
  });
  $('f-reset').addEventListener('click', () => update({ ...FIND_DEFAULTS, has: [] }));
  $('find-more').addEventListener('click', () => { shown += PAGE; update({}, { resetPage: false }); });

  $('f-sort').replaceChildren(...Object.entries(FIND_SORTS).map(([v, label]) => h('option', { value: v }, label)));
  const refresh = () => { state = parseFindHash(location.hash); shown = PAGE; render(); };
  render();
  return { refresh };
}
