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

function workflowCard(wf, actions) {
  const runIt = () => actions.runWorkflow?.(wf);
  return el('div', {
    class: 'card wf-card',
    tabIndex: 0,
    role: 'button',
    onclick: runIt,
    onkeydown: (e) => {
      if (e.target !== e.currentTarget) return;
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        runIt();
      }
    },
  },
    el('div', { class: 'card-top' },
      badge({ id: wf.id, name: '⚡' }),
      el('div', { class: 'wf-card-btns', onclick: (e) => e.stopPropagation() },
        el('button', {
          class: 'btn sm',
          type: 'button',
          title: '编辑流水线',
          textContent: '编辑',
          onclick: () => actions.editWorkflow?.(wf),
        }),
        el('button', {
          class: 'btn sm',
          type: 'button',
          title: '删除流水线',
          textContent: '×',
          onclick: () => actions.deleteWorkflow?.(wf.id),
        }))),
    el('div', { class: 'card-name', textContent: wf.name }),
    el('div', { class: 'card-desc', textContent: wf.description || `${wf.steps?.length ?? 0} 个串联动作` }),
    el('div', { class: 'card-foot' },
      el('span', { class: 'tag ok', textContent: `⚡ ${wf.steps?.length ?? 0} 步流水线` }),
      el('span', { class: 'spacer' }),
      el('span', { class: 'muted small', textContent: '点击执行' })));
}

const grid = (list, app, actions, extra = null) => el('div', { class: 'grid' }, ...list.map((p) => card(p, app, actions)), extra);
const section = (title, content, headExtra = null) => el('section', { class: 'sec' },
  headExtra
    ? el('div', { class: 'sec-head' }, el('h2', { textContent: title }), headExtra)
    : el('h2', { textContent: title }),
  content);
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

    // 快捷流水线组合（当有已保存流水线，或有插件声明了 actions 时展示）
    const hasActionPlugins = app.plugins.some((p) => p.enabled && !p.error && Array.isArray(p.actions) && p.actions.length > 0);
    const workflows = Array.isArray(app.workflows) ? app.workflows : [];
    if (workflows.length > 0 || hasActionPlugins) {
      const addWfBtn = el('button', {
        class: 'btn sm',
        type: 'button',
        onclick: () => actions.createWorkflow?.(),
      }, '+ 新建流水线');
      const wfContent = workflows.length > 0
        ? el('div', { class: 'grid' }, ...workflows.map((wf) => workflowCard(wf, actions)))
        : el('div', { class: 'chips' },
            el('button', {
              class: 'chip',
              type: 'button',
              onclick: () => actions.createWorkflow?.(),
            }, icon('zap', 14), '串联多个插件动作（如 JSON 格式化 → 哈希计算）'));
      out.push(section('快捷流水线', wfContent, addWfBtn));
    }

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
