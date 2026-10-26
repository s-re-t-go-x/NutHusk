import { el, badge, icon, switchEl, trustTag, PERM_LABEL } from './util.js';

function card(p, app, actions) {
  const fav = app.favorites.includes(p.id);
  const openIt = () => actions.open(p.id);
  return el('div', {
    class: 'card' + (p.error ? ' error' : '') + (p.enabled ? '' : ' off'),
    tabIndex: 0,
    role: 'button',
    onclick: openIt,
    onkeydown: (e) => {
      if (e.target !== e.currentTarget) return;
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        openIt();
      }
    },
  },
    el('div', { class: 'card-top' },
      badge(p),
      el('button', {
        class: 'star' + (fav ? ' on' : ''),
        type: 'button',
        title: fav ? '取消收藏' : '收藏',
        'aria-label': fav ? '取消收藏' : '收藏',
        textContent: fav ? '★' : '☆',
        onclick: (e) => { e.stopPropagation(); actions.toggleFavorite(p.id); },
      })),
    el('div', { class: 'card-name', textContent: p.name }),
    el('div', { class: 'card-desc', textContent: p.error ? p.error : p.description }),
    el('div', { class: 'card-foot' },
      el('span', { class: 'tag', textContent: p.category }),
      trustTag(p.trust),
      ...p.permissions.filter((x) => x === 'network').map((x) => el('span', { class: 'tag warn', textContent: PERM_LABEL[x] })),
      el('span', { class: 'spacer' }),
      switchEl({
        checked: p.enabled,
        disabled: !!p.error,
        title: p.enabled ? '已启用，点击禁用' : '已禁用，点击启用',
        label: '启用或禁用',
        onclick: (e) => actions.setEnabled(p.id, e.target.checked),
      })));
}

const grid = (list, app, actions, extra = null) => el('div', { class: 'grid' }, ...list.map((p) => card(p, app, actions)), extra);
const section = (title, content) => el('section', { class: 'sec' }, el('h2', { textContent: title }), content);
const empty = (text) => el('div', { class: 'empty', textContent: text });

const importCard = (actions) => el('div', {
  class: 'card import',
  tabIndex: 0,
  role: 'button',
  onclick: () => actions.pickAndImport(),
  onkeydown: (e) => { if (e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); actions.pickAndImport(); } },
},
  el('div', { class: 'import-plus' }, icon('plus', 26)),
  el('div', { class: 'card-name', textContent: '导入插件' }),
  el('div', { class: 'card-desc', textContent: '选择 .nut 插件包，或直接拖进窗口' }));

export function renderHome(root, app, actions) {
  const byId = new Map(app.plugins.map((p) => [p.id, p]));
  const existing = (ids) => ids.map((id) => byId.get(id)).filter(Boolean);
  const usable = (ids) => existing(ids).filter((p) => p.enabled && !p.error);
  const f = app.homeFilter;
  const out = [];

  if (app.plugins.length === 0) {
    out.push(empty('还没有任何插件。点击左下角的「插件管理」导入，或把插件包拖进窗口。'));
    out.push(el('div', { class: 'grid' }, importCard(actions)));
  } else if (f === 'all') {
    const recent = usable(app.recent);
    const favs = existing(app.favorites);
    // 首页是否显示这两块，由设置页控制
    if (app.settings.showRecent && recent.length) {
      out.push(section('最近使用', el('div', { class: 'chips' }, ...recent.map((p) =>
        el('button', { class: 'chip', type: 'button', onclick: () => actions.open(p.id) }, badge(p, true), p.name)))));
    }
    if (app.settings.showFavorites && favs.length) out.push(section('收藏', grid(favs, app, actions)));
    out.push(section('全部工具', grid(app.plugins, app, actions, importCard(actions))));
  } else if (f === 'favorites') {
    const favs = existing(app.favorites);
    out.push(section('收藏', favs.length ? grid(favs, app, actions) : empty('点击卡片右上角的 ☆，把常用工具固定在这里。')));
  } else if (f === 'recent') {
    const recent = existing(app.recent);
    out.push(section('最近使用', recent.length ? grid(recent, app, actions) : empty('还没有使用记录。')));
  } else if (f.startsWith('cat:')) {
    const cat = f.slice(4);
    const list = app.plugins.filter((p) => p.category === cat);
    out.push(section(cat, list.length ? grid(list, app, actions) : empty('这个分类下没有工具。')));
  }

  root.replaceChildren(...out);
}
