// Searchlight build rankings: a static client for data/searchlight.json,
// which `searchlight site` exports (see docs/site.md).
//
// The pure functions (filtering, facet counts, ranking, URL state) are
// exported for tests/site/test.html; start() wires them to the page.

export const FORMAT = 1;
export const STATS = ['Weapons', 'Health', 'Class', 'Grenade', 'Super', 'Melee'];
/** "Run with" matches components in at least this percent of a build's runs. */
export const RUN_WITH_MIN = 25;
/** Value standing for "no exotic" in the exotic armor and weapon filters. */
export const NONE = '(none)';
/** Below this many captured runs, a scope's ranking is flagged as tentative. */
export const THIN_SCOPE = 500;
const PAGE = 30;

export const DEFAULTS = Object.freeze({
  scope: 'all', cls: '', element: '', armor: '', weapon: '', sup: '',
  aspect1: '', aspect2: '', with: '', min: 3, sort: 'effect',
});
const HASH_KEYS = {
  scope: 'activity', cls: 'class', element: 'element', armor: 'armor', weapon: 'weapon',
  sup: 'super', aspect1: 'aspect', aspect2: 'aspect2', with: 'with', min: 'min', sort: 'sort',
};
export const SORTS = {
  effect: { label: 'Build effect', column: 1 },
  raw: { label: 'Raw result', column: 2 },
  players: { label: 'Players', column: 4 },
  runs: { label: 'Runs', column: 5 },
};
const KIND_LABELS = {
  fragments: 'Fragments', weapons: 'Weapons', armor_mods: 'Armor mods',
  artifact_perks: 'Artifact perks', set_bonuses: 'Set bonuses',
  class_item_perks: 'Class item perks', grenade: 'Grenades', melee: 'Melees',
  class_ability: 'Class abilities', movement: 'Movement',
};

/** A log-scale effect as a signed percentage: 0.1 → "+11%". */
export function pct(x) {
  const v = Math.round((Math.exp(x) - 1) * 100);
  if (v === 0) return '±0%';
  return (v > 0 ? '+' : '−') + Math.abs(v) + '%';
}

/** Index the exported data for filtering. */
export function prepare(data) {
  if (data.format !== FORMAT) {
    throw new Error(`data format ${data.format}, expected ${FORMAT}: re-export with this version of searchlight`);
  }
  const groups = data.groups.map((g, id) => ({ ...g, id }));
  const scopes = new Map(data.scopes.map((s) => [s.id, s]));
  const appearances = groups.map(() => []);
  for (const s of data.scopes) for (const r of s.rows) appearances[r[0]].push({ scope: s, row: r });
  // Per group: component name → kind, for components in ≥ RUN_WITH_MIN% of runs.
  const runWith = groups.map((g) => {
    const m = new Map();
    for (const [kind, list] of Object.entries(g.details.components)) {
      for (const [n, p] of list) if (p >= RUN_WITH_MIN && !m.has(data.names[n])) m.set(data.names[n], kind);
    }
    return m;
  });
  return { data, groups, scopes, appearances, runWith };
}

/** The values a group offers for one facet. */
function values(model, g, facet) {
  switch (facet) {
    case 'cls': return g.class ? [g.class] : [];
    case 'element': return g.element ? [g.element] : [];
    case 'armor': return [g.armor ?? NONE];
    case 'weapon': return [g.weapon ?? NONE];
    case 'sup': return g.super ? [g.super] : [];
    case 'aspect1':
    case 'aspect2': return g.aspects;
    case 'with': return [...model.runWith[g.id].keys()];
    default: return [];
  }
}

/** Whether a group passes every filter except `skip`. */
export function matches(model, g, state, skip = null) {
  if (skip !== 'cls' && state.cls && g.class !== state.cls) return false;
  if (skip !== 'element' && state.element && g.element !== state.element) return false;
  if (skip !== 'armor' && state.armor && (g.armor ?? NONE) !== state.armor) return false;
  if (skip !== 'weapon' && state.weapon && (g.weapon ?? NONE) !== state.weapon) return false;
  if (skip !== 'sup' && state.sup && g.super !== state.sup) return false;
  if (skip !== 'aspect1' && state.aspect1 && !g.aspects.includes(state.aspect1)) return false;
  if (skip !== 'aspect2' && state.aspect2 && !g.aspects.includes(state.aspect2)) return false;
  if (skip !== 'with' && state.with && !model.runWith[g.id].has(state.with)) return false;
  return true;
}

export function scopeOf(model, state) {
  return model.scopes.get(state.scope) ?? model.scopes.get('all');
}

/** Ranked rows of the selected scope that pass the filters. */
export function results(model, state) {
  const scope = scopeOf(model, state);
  const col = (SORTS[state.sort] ?? SORTS.effect).column;
  const rows = scope.rows.filter((r) => r[4] >= state.min && matches(model, model.groups[r[0]], state));
  rows.sort((a, b) => b[col] - a[col] || b[1] - a[1] || b[4] - a[4] || a[0] - b[0]);
  return { scope, rows };
}

/**
 * Options for one facet with the number of builds each would leave, given
 * every other filter: Map(value → count).
 */
export function facetOptions(model, state, facet) {
  const counts = new Map();
  for (const r of scopeOf(model, state).rows) {
    if (r[4] < state.min) continue;
    const g = model.groups[r[0]];
    if (!matches(model, g, state, facet)) continue;
    for (const v of values(model, g, facet)) counts.set(v, (counts.get(v) ?? 0) + 1);
  }
  return counts;
}

/** Kinds of the "run with" options, for grouping them. */
export function runWithKinds(model) {
  const kinds = new Map();
  for (const m of model.runWith) for (const [name, kind] of m) if (!kinds.has(name)) kinds.set(name, kind);
  return kinds;
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

// ---------------------------------------------------------------------------
// DOM
// ---------------------------------------------------------------------------

/** Create an element: h('div', { class: 'x' }, child, 'text', ...). */
function h(tag, attrs = {}, ...children) {
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
const n = (x) => fmt.format(x);

function icon(model, name, cls = 'icon') {
  const path = name && model.data.icons[name];
  return path ? h('img', { class: cls, src: model.data.icon_base + path, alt: '', loading: 'lazy', width: 40, height: 40 }) : null;
}

function elementBadge(element) {
  return element ? h('span', { class: `badge el-${element.toLowerCase()}` }, element) : null;
}

/** A diverging bar for a log-scale effect (±50% fills one side). */
function effectBar(x) {
  const v = Math.max(-1, Math.min(1, (Math.exp(x) - 1) / 0.5));
  const side = v >= 0 ? 'pos' : 'neg';
  return h('span', { class: 'bar', 'aria-hidden': 'true' },
    h('span', { class: `fill ${side}`, style: `width:${(Math.abs(v) * 50).toFixed(1)}%` }));
}

function componentList(model, g, kind, limit = 12) {
  const list = g.details.components[kind];
  if (!list || list.length === 0) return null;
  return h('section', { class: 'detail' },
    h('h4', {}, KIND_LABELS[kind] ?? kind),
    h('ul', { class: 'chips' }, list.slice(0, limit).map(([i, p]) => {
      const name = model.data.names[i];
      return h('li', { class: 'chip' }, icon(model, name, 'chip-icon'), h('span', {}, name), h('span', { class: 'share' }, `${p}%`));
    })));
}

function statFocus(g) {
  if (!g.details.stats.some((x) => x > 0)) return null;
  return h('section', { class: 'detail' },
    h('h4', {}, 'Stat focus'),
    h('ul', { class: 'stats' }, STATS.map((s, i) => {
      const level = g.details.stats[i];
      return h('li', {},
        h('span', { class: 'stat-name' }, s),
        h('span', { class: 'stat-bar', 'aria-hidden': 'true' }, h('span', { style: `width:${(level / 4 * 100).toFixed(0)}%` })),
        h('span', { class: 'stat-value' }, `≈${Math.round(level * 50)}`));
    })));
}

/** "Master Raid · Vault of Glass: Master", without repeating a prefix. */
export function scopeLabel(scope) {
  if (!scope.parent || scope.name.startsWith(scope.parent)) return scope.name;
  return `${scope.parent} · ${scope.name}`;
}

function elsewhere(model, g, current, go) {
  const others = model.appearances[g.id].filter((a) => a.scope.id !== current.id && a.scope.kind !== 'all');
  if (others.length === 0) return null;
  others.sort((a, b) => b.row[1] - a.row[1]);
  return h('section', { class: 'detail wide' },
    h('h4', {}, 'In other activities'),
    h('table', { class: 'elsewhere' },
      h('thead', {}, h('tr', {}, h('th', { scope: 'col' }, 'Activity'), h('th', { scope: 'col' }, 'Effect'), h('th', { scope: 'col' }, 'Players'))),
      h('tbody', {}, others.slice(0, 12).map((a) => h('tr', {},
        h('td', {}, h('button', { type: 'button', class: 'link', onclick: () => go(a.scope.id) },
          scopeLabel(a.scope))),
        h('td', { class: a.row[1] >= 0 ? 'pos-text' : 'neg-text' }, pct(a.row[1])),
        h('td', {}, n(a.row[4])))))));
}

function details(model, g, scope, go) {
  return h('div', { class: 'details-body' },
    h('p', { class: 'muted' }, `How it is usually run, from ${n(Math.round(g.details.runs))} captured clears in all activities. Percentages are shares of those runs.`),
    h('div', { class: 'detail-grid' },
      ['grenade', 'melee', 'class_ability', 'movement'].map((k) => componentList(model, g, k, 3)),
      statFocus(g),
      componentList(model, g, 'fragments', 8),
      componentList(model, g, 'class_item_perks', 6),
      componentList(model, g, 'weapons', 8),
      componentList(model, g, 'armor_mods', 10),
      componentList(model, g, 'set_bonuses', 6),
      componentList(model, g, 'artifact_perks', 10),
      elsewhere(model, g, scope, go)));
}

function card(model, row, rank, scope, go) {
  const g = model.groups[row[0]];
  const [, effect, raw, skill, players, runs, last] = row;
  const title = [g.armor ?? 'No exotic armor', g.weapon ?? 'No exotic weapon'];
  const body = h('details', { class: 'more' }, h('summary', {}, 'How it’s run'));
  body.addEventListener('toggle', () => {
    if (body.open && body.childElementCount === 1) body.append(details(model, g, scope, go));
  }, { once: false });
  return h('li', { class: 'card' },
    h('div', { class: 'card-main' },
      h('span', { class: 'rank' }, `${rank}`),
      h('div', { class: 'identity' },
        h('div', { class: 'exotics' },
          h('span', { class: 'exotic' }, icon(model, g.armor), h('span', { class: g.armor ? '' : 'muted' }, title[0])),
          h('span', { class: 'exotic' }, icon(model, g.weapon), h('span', { class: g.weapon ? '' : 'muted' }, title[1]))),
        h('p', { class: 'subtitle' }, elementBadge(g.element), ` ${g.class ?? ''}`, g.super ? ` · ${g.super}` : ''),
        h('ul', { class: 'aspects' }, g.aspects.map((a) => h('li', {}, icon(model, a, 'chip-icon'), a)))),
      h('div', { class: 'metrics' },
        h('div', { class: 'effect' },
          h('span', { class: `effect-value ${effect >= 0 ? 'pos-text' : 'neg-text'}` }, pct(effect)),
          h('span', { class: 'effect-label' }, 'build effect')),
        effectBar(effect),
        h('p', { class: 'secondary' }, `raw ${pct(raw)} · players’ skill ${pct(skill)}`),
        h('p', { class: 'secondary' }, `${n(players)} players · ${n(Math.round(runs))} runs`),
        h('p', { class: 'secondary' }, `last used ${last}`))),
    body);
}

function fillSelect(select, options, value, anyLabel) {
  select.replaceChildren(h('option', { value: '' }, anyLabel), ...options);
  select.value = value;
  if (select.value !== value) {
    // The selection has no builds left; keep it visible so it can be undone.
    select.append(h('option', { value }, `${value === NONE ? 'None' : value} (0)`));
    select.value = value;
  }
}

function optionList(counts, label = (v) => v) {
  return [...counts.entries()]
    .sort((a, b) => (a[0] === NONE ? -1 : b[0] === NONE ? 1 : a[0].localeCompare(b[0])))
    .map(([v, c]) => h('option', { value: v }, `${label(v)} (${c})`));
}

export function start(root, data) {
  const model = prepare(data);
  const kinds = runWithKinds(model);
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
    // "Run with": grouped by kind.
    const withCounts = facetOptions(model, state, 'with');
    const groups = new Map();
    for (const [name, count] of withCounts) {
      const kind = kinds.get(name) ?? 'other';
      if (!groups.has(kind)) groups.set(kind, new Map());
      groups.get(kind).set(name, count);
    }
    const order = Object.keys(KIND_LABELS);
    fillSelect($('with'), [...groups.entries()]
      .sort((a, b) => order.indexOf(a[0]) - order.indexOf(b[0]))
      .map(([kind, counts]) => h('optgroup', { label: KIND_LABELS[kind] ?? kind }, optionList(counts))),
    state.with, 'Anything');
    $('min').value = String(state.min);
    $('sort').value = state.sort;
  }

  function renderScopeNote(scope) {
    const note = $('scope-note');
    note.replaceChildren(
      `Ranked from ${n(scope.clears)} qualified clears by ${n(scope.players)} players; `,
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
    const list = $('results');
    list.replaceChildren(...rows.slice(0, shown).map((r, i) => card(model, r, i + 1, scope, go)));
    $('empty').hidden = rows.length !== 0;
    $('more').hidden = rows.length <= shown;
  }

  // Wire controls.
  scopeSelect.addEventListener('change', () => update({ scope: scopeSelect.value }));
  for (const input of root.querySelectorAll('input[name="class"]')) {
    input.addEventListener('change', () => update({ cls: input.value }));
  }
  for (const [facet] of facets) $(facet).addEventListener('change', (e) => update({ [facet]: e.target.value }));
  $('with').addEventListener('change', (e) => update({ with: e.target.value }));
  $('min').addEventListener('change', (e) => update({ min: Number.parseInt(e.target.value, 10) }));
  $('sort').addEventListener('change', (e) => update({ sort: e.target.value }));
  $('reset').addEventListener('click', () => update({ ...DEFAULTS, scope: state.scope }));
  $('more').addEventListener('click', () => { shown += PAGE; update({}, { resetPage: false }); });
  window.addEventListener('hashchange', () => { state = parseHash(location.hash); shown = PAGE; render(); });

  $('data-range').textContent = `${data.first_run} to ${data.last_run}`;
  $('total-clears').textContent = n(scopeOf(model, { scope: 'all' }).clears);
  render();
  root.classList.remove('loading');
}
