import { el, badge, icon, trustTag, fmtSize, toast } from './util.js';
import { showDialog } from './dialog.js';

export function createDownloadState() {
  return {
    sources: [],
    selectedSource: 'official',
    index: null,
    trust: null,
    fromCache: false,
    error: null,
    query: '',
    category: 'all',
    loading: false,
    downloading: new Map(), // key -> { downloaded, total }
  };
}

function filterEntries(entries, query, cat) {
  const q = query.trim().toLowerCase();
  return entries.filter((e) => {
    if (cat !== 'all' && (e.category || '其他') !== cat) return false;
    if (!q) return true;
    return [e.name, e.id, e.description, e.author].some((s) => String(s ?? '').toLowerCase().includes(q));
  });
}

function sourceDialog(actions) {
  const urlInput = el('input', {
    class: 'input-text',
    type: 'url',
    placeholder: 'https://... （直接 .nut 下载链接，或私有 index.json 索引地址）',
  });
  const nameInput = el('input', {
    class: 'input-text',
    type: 'text',
    placeholder: '源名称（选填，仅在添加为源时生效）',
  });
  const body = el('div', { class: 'dlg-sec' },
    el('p', { textContent: '支持粘贴两种地址：' }),
    el('ul', { style: 'margin: 0; padding-left: 20px; font-size: 13px; color: var(--muted);' },
      el('li', { textContent: '插件包地址：以 .nut / .zip 结尾的 HTTPS 下载直链，直接下载并确认安装。' }),
      el('li', { textContent: '私有源地址：指向 index.json 的 HTTPS 索引地址，添加后可在本页浏览该源的所有插件。' })),
    el('div', { style: 'margin-top: 8px;' }, urlInput),
    el('div', { style: 'margin-top: 6px;' }, nameInput));

  showDialog({
    title: '手动安装 / 添加私有源',
    body,
    buttons: [
      {
        label: '直接下载安装',
        primary: true,
        value: 'download',
      },
      {
        label: '添加为插件源',
        value: 'add_source',
      },
      { label: '取消', value: null },
    ],
  }).then(async (val) => {
    const url = urlInput.value.trim();
    if (!url) return;
    if (val === 'download') {
      await actions.installDirect(url);
    } else if (val === 'add_source') {
      await actions.addSource(url, nameInput.value.trim() || null);
    }
  });
}

function entryCard(e, app, actions, downloading) {
  const installed = app.plugins.find((p) => p.id === e.id);
  const prog = downloading.get(`pkg:${e.url}`);
  const isUpgradable = installed && installed.version !== e.version;

  let actionArea;
  if (prog) {
    const pct = prog.total ? Math.round((prog.downloaded / prog.total) * 100) : 0;
    actionArea = el('div', { class: 'progress-box' },
      el('div', { class: 'progress-bar' }, el('div', { class: 'progress-fill', style: `width: ${pct}%` })),
      el('div', { class: 'progress-text' },
        el('span', { textContent: prog.total ? `${fmtSize(prog.downloaded)} / ${fmtSize(prog.total)}` : fmtSize(prog.downloaded) }),
        el('button', { class: 'linkbtn', type: 'button', textContent: '取消', onclick: () => actions.cancelDownload(`pkg:${e.url}`) })));
  } else if (installed && !isUpgradable) {
    actionArea = el('div', { style: 'display: flex; align-items: center; justify-content: space-between;' },
      el('span', { class: 'tag', textContent: `已安装 v${installed.version}` }),
      el('button', { class: 'btn sm', type: 'button', textContent: '重新安装', onclick: () => actions.installFromMarket(e) }));
  } else if (isUpgradable) {
    actionArea = el('div', { style: 'display: flex; align-items: center; justify-content: space-between;' },
      el('span', { class: 'tag warn', textContent: `已装 v${installed.version}` }),
      el('button', { class: 'btn sm primary', type: 'button', textContent: '更新', onclick: () => actions.installFromMarket(e) }));
  } else {
    actionArea = el('button', {
      class: 'btn sm primary',
      type: 'button',
      style: 'width: 100%;',
      textContent: '下载安装',
      onclick: () => actions.installFromMarket(e),
    });
  }

  return el('div', { class: 'card' },
    el('div', { class: 'card-top' },
      badge({ id: e.id, name: e.name }),
      el('div', { style: 'margin-left: auto; font-size: 12px; color: var(--muted);', textContent: `v${e.version}` })),
    el('div', { class: 'card-name', textContent: e.name }),
    e.author ? el('div', { class: 'card-author', textContent: `作者：${e.author}` }) : null,
    el('div', { class: 'card-desc', textContent: e.description || '暂无说明' }),
    el('div', { class: 'card-foot' },
      el('span', { class: 'tag', textContent: e.category || '其他' }),
      e.size ? el('span', { class: 'tag', textContent: fmtSize(e.size) }) : null),
    el('div', { style: 'margin-top: 10px;' }, actionArea));
}

export function renderDownload(root, app, actions) {
  const m = app.downloadState;
  const entries = m.index?.entries ?? m.index?.plugins ?? [];
  const cats = new Set(['all']);
  for (const e of entries) cats.add(e.category || '其他');

  const listEl = el('div', { class: 'grid' });
  const drawList = () => {
    if (m.loading) {
      listEl.replaceChildren(el('div', { class: 'empty', textContent: '正在获取插件索引…' }));
      return;
    }
    if (m.error && !entries.length) {
      listEl.replaceChildren(el('div', { class: 'empty', textContent: `获取失败：${m.error}` }));
      return;
    }
    const filtered = filterEntries(entries, m.query, m.category);
    listEl.replaceChildren(...(filtered.length
      ? filtered.map((e) => entryCard(e, app, actions, m.downloading))
      : [el('div', { class: 'empty', textContent: entries.length ? '没有符合条件的插件' : '源里暂无插件' })]));
  };

  const currentSource = m.sources.find((s) => s.id === m.selectedSource) ?? m.sources[0];

  const sourceSelector = el('div', { class: 'source-select' },
    el('span', { textContent: '当前插件源：' }),
    el('select', {
      onchange: (e) => {
        m.selectedSource = e.target.value;
        actions.loadMarketSource(m.selectedSource);
      },
    }, ...m.sources.map((s) => el('option', { value: s.id, selected: s.id === m.selectedSource, textContent: s.name }))),
    currentSource && !currentSource.official
      ? el('button', {
        class: 'btn sm danger',
        type: 'button',
        textContent: '删除此源',
        onclick: () => actions.removeSource(currentSource.id),
      })
      : null);

  const search = el('input', {
    class: 'msearch',
    type: 'search',
    placeholder: '搜索插件名称、作者、说明',
    value: m.query,
    'aria-label': '搜索插件',
    oninput: (e) => { m.query = e.target.value; drawList(); },
  });

  const chips = el('div', { class: 'mchips' }, ...[...cats].map((c) => el('button', {
    class: 'mchip' + (m.category === c ? ' active' : ''),
    type: 'button',
    textContent: c === 'all' ? '全部' : c,
    onclick: () => { m.category = c; renderDownload(root, app, actions); },
  })));

  const sourceInfoBox = currentSource ? el('div', { class: 'source-info' },
    el('span', { textContent: `地址：${currentSource.url}` }),
    m.trust ? trustTag(m.trust) : null,
    m.fromCache ? el('span', { class: 'tag warn', title: m.error || '', textContent: '离线缓存' }) : null) : null;

  root.replaceChildren(
    el('div', { class: 'mhead' },
      el('div', {},
        el('h2', { class: 'mtitle', textContent: '插件下载' }),
        el('div', { class: 'muted small', textContent: '收录精选常用插件，支持官方源与私有源' })),
      el('div', { class: 'mbtns' },
        el('button', { class: 'btn', type: 'button', onclick: () => actions.loadMarketSource(m.selectedSource) }, icon('refresh', 14), ' 刷新'),
        el('button', { class: 'btn primary', type: 'button', onclick: () => sourceDialog(actions) }, icon('plus', 14), ' 手动安装 / 添加源'))),
    el('div', { style: 'margin-bottom: 12px; display: flex; flex-direction: column; gap: 8px;' },
      sourceSelector,
      sourceInfoBox),
    el('div', { class: 'mtools' }, search, chips),
    listEl);

  drawList();
}
