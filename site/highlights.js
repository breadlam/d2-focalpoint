// Skinwalker highlights: per class and subclass, exact builds that are as
// well ranked and as unlike each other as the data allows — for the neutral
// game (workhorses that hold up across activities: up to three per classic
// subclass and six Prismatic) and for each activity (up to one per
// subclass). Read from data/book/highlights.<set>.json, which
// `focalpoint site` exports (see docs/site.md and docs/adr/0007-highlights.md).

import {
  DEFAULTS, FORMAT, NONE, buildLink, decodeRecipe, elementBadge, findHash, formatHash, h, lazyDetails,
  loadoutPanel, pct, plural, shareButton, typeIdentity, usageLine,
} from './app.js';

export const NEUTRAL = 'neutral';
export const HIGHLIGHT_DEFAULTS = Object.freeze({ set: NEUTRAL, cls: 'Hunter' });
const CLASSES = ['Hunter', 'Titan', 'Warlock'];
/** Subclasses in the order the export lists them. */
export const SUBCLASSES = ['Arc', 'Solar', 'Void', 'Stasis', 'Strand', 'Prismatic'];

/** Highlights state from the URL: `#view=highlights&set=…&class=…`. */
export function parseHighlightHash(hash) {
  const p = new URLSearchParams(hash.replace(/^#/, ''));
  const cls = p.get('class');
  return {
    set: p.get('set') || HIGHLIGHT_DEFAULTS.set,
    cls: CLASSES.includes(cls) ? cls : HIGHLIGHT_DEFAULTS.cls,
  };
}

export function formatHighlightHash(state) {
  const p = new URLSearchParams({ view: 'highlights' });
  if (state.set !== HIGHLIGHT_DEFAULTS.set) p.set('set', state.set);
  p.set('class', state.cls);
  return `#${p}`;
}

/** A set's picks of one class. */
export function picksOf(file, cls) {
  return file.classes.find((c) => c.class === cls)?.picks ?? [];
}

/** Picks grouped by subclass, in subclass order: `[[subclass, [pick]]]`. */
export function bySubclass(types, picks) {
  const groups = new Map();
  for (const p of picks) {
    const el = types[p[0]].element ?? '';
    if (!groups.has(el)) groups.set(el, []);
    groups.get(el).push(p);
  }
  const rank = (el) => (SUBCLASSES.includes(el) ? SUBCLASSES.indexOf(el) : SUBCLASSES.length);
  return [...groups].sort(([a], [b]) => rank(a) - rank(b));
}

/** The ranked book's URL for a build type in a scope, showing every listing. */
export function rankedHash(t, scope) {
  return formatHash({
    ...DEFAULTS, scope, min: 1, cls: t.class ?? '', element: t.element ?? '', armor: t.armor ?? NONE,
    weapon: t.weapon ?? NONE, sup: t.super ?? '', aspect1: t.aspects[0] ?? '', aspect2: t.aspects[1] ?? '',
  });
}

/** How much of a pick's playstyle its subclass's picks above don't share ("" for the first). */
export function noveltyLine(k, novelty) {
  if (k === 0) return '';
  const share = Math.round(novelty * 100);
  return share >= 99 ? 'distinct' : `${share}% distinct`;
}

function highlightCard(model, file, pick, k, groups) {
  // [type, effect, players, runs, last used, recipe, activity groups, novelty]
  const [type, effect, players, runs, last, recipeIndex, usedIn, novelty] = pick;
  const t = model.data.types[type];
  const r = decodeRecipe(file.recipes[recipeIndex]);
  const neutral = file.id === NEUTRAL;
  const scope = neutral ? 'all' : file.id;
  return h('li', { class: 'card highlight' },
    h('div', { class: 'card-main' },
      h('span', { class: 'rank' }, `${k + 1}`),
      typeIdentity(model, t),
      h('div', { class: 'metrics' },
        usageLine(players, runs),
        neutral ? h('p', { class: 'secondary' }, `${usedIn} of ${groups} activity groups`) : null,
        h('p', { class: 'secondary' }, `last used ${last}`),
        k > 0 ? h('p', { class: 'secondary novelty' }, noveltyLine(k, novelty)) : null)),
    h('section', { class: 'recipe', 'aria-label': 'Build' },
      loadoutPanel(model, r),
      shareButton(buildLink(t, r.id))),
    lazyDetails('Ranking details', () => h('div', { class: 'ranking-details' },
      h('p', {}, `${neutral ? 'Neutral-game effect' : 'Effect'} ${pct(effect)}`))),
    h('p', { class: 'card-links' },
      h('a', { class: 'browse-link', href: rankedHash(t, scope) }, 'In Ranked Builds →'),
      h('a', { class: 'browse-link', href: findHash(t) }, 'All builds of this type →')));
}

/** Run the highlights view in `root`; returns `{ refresh }` for URL changes. */
export async function startHighlights(root, data, load) {
  if (data.format !== FORMAT) {
    throw new Error(`data format ${data.format}, expected ${FORMAT}: re-export with this version of focalpoint`);
  }
  const $ = (id) => root.querySelector(`#${id}`);
  const model = { data };
  const files = new Map();
  const sets = new Map(data.highlights.map(([id, name, file]) => [id, { id, name, file }]));
  const groups = data.scopes.filter((s) => s.kind === 'group').length;
  let state = parseHighlightHash(location.hash);
  let generation = 0;

  // Set picker: the neutral game, then each activity group and its activities.
  const setSelect = $('hl-set');
  const option = (id, label) => (sets.has(id) ? h('option', { value: id }, label) : null);
  const byParent = new Map();
  for (const s of data.scopes) if (s.kind === 'activity') {
    if (!byParent.has(s.parent)) byParent.set(s.parent, []);
    byParent.get(s.parent).push(s);
  }
  setSelect.replaceChildren(option(NEUTRAL, 'Neutral game'),
    ...data.scopes.filter((s) => s.kind === 'group').map((s) => {
      const children = byParent.get(s.name) ?? [];
      return h('optgroup', { label: s.name },
        option(s.id, children.length ? `All ${s.name}` : s.name),
        children.map((c) => option(c.id, c.name)));
    }));
  setSelect.addEventListener('change', () => update({ set: setSelect.value }));
  for (const input of root.querySelectorAll('input[name="hl-class"]')) {
    input.addEventListener('change', () => update({ cls: input.value }));
  }

  function update(patch) {
    state = { ...state, ...patch };
    const hash = formatHighlightHash(state);
    if (hash !== location.hash) history.replaceState(null, '', hash);
    render();
  }

  async function render() {
    const mine = ++generation;
    if (!sets.has(state.set)) state = { ...state, set: NEUTRAL };
    setSelect.value = state.set;
    for (const input of root.querySelectorAll('input[name="hl-class"]')) input.checked = input.value === state.cls;
    const set = sets.get(state.set);
    const note = $('hl-note');
    const list = $('hl-results');
    note.classList.remove('error');
    if (!files.has(set.id)) {
      note.textContent = 'Loading…';
      const file = await load(`data/book/${set.file}`);
      if (file.format !== FORMAT) throw new Error(`${set.file}: data format ${file.format}, expected ${FORMAT}`);
      files.set(set.id, file);
      if (mine !== generation) return;
    }
    const file = files.get(set.id);
    const picks = picksOf(file, state.cls);
    note.textContent = set.id === NEUTRAL ? 'Builds that hold up in any activity.' : '';
    $('hl-summary').textContent = plural(picks.length, 'build');
    list.replaceChildren(...bySubclass(model.data.types, picks).map(([subclass, group]) =>
      h('section', { class: 'subclass-group', 'aria-label': subclass },
        h('h3', { class: 'subclass-heading' }, elementBadge(subclass)),
        h('ol', { class: 'results' }, group.map((p, k) => highlightCard(model, file, p, k, groups))))));
  }

  await render();
  return {
    refresh: () => {
      state = parseHighlightHash(location.hash);
      return render();
    },
  };
}
