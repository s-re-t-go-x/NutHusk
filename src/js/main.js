import { el, badge, icon, toast, pluginUrl, trustTag, fmtSize } from './util.js';
import * as S from './state.js';
import { renderHome } from './home.js';
import { renderManage } from './manage.js';
import { renderSettings } from './settings.js';
import { renderDownload, createDownloadState } from './download.js';
import { openPalette } from './palette.js';
import { showDialog, isDialogOpen, closeActiveDialog } from './dialog.js';
import { importPackages, importDroppedFiles } from './importer.js';

const { invoke } = window.__TAURI__.core;
const { HOME, DOWNLOAD, MANAGE, SETTINGS } = S;
const isPage = (id) => id === HOME || id === DOWNLOAD || id === MANAGE || id === SETTINGS;
const PAGE_TABS = {
  [HOME]: ['home', '首页'],
  [DOWNLOAD]: ['download', '插件下载'],
  [MANAGE]: ['plugin', '插件管理'],
  [SETTINGS]: ['settings', '设置'],
};

const app = {
  plugins: [],
  favorites: S.loadListWithFallback(localStorage, 'nuthusk.favorites', 'toolbox.favorites'),
  recent: S.loadListWithFallback(localStorage, 'nuthusk.recent', 'toolbox.recent'),
  tabs: [HOME],
  active: HOME,
  homeFilter: 'all', // 'all' | 'favorites' | 'recent' | 'cat:<分类名>'
  frames: new Map(), // 插件 id -> iframe（标签切换时保留，不丢失工具里的状态）
  manage: { filter: 'all', query: '', openLogs: new Set(), logs: new Map() },
  downloadState: createDownloadState(),
  runtimes: [],
  settings: { ...S.DEFAULT_SETTINGS },
  dirs: [],
  signers: [],
  version: '',
};

const $ = (id) => document.getElementById(id);
const navEl = $('nav');
const navBottomEl = $('nav-bottom');
const tabsEl = $('tabs');
const toolheadEl = $('toolhead');
const stageEl = $('stage');
const homeEl = $('home');
const downloadEl = $('download');
const manageEl = $('manage');
const settingsEl = $('settings');
const statusEl = $('status');
const byId = (id) => app.plugins.find((p) => p.id === id);
const isUsable = (p) => p && p.enabled && !p.error;

/* ---------- 主题 ---------- */

const darkQuery = window.matchMedia?.('(prefers-color-scheme: dark)');
const effectiveTheme = () => (app.settings.theme === 'system' ? (darkQuery?.matches ? 'dark' : 'light') : app.settings.theme);

function sendTheme(frame) {
  frame.contentWindow?.postMessage({ type: 'theme', theme: app.settings.theme }, '*');
}

function applyTheme() {
  const t = app.settings.theme;
  if (t === 'light' || t === 'dark') document.documentElement.setAttribute('data-theme', t);
  else document.documentElement.removeAttribute('data-theme');
  try { localStorage.setItem('nuthusk.theme', t); } catch { /* 只是少了一次防闪烁的记忆 */ }
  for (const frame of app.frames.values()) sendTheme(frame); // 插件界面同步
  renderTopbar();
}

function renderTopbar() {
  const dark = effectiveTheme() === 'dark';
  $('theme-toggle').replaceChildren(icon(dark ? 'moon' : 'sun', 17));
  $('theme-toggle').title = dark ? '当前为深色，点击切换为浅色' : '当前为浅色，点击切换为深色';
  $('theme-toggle').setAttribute('aria-label', '切换亮/暗主题');
  if (!$('search-icon').firstChild) $('search-icon').append(icon('search', 15));
}

darkQuery?.addEventListener?.('change', renderTopbar);

/* ---------- 标签与插件界面 ---------- */

function createFrame(p) {
  const frame = el('iframe', { class: 'frame', title: p.name });
  frame.setAttribute('sandbox', 'allow-scripts'); // 不加 allow-same-origin：插件拿不到宿主任何权限
  frame.addEventListener('load', () => sendTheme(frame));
  frame.src = pluginUrl(p.id, p.ui);
  stageEl.append(frame);
  app.frames.set(p.id, frame);
}

function openPlugin(id) {
  const p = byId(id);
  if (!p) return;
  if (p.error) return toast(`${p.name} 无法加载：${p.error}`);
  if (!p.enabled) return toast(`${p.name} 已禁用，请先在卡片上启用`);
  if (!p.ui) return toast(`${p.name} 没有界面`);

  app.tabs = S.openTab(app.tabs, id);
  if (!app.frames.has(id)) createFrame(p);
  app.recent = S.pushRecent(app.recent, id);
  S.saveList(localStorage, 'nuthusk.recent', app.recent);
  activate(id);
}

function dropTab(id) {
  const r = S.closeTab(app.tabs, app.active, id);
  app.tabs = r.tabs;
  app.active = r.active;
  app.frames.get(id)?.remove();
  app.frames.delete(id);
}

function closeTab(id) {
  dropTab(id);
  activate(app.active);
}

function activate(id) {
  app.active = id;
  renderAll();
  if (!isPage(id)) app.frames.get(id)?.focus();
  if (id === DOWNLOAD && !app.downloadState.sources.length) {
    actions.loadMarketSources();
  }
  if (id === SETTINGS) {
    loadDirs().then(() => { if (app.active === SETTINGS) renderAll(); });
    actions.loadRuntimes();
  }
}

function openPage(id) {
  app.tabs = S.openTab(app.tabs, id);
  activate(id);
}

/* ---------- 渲染 ---------- */

function renderSidebar() {
  const cats = new Map();
  for (const p of app.plugins) cats.set(p.category, (cats.get(p.category) ?? 0) + 1);
  const item = (key, label, iconName, count) => el('button', {
    class: 'nav-item' + (app.active === HOME && app.homeFilter === key ? ' active' : ''),
    type: 'button',
    onclick: () => { app.homeFilter = key; activate(HOME); },
  },
    el('span', { class: 'nav-icon' }, icon(iconName, 16)),
    el('span', { class: 'nav-label', textContent: label }),
    count === undefined ? null : el('span', { class: 'nav-count', textContent: String(count) }));

  const favCount = app.favorites.filter((id) => byId(id)).length;
  navEl.replaceChildren(
    item('all', '首页', 'home'),
    item('favorites', '收藏', 'star', favCount),
    item('recent', '最近使用', 'clock'),
    el('div', { class: 'nav-group', textContent: '分类' }),
    ...[...cats].sort(([a], [b]) => a.localeCompare(b, 'zh')).map(([c, n]) => item('cat:' + c, c, 'folder', n)));

  const bad = app.plugins.filter((p) => p.error).length;
  const pageItem = (id, label, iconName, extra = null) => el('button', {
    class: 'nav-item' + (app.active === id ? ' active' : ''),
    type: 'button',
    onclick: () => actions.openPage(id),
  }, el('span', { class: 'nav-icon' }, icon(iconName, 16)), el('span', { class: 'nav-label', textContent: label }), extra);
  navBottomEl.replaceChildren(
    pageItem(DOWNLOAD, '插件下载', 'download'),
    pageItem(MANAGE, '插件管理', 'plugin', bad ? el('span', { class: 'nav-count', title: '有插件加载失败', textContent: `⚠ ${bad}` }) : null),
    pageItem(SETTINGS, '设置', 'settings'));
}

function renderTabs() {
  tabsEl.replaceChildren(...app.tabs.map((id) => {
    const page = PAGE_TABS[id];
    const p = page ? null : byId(id);
    const selected = id === app.active;
    return el('div', {
      class: 'tab' + (selected ? ' active' : ''),
      role: 'tab',
      'aria-selected': String(selected),
      tabIndex: 0,
      onclick: () => activate(id),
      onauxclick: (e) => { if (e.button === 1 && id !== HOME) { e.preventDefault(); closeTab(id); } },
      onkeydown: (e) => { if (e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); activate(id); } },
    },
      page ? el('span', { class: 'nav-icon' }, icon(page[0], 14)) : badge(p ?? { id, name: '?' }, true),
      el('span', { class: 'tab-title', textContent: page ? page[1] : (p?.name ?? id) }),
      id !== HOME ? el('button', {
        class: 'tab-close', type: 'button', title: '关闭（Ctrl W）', 'aria-label': '关闭',
        textContent: '×',
        onclick: (e) => { e.stopPropagation(); closeTab(id); },
      }) : null);
  }));
}

/** 工具页顶栏：图标、名称、说明、来源标识、收藏。 */
function renderToolhead() {
  const p = isPage(app.active) ? null : byId(app.active);
  toolheadEl.classList.toggle('hidden', !p);
  if (!p) return;
  const fav = app.favorites.includes(p.id);
  toolheadEl.replaceChildren(
    badge(p),
    el('div', { class: 'th-text' },
      el('div', { class: 'th-name' }, p.name, el('span', { class: 'muted small', textContent: ` v${p.version}` })),
      el('div', { class: 'th-desc', textContent: p.description })),
    trustTag(p.trust),
    el('button', {
      class: 'star' + (fav ? ' on' : ''), type: 'button',
      title: fav ? '取消收藏' : '收藏', 'aria-label': fav ? '取消收藏' : '收藏',
      textContent: fav ? '★' : '☆',
      onclick: () => actions.toggleFavorite(p.id),
    }));
}

function renderStage() {
  homeEl.classList.toggle('hidden', app.active !== HOME);
  downloadEl.classList.toggle('hidden', app.active !== DOWNLOAD);
  manageEl.classList.toggle('hidden', app.active !== MANAGE);
  settingsEl.classList.toggle('hidden', app.active !== SETTINGS);
  for (const [id, frame] of app.frames) frame.classList.toggle('active', id === app.active);
  if (app.active === HOME) renderHome(homeEl, app, actions);
  if (app.active === DOWNLOAD) renderDownload(downloadEl, app, actions);
  if (app.active === MANAGE) renderManage(manageEl, app, actions);
  if (app.active === SETTINGS) renderSettings(settingsEl, app, actions);
  renderToolhead();
}

function renderStatus() {
  const total = app.plugins.length;
  const bad = app.plugins.filter((p) => p.error).length;
  const on = app.plugins.filter(isUsable).length;
  const cur = byId(app.active);
  statusEl.replaceChildren(
    el('span', {}, `已加载 ${total} 个插件 · 可用 ${on}`, bad ? el('span', { class: 'st-warn', textContent: ` · ${bad} 个加载失败` }) : null),
    el('span', { class: 'st-right' },
      cur ? el('span', { textContent: `${cur.name} v${cur.version}` }) : null,
      cur ? el('span', { class: 'st-sep' }) : null,
      el('span', { textContent: `NutHusk${app.version ? ' v' + app.version : ''}` })));
}

function renderAll() {
  renderSidebar();
  renderTabs();
  renderStage();
  renderStatus();
}

/* ---------- 数据加载 ---------- */

async function loadDirs() {
  try {
    app.dirs = await invoke('plugin_dirs');
  } catch (err) {
    toast(`读取插件目录失败：${err}`);
  }
}

async function refresh() {
  app.plugins = await invoke('list_plugins');
  for (const id of [...app.tabs]) {
    if (!isPage(id) && !isUsable(byId(id))) dropTab(id); // 被禁用、卸载或失效的插件，关闭其标签
  }
  if (app.homeFilter.startsWith('cat:') && !app.plugins.some((p) => 'cat:' + p.category === app.homeFilter)) {
    app.homeFilter = 'all';
  }
  if (app.active === SETTINGS || app.dirs.length) await loadDirs(); // 各目录的插件数会随之变化
  renderAll();
}

/* ---------- 动作 ---------- */

const actions = {
  open: openPlugin,
  openPage,

  toggleFavorite(id) {
    app.favorites = S.toggleItem(app.favorites, id);
    S.saveList(localStorage, 'nuthusk.favorites', app.favorites);
    renderAll();
  },

  async setEnabled(id, enabled) {
    try {
      await invoke('set_plugin_enabled', { id, enabled });
    } catch (err) {
      toast(String(err));
    }
    await refresh();
  },

  openSettings() {
    openPage(SETTINGS);
  },

  async patchSettings(patch) {
    try {
      app.settings = { ...app.settings, ...(await invoke('patch_settings', { patch })) };
    } catch (err) {
      toast(`保存设置失败：${err}`);
    }
    applyTheme();
    renderAll();
  },

  /** 调整插件目录优先级：delta 为 -1 表示上移，1 表示下移。 */
  async moveDir(key, delta) {
    const order = app.dirs.map((d) => d.key);
    const i = order.indexOf(key);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= order.length) return;
    [order[i], order[j]] = [order[j], order[i]];
    try {
      app.dirs = await invoke('set_dir_order', { order });
      app.settings = { ...app.settings, dirOrder: order };
      await refresh();
      toast('已调整目录优先级');
    } catch (err) {
      toast(`调整失败：${err}`);
    }
  },

  async pickAndImport() {
    const dialog = window.__TAURI__.dialog;
    if (!dialog?.open) return toast('当前环境不支持选择文件，请把插件包拖进窗口');
    let picked;
    try {
      picked = await dialog.open({ multiple: true, filters: [{ name: '插件包', extensions: ['nut', 'mytool', 'zip'] }] });
    } catch (err) {
      return toast(`无法打开文件选择框：${err}`);
    }
    const paths = picked ? [].concat(picked) : [];
    if (paths.length) await importPackages(paths, { invoke, refresh });
  },

  async uninstall(p) {
    const ok = await showDialog({
      title: '卸载插件',
      body: el('div', {},
        el('p', { textContent: `确定卸载「${p.name}」吗？它的文件夹会被删除，此操作不可撤销。` }),
        el('p', { class: 'muted small mrow-path', textContent: p.path })),
      buttons: [{ label: '取消', value: false }, { label: '卸载', value: true, danger: true, primary: true }],
    });
    if (!ok) return;
    try {
      await invoke('uninstall_plugin', { id: p.id });
      toast(`已卸载：${p.name}`);
    } catch (err) {
      toast(String(err));
    }
    app.manage.openLogs.delete(p.id);
    app.manage.logs.delete(p.id);
    await refresh();
  },

  async loadLogs(id) {
    try {
      app.manage.logs.set(id, await invoke('plugin_logs', { id }));
    } catch (err) {
      app.manage.logs.set(id, [`读取日志失败：${err}`]);
    }
    renderAll();
  },

  async toggleLogs(id) {
    if (app.manage.openLogs.has(id)) {
      app.manage.openLogs.delete(id);
      return renderAll();
    }
    app.manage.openLogs.add(id);
    renderAll();
    await actions.loadLogs(id);
  },

  async rescan() {
    try {
      await invoke('reload_plugins');
      await refresh();
      toast('已重新扫描插件目录');
    } catch (err) {
      toast(`扫描失败：${err}`);
    }
  },

  async loadRuntimes() {
    try {
      app.runtimes = await invoke('runtime_status');
    } catch (err) {
      toast(`读取运行环境状态失败：${err}`);
    }
    if (app.active === SETTINGS) renderAll();
  },

  async installRuntime(name) {
    toast(`正在下载并安装运行环境：${name}…`);
    try {
      await invoke('runtime_install', { name });
      toast(`运行环境 ${name} 安装成功！`);
      await actions.loadRuntimes();
    } catch (err) {
      toast(`安装失败：${err}`);
    }
  },

  async removeRuntime(name) {
    const ok = await showDialog({
      title: '卸载运行环境',
      body: el('p', { textContent: `确定卸载便携运行环境「${name}」吗？依赖此环境的插件将无法运行。` }),
      buttons: [{ label: '取消', value: false }, { label: '卸载', value: true, danger: true, primary: true }],
    });
    if (!ok) return;
    try {
      await invoke('runtime_remove', { name });
      toast(`已卸载运行环境：${name}`);
      await actions.loadRuntimes();
    } catch (err) {
      toast(`卸载失败：${err}`);
    }
  },

  async loadMarketSources() {
    try {
      app.downloadState.sources = await invoke('market_sources');
      if (!app.downloadState.sources.some((s) => s.id === app.downloadState.selectedSource)) {
        app.downloadState.selectedSource = app.downloadState.sources[0]?.id ?? 'official';
      }
      await actions.loadMarketSource(app.downloadState.selectedSource);
    } catch (err) {
      toast(`加载插件源失败：${err}`);
    }
  },

  async loadMarketSource(id) {
    app.downloadState.selectedSource = id;
    app.downloadState.loading = true;
    app.downloadState.error = null;
    if (app.active === DOWNLOAD) renderAll();
    try {
      const res = await invoke('market_index', { sourceId: id });
      app.downloadState.index = res.index ?? { name: res.name, entries: res.entries ?? res.plugins ?? [] };
      app.downloadState.trust = res.trust;
      app.downloadState.fromCache = !!res.fromCache;
      app.downloadState.error = res.error ?? null;
    } catch (err) {
      app.downloadState.error = String(err);
      toast(`获取插件索引失败：${err}`);
    } finally {
      app.downloadState.loading = false;
      if (app.active === DOWNLOAD) renderAll();
    }
  },

  async addSource(url, name) {
    try {
      await invoke('source_add', { url, name });
      toast('已添加插件源');
      await actions.loadMarketSources();
    } catch (err) {
      toast(`添加失败：${err}`);
    }
  },

  async removeSource(id) {
    try {
      await invoke('source_remove', { id });
      toast('已删除插件源');
      await actions.loadMarketSources();
    } catch (err) {
      toast(`删除失败：${err}`);
    }
  },

  async installFromMarket(entry) {
    const key = `pkg:${entry.url}`;
    app.downloadState.downloading.set(key, { downloaded: 0, total: entry.size || 0 });
    if (app.active === DOWNLOAD) renderAll();
    try {
      const tmpPath = await invoke('market_download', {
        url: entry.url,
        sha256: entry.sha256,
        sourceId: app.downloadState.selectedSource,
      });
      app.downloadState.downloading.delete(key);
      if (app.active === DOWNLOAD) renderAll();
      await importPackages([tmpPath], { invoke, refresh });
    } catch (err) {
      app.downloadState.downloading.delete(key);
      if (app.active === DOWNLOAD) renderAll();
      toast(`下载插件失败：${err}`);
    }
  },

  async installDirect(url) {
    const key = `pkg:${url}`;
    app.downloadState.downloading.set(key, { downloaded: 0, total: 0 });
    if (app.active === DOWNLOAD) renderAll();
    try {
      const tmpPath = await invoke('market_download', { url });
      app.downloadState.downloading.delete(key);
      if (app.active === DOWNLOAD) renderAll();
      await importPackages([tmpPath], { invoke, refresh });
    } catch (err) {
      app.downloadState.downloading.delete(key);
      if (app.active === DOWNLOAD) renderAll();
      toast(`下载插件失败：${err}`);
    }
  },

  async cancelDownload(key) {
    try {
      await invoke('download_cancel', { key });
      app.downloadState.downloading.delete(key);
      if (app.active === DOWNLOAD) renderAll();
    } catch (err) {
      toast(`取消失败：${err}`);
    }
  },
};

/* ---------- 快捷键 ---------- */

function showPalette() {
  openPalette({ plugins: app.plugins, favorites: app.favorites, recent: app.recent, onOpen: openPlugin });
}

function handleShortcut(e) {
  if (isDialogOpen()) return false; // 对话框打开时不响应全局快捷键
  if (!(e.ctrlKey || e.metaKey)) return false;
  const k = String(e.key ?? '').toLowerCase();
  if (k === 'k') { showPalette(); return true; }
  if (k === 'w' && e.ctrlKey) { if (app.active !== HOME) closeTab(app.active); return true; }
  if (/^[1-9]$/.test(k)) {
    const id = app.tabs[Number(k) - 1];
    if (id) activate(id);
    return true;
  }
  if (k === 'tab') { activate(S.cycleTab(app.tabs, app.active, e.shiftKey ? -1 : 1)); return true; }
  return false;
}

window.addEventListener('keydown', (e) => { if (handleShortcut(e)) e.preventDefault(); });
$('open-palette').addEventListener('click', showPalette);
$('theme-toggle').addEventListener('click', () => actions.patchSettings({ theme: effectiveTheme() === 'dark' ? 'light' : 'dark' }));
$('kbd-hint').textContent = /Mac/.test(navigator.platform ?? '') ? '⌘ K' : 'Ctrl K';

/* ---------- 运行环境缺失处理 ---------- */

let runtimeProgressCb = null;

async function handleRuntimeMissing(pluginId, reqData, frame, info) {
  const p = byId(pluginId);
  const pluginName = p ? p.name : pluginId;

  if (!info.downloadable) {
    await showDialog({
      title: '缺少运行环境',
      body: el('div', {},
        el('p', { textContent: `插件「${pluginName}」需要运行环境 ${info.name}，但当前系统尚未安装，且暂无可自动下载的便携包。` }),
        el('p', { class: 'muted small', textContent: info.description || '请先在系统中安装此环境并加入系统 PATH 后重试。' })),
      buttons: [{ label: '确定', primary: true, value: true }],
    });
    return false;
  }

  const sizeText = info.size ? `约 ${(info.size / 1024 / 1024).toFixed(1)} MB` : '体积未知';
  const ok = await showDialog({
    title: '缺少运行环境',
    body: el('div', {},
      el('p', { textContent: `插件「${pluginName}」需要运行环境「${info.name}」才能执行。` }),
      el('p', { class: 'muted small', textContent: `系统当前缺少该环境。是否立即下载并配置官方便携版？（${sizeText}）` }),
      el('p', { class: 'muted small', textContent: '下载后仅保存在应用数据目录，不污染系统 PATH。' })),
    buttons: [
      { label: '取消', value: false },
      { label: '立即下载并继续', value: true, primary: true },
    ],
  });

  if (!ok) return false;

  const progressFill = el('div', { class: 'progress-fill', style: 'width: 0%' });
  const progressText = el('span', { textContent: '准备下载…' });
  const dlgBody = el('div', { class: 'dlg-sec' },
    el('p', { textContent: `正在下载并解压便携运行时：${info.name}…` }),
    el('div', { class: 'progress-box', style: 'margin-top: 12px;' },
      el('div', { class: 'progress-bar' }, progressFill),
      el('div', { class: 'progress-text' }, progressText)));

  let cancelled = false;
  showDialog({
    title: '正在配置运行环境',
    body: dlgBody,
    buttons: [{ label: '取消', value: false }],
  }).then((res) => {
    if (res === false) {
      cancelled = true;
      invoke('download_cancel', { key: `runtime:${info.name}` }).catch(() => {});
    }
  });

  runtimeProgressCb = ({ downloaded, total }) => {
    if (total > 0) {
      const pct = Math.min(100, Math.round((downloaded / total) * 100));
      progressFill.style.width = `${pct}%`;
      progressText.textContent = `${fmtSize(downloaded)} / ${fmtSize(total)} (${pct}%)`;
    } else {
      progressText.textContent = fmtSize(downloaded);
    }
  };

  try {
    await invoke('runtime_install', { name: info.name });
    runtimeProgressCb = null;
    closeActiveDialog(true);
    toast(`运行环境 ${info.name} 安装成功，正在继续执行…`);
  } catch (installErr) {
    runtimeProgressCb = null;
    closeActiveDialog(false);
    if (!cancelled) toast(`运行环境安装失败：${installErr}`);
    return false;
  }

  // 自动无缝重试原插件后端调用
  try {
    const retryResult = await invoke('plugin_call', {
      id: pluginId,
      method: String(reqData.method),
      params: reqData.params ?? null,
    });
    frame.contentWindow?.postMessage({ reqId: reqData.reqId, result: retryResult }, '*');
    return true;
  } catch (retryErr) {
    frame.contentWindow?.postMessage({ reqId: reqData.reqId, error: String(retryErr) }, '*');
    return true;
  }
}

/* ---------- 全局下载进度监听 ---------- */

window.__TAURI__.event?.listen?.('download-progress', (e) => {
  const payload = e?.payload;
  if (!payload || !payload.key) return;
  const { key, downloaded, total } = payload;
  if (key.startsWith('pkg:')) {
    app.downloadState.downloading.set(key, { downloaded, total });
    if (app.active === DOWNLOAD) renderAll();
  } else if (key.startsWith('runtime:')) {
    if (runtimeProgressCb) runtimeProgressCb({ downloaded, total });
  }
});

/* ---------- 桥接：插件界面 → 壳 → Rust → 插件后端 ---------- */

window.addEventListener('message', async (e) => {
  const entry = [...app.frames].find(([, f]) => f.contentWindow === e.source);
  if (!entry) return; // 只接受来自已打开的插件 iframe 的消息
  const [id, frame] = entry;
  const d = e.data ?? {};

  if (d.type === 'key') { handleShortcut(d); return; } // SDK 转发的快捷键（焦点在插件内时）
  if (d.type !== 'call') return;

  try {
    const result = await invoke('plugin_call', { id, method: String(d.method), params: d.params ?? null });
    frame.contentWindow?.postMessage({ reqId: d.reqId, result }, '*');
  } catch (err) {
    const errStr = String(err);
    const rmIdx = errStr.indexOf('RuntimeMissing:');
    if (rmIdx >= 0) {
      let missingInfo = null;
      try {
        missingInfo = JSON.parse(errStr.slice(rmIdx + 'RuntimeMissing:'.length));
      } catch { /* 容忍解析错误 */ }
      if (missingInfo) {
        const handled = await handleRuntimeMissing(id, d, frame, missingInfo);
        if (handled) return;
      }
    }
    frame.contentWindow?.postMessage({ reqId: d.reqId, error: errStr }, '*');
  }
});

/* ---------- 拖入插件包 ----------
 * 用浏览器自带的拖放（窗口配置里已关闭 Tauri 的原生文件拖放）。事件只会到达宿主自己的界面；
 * 落在插件 iframe 上的拖放归插件自己处理，这样需要接收文件的工具（如图片压缩）不会被抢走。 */

const dropOverlay = $('drop-overlay');
let dragDepth = 0;
const hasFiles = (e) => [...(e.dataTransfer?.types ?? [])].includes('Files');

window.addEventListener('dragenter', (e) => {
  if (!hasFiles(e)) return;
  e.preventDefault();
  dragDepth++;
  dropOverlay.classList.remove('hidden');
});
window.addEventListener('dragover', (e) => {
  if (!hasFiles(e)) return;
  e.preventDefault(); // 不取消默认行为的话，浏览器会把文件当作要打开的页面
  e.dataTransfer.dropEffect = 'copy';
});
window.addEventListener('dragleave', (e) => {
  if (!hasFiles(e)) return;
  dragDepth = Math.max(0, dragDepth - 1);
  if (!dragDepth) dropOverlay.classList.add('hidden');
});
window.addEventListener('drop', (e) => {
  if (!hasFiles(e)) return;
  e.preventDefault();
  dragDepth = 0;
  dropOverlay.classList.add('hidden');
  importDroppedFiles([...e.dataTransfer.files], { invoke, refresh });
});

/* ---------- 启动 ---------- */

async function start() {
  const [settings, signers, version] = await Promise.allSettled([
    invoke('get_settings'),
    invoke('trusted_signers'),
    window.__TAURI__.app?.getVersion?.() ?? '',
  ]);
  if (settings.status === 'fulfilled' && settings.value) app.settings = { ...app.settings, ...settings.value };
  if (signers.status === 'fulfilled' && Array.isArray(signers.value)) app.signers = signers.value;
  if (version.status === 'fulfilled' && typeof version.value === 'string') app.version = version.value;
  applyTheme();
  await refresh();
}

renderTopbar();
start().catch((err) => toast(`启动失败：${err}`));
