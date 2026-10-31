import { el, badge, icon, toast, pluginUrl, trustTag, fmtSize } from './util.js';
import * as S from './state.js';
import { renderHome } from './home.js';
import { renderManage } from './manage.js';
import { renderSettings } from './settings.js';
import { renderDownload, createDownloadState } from './download.js';
import { createDownloadActions } from './download-actions.js';
import { openPalette } from './palette.js';
import { showDialog, isDialogOpen, closeActiveDialog } from './dialog.js';
import { importPackages, importDroppedFiles } from './importer.js';
import { applyProfile } from './profile.js';
import { openWorkflowRunner, openWorkflowBuilder, promptSaveWorkspace } from './workflow.js';

const { invoke } = window.__TAURI__.core;
const errorMessage = (err) => err && typeof err === 'object' && typeof err.message === 'string' ? err.message : String(err);
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
  workspaces: S.loadWorkspaces(localStorage),
  workflows: S.loadWorkflows(localStorage),
  tabs: [HOME],
  active: HOME,
  splitId: null, // 右侧双栏分屏中并排展示的插件 id
  homeFilter: 'all', // 'all' | 'favorites' | 'recent' | 'cat:<分类名>'
  frames: new Map(), // 插件 id -> iframe（标签切换时保留，不丢失工具里的状态）
  loadedFrames: new Set(), // 已触发过 load 的插件 id
  frameLoadWaiters: new Map(), // 插件 id -> [resolve]
  pendingInputs: new Map(), // 插件 id -> 尚未派发的带内容打开 payload
  pendingActions: new Map(), // reqId -> { resolve, reject, timer }
  actionSeq: 0,
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

/* ---------- 主题与窗口置顶 ---------- */

const darkQuery = window.matchMedia?.('(prefers-color-scheme: dark)');
const effectiveTheme = () => app.settings.theme === 'system' ? (darkQuery?.matches ? 'dark' : 'light') : app.settings.theme;
const sendTheme = (frame) => frame.contentWindow?.postMessage({ type: 'theme', theme: app.settings.theme }, '*');

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

  const pinBtn = $('pin-toggle');
  if (pinBtn) {
    const pinned = !!app.settings.alwaysOnTop;
    pinBtn.replaceChildren(icon('pin', 16));
    pinBtn.classList.toggle('active', pinned);
    pinBtn.title = pinned ? '窗口已置顶，点击取消置顶' : '将窗口固定在最前端（窗口置顶）';
    pinBtn.setAttribute('aria-label', '切换窗口置顶');
    pinBtn.setAttribute('aria-pressed', String(pinned));
  }
  if (!$('search-icon').firstChild) $('search-icon').append(icon('search', 15));
}

darkQuery?.addEventListener?.('change', renderTopbar);

/* ---------- 标签、插件界面与双栏分屏 ---------- */

function createFrame(p) {
  const frame = el('iframe', { class: 'frame', title: p.name });
  frame.setAttribute('sandbox', 'allow-scripts'); // 不加 allow-same-origin：插件拿不到宿主任何权限
  frame.addEventListener('load', () => {
    app.loadedFrames.add(p.id);
    sendTheme(frame);
    if (app.pendingInputs.has(p.id)) {
      const payload = app.pendingInputs.get(p.id);
      app.pendingInputs.delete(p.id);
      frame.contentWindow?.postMessage({ type: 'input', payload }, '*');
    }
    const waiters = app.frameLoadWaiters.get(p.id);
    if (waiters) {
      app.frameLoadWaiters.delete(p.id);
      for (const fn of waiters) fn(frame);
    }
  });
  frame.src = pluginUrl(p.id, p.ui);
  stageEl.append(frame);
  app.frames.set(p.id, frame);
  return frame;
}

function ensureFrameLoaded(p) {
  if (!app.frames.has(p.id)) createFrame(p);
  const frame = app.frames.get(p.id);
  if (app.loadedFrames.has(p.id)) return Promise.resolve(frame);
  return new Promise((resolve) => {
    const list = app.frameLoadWaiters.get(p.id) ?? [];
    list.push(resolve);
    app.frameLoadWaiters.set(p.id, list);
  });
}

/** 调用单个插件动作（支持带后端进程的插件及注册了 nuthusk.onAction 的纯前端插件）。 */
async function executePluginAction(p, act, inputText) {
  const method = act.method || act.id;
  const params = {
    ...(act.defaultParams && typeof act.defaultParams === 'object' ? act.defaultParams : {}),
    text: String(inputText ?? ''),
  };

  if (p.hasBackend) {
    const wasRunning = !!p.backendRunning;
    const res = await invoke('plugin_call', { id: p.id, method, params }).catch((err) => { throw new Error(errorMessage(err)); });
    if (!wasRunning) await syncPluginStates();
    return res;
  }

  if (!p.ui) {
    throw new Error(`插件「${p.name}」既没有后端也没有界面`);
  }

  const frame = await ensureFrameLoaded(p);
  return new Promise((resolve, reject) => {
    const reqId = `act-${++app.actionSeq}`;
    const timer = setTimeout(() => {
      app.pendingActions.delete(reqId);
      reject(new Error(`插件「${p.name}」执行动作超时`));
    }, 8000);
    app.pendingActions.set(reqId, { pluginId: p.id, resolve, reject, timer });
    frame.contentWindow?.postMessage({ type: 'action', reqId, method, params }, '*');
  });
}

function deliverInputToPlugin(id, rawPayload) {
  if (!rawPayload) return;
  const payload = typeof rawPayload === 'string' ? { text: rawPayload } : rawPayload;
  const frame = app.frames.get(id);
  if (frame && app.loadedFrames.has(id)) {
    frame.contentWindow?.postMessage({ type: 'input', payload }, '*');
  } else {
    app.pendingInputs.set(id, payload);
  }
}

function openPlugin(id, inputPayload = null) {
  const p = byId(id);
  if (!p) return;
  if (p.error) return toast(`${p.name} 无法加载：${p.error}`);
  if (!p.enabled) return toast(`${p.name} 已禁用，请先在卡片上启用`);
  if (!p.ui) return toast(`${p.name} 没有界面`);

  app.tabs = S.openTab(app.tabs, id);
  if (!app.frames.has(id)) createFrame(p);
  if (inputPayload) deliverInputToPlugin(id, inputPayload);
  app.recent = S.pushRecent(app.recent, id);
  S.saveList(localStorage, 'nuthusk.recent', app.recent);
  activate(id);
}

/** 在右侧双栏分屏中打开或切换指定插件（不切换主标签）。 */
function setSplit(id, inputPayload = null) {
  if (!id || isPage(id) || id === app.active) {
    app.splitId = null;
    renderAll();
    return;
  }
  const p = byId(id);
  if (!p || p.error || !p.enabled || !p.ui) return;
  app.tabs = S.openTab(app.tabs, id);
  if (!app.frames.has(id)) createFrame(p);
  if (inputPayload) deliverInputToPlugin(id, inputPayload);
  app.splitId = id;
  renderAll();
}

function closeSplit() {
  if (!app.splitId) return;
  app.splitId = null;
  renderAll();
}

function swapSplit() {
  if (!app.splitId || isPage(app.active)) return;
  const prevActive = app.active;
  app.active = app.splitId;
  app.splitId = prevActive;
  renderAll();
}

function toggleSplitPicker() {
  if (isPage(app.active)) return toast('请先打开一个工具，再使用左右分屏');
  if (app.splitId) {
    closeSplit();
    return;
  }
  openPalette({
    plugins: app.plugins,
    favorites: app.favorites,
    recent: app.recent,
    mode: 'split',
    activeId: app.active,
    onOpen: (targetId) => setSplit(targetId),
  });
}

function switchToWorkspaceLayout(ws) {
  if (!ws || typeof ws !== 'object') return;
  const targetTabs = new Set([HOME]);
  for (const id of ws.tabs ?? []) {
    if (isPage(id)) {
      targetTabs.add(id);
    } else {
      const p = byId(id);
      if (isUsable(p) && p.ui) {
        targetTabs.add(id);
        if (!app.frames.has(id)) createFrame(p);
      }
    }
  }
  const nextActive = typeof ws.active === 'string' && targetTabs.has(ws.active) ? ws.active : HOME;
  let nextSplitId = null;
  if (typeof ws.splitId === 'string' && ws.splitId !== nextActive) {
    const sp = byId(ws.splitId);
    if (isUsable(sp) && sp.ui) {
      targetTabs.add(ws.splitId);
      if (!app.frames.has(ws.splitId)) createFrame(sp);
      nextSplitId = ws.splitId;
    }
  }
  // 关闭不在目标场景中的工具标签
  for (const existingId of [...app.tabs]) {
    if (existingId !== HOME && !targetTabs.has(existingId)) {
      dropTab(existingId);
    }
  }
  app.tabs = [...targetTabs];
  app.active = nextActive;
  app.splitId = nextSplitId;
  activate(app.active);
}

function restoreWorkspace(ws) {
  if (!ws || typeof ws !== 'object') return;
  if (Array.isArray(ws.favorites)) {
    app.favorites = ws.favorites.filter((id) => typeof id === 'string');
    S.saveList(localStorage, 'nuthusk.favorites', app.favorites);
  }
  if (Array.isArray(ws.savedWorkspaces)) {
    app.workspaces = ws.savedWorkspaces.map(S.normalizeWorkspace).filter(Boolean);
    S.saveWorkspaces(localStorage, app.workspaces);
  }
  if (Array.isArray(ws.workflows)) {
    app.workflows = ws.workflows.map(S.normalizeWorkflow).filter(Boolean);
    S.saveWorkflows(localStorage, app.workflows);
  }
  switchToWorkspaceLayout(ws);
  applyTheme();
}

function dropTab(id) {
  const p = isPage(id) ? null : byId(id);
  const r = S.closeTab(app.tabs, app.active, id);
  app.tabs = r.tabs;
  app.active = r.active;
  if (app.splitId === id || app.splitId === app.active) app.splitId = null;
  app.frames.get(id)?.remove();
  app.frames.delete(id);
  app.loadedFrames.delete(id);
  app.pendingInputs.delete(id);
  if (p && p.hasBackend && app.settings.stopOnClose) {
    invoke('stop_plugin_backend', { id }).then(() => syncPluginStates()).catch(() => {});
  }
}

function closeTab(id) {
  dropTab(id);
  activate(app.active);
}

function activate(id) {
  app.active = id;
  if (app.splitId === id) app.splitId = null;
  renderAll();
  if (!isPage(id)) {
    const frame = app.frames.get(id);
    frame?.focus();
    frame?.contentWindow?.postMessage({ type: 'focus' }, '*');
  }
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

  const wsGroupHead = el('div', { class: 'nav-group nav-group-row' },
    el('span', { textContent: '场景工作区' }),
    el('button', {
      class: 'nav-group-btn',
      type: 'button',
      title: '将当前打开的标签页与分屏保存为命名场景工作区',
      textContent: '+ 保存',
      onclick: () => actions.saveCurrentWorkspace(),
    }));

  const wsItems = app.workspaces.map((ws) => el('div', {
    class: 'nav-item nav-ws-item',
    role: 'button',
    tabIndex: 0,
    title: `切换到场景工作区：${ws.name}`,
    onclick: () => actions.switchWorkspace(ws),
    onkeydown: (e) => {
      if (e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) {
        e.preventDefault();
        actions.switchWorkspace(ws);
      }
    },
  },
    el('span', { class: 'nav-icon' }, icon('layers', 15)),
    el('span', { class: 'nav-label', textContent: ws.name }),
    el('button', {
      class: 'tab-close',
      type: 'button',
      title: '删除此工作区',
      textContent: '×',
      onclick: (e) => { e.stopPropagation(); actions.deleteWorkspace(ws.id); },
    })));

  const favCount = app.favorites.filter((id) => byId(id)).length;
  navEl.replaceChildren(
    item('all', '首页', 'home'),
    item('favorites', '收藏', 'star', favCount),
    item('recent', '最近使用', 'clock'),
    el('div', { class: 'nav-group', textContent: '分类' }),
    ...[...cats].sort(([a], [b]) => a.localeCompare(b, 'zh')).map(([c, n]) => item('cat:' + c, c, 'folder', n)),
    wsGroupHead,
    ...wsItems);

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

let draggingTabId = null;

function renderTabs() {
  const inSplit = !isPage(app.active) && app.splitId && app.splitId !== app.active;
  tabsEl.replaceChildren(...app.tabs.map((id) => {
    const page = PAGE_TABS[id];
    const p = page ? null : byId(id);
    const selected = id === app.active;
    const isSplitRight = inSplit && id === app.splitId;
    const canDrag = id !== HOME;
    return el('div', {
      class: 'tab' + (selected ? ' active' : '') + (isSplitRight ? ' split' : ''),
      role: 'tab',
      'aria-selected': String(selected),
      tabIndex: 0,
      draggable: canDrag,
      title: !page && !isPage(app.active) && id !== app.active ? '点击切换，按住 Shift 点击在右侧分屏并排打开' : '',
      onclick: (e) => {
        if (e.shiftKey && !page && !isPage(app.active) && id !== app.active) {
          setSplit(id);
          return;
        }
        activate(id);
      },
      onauxclick: (e) => { if (e.button === 1 && id !== HOME) { e.preventDefault(); closeTab(id); } },
      onkeydown: (e) => { if (e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); activate(id); } },
      ondragstart: canDrag ? (e) => {
        draggingTabId = id;
        e.currentTarget.classList.add('dragging');
        if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
      } : null,
      ondragend: canDrag ? (e) => {
        draggingTabId = null;
        e.currentTarget.classList.remove('dragging');
      } : null,
      ondragover: canDrag ? (e) => {
        if (!draggingTabId || draggingTabId === id) return;
        e.preventDefault();
        e.currentTarget.classList.add('drag-over');
      } : null,
      ondragleave: canDrag ? (e) => {
        e.currentTarget.classList.remove('drag-over');
      } : null,
      ondrop: canDrag ? (e) => {
        e.preventDefault();
        e.currentTarget.classList.remove('drag-over');
        if (!draggingTabId || draggingTabId === id) return;
        app.tabs = S.moveTab(app.tabs, draggingTabId, id);
        draggingTabId = null;
        renderTabs();
      } : null,
    },
      page ? el('span', { class: 'nav-icon' }, icon(page[0], 14)) : badge(p ?? { id, name: '?' }, true),
      el('span', { class: 'tab-title', textContent: page ? page[1] : (p?.name ?? id) }),
      id !== HOME ? el('button', {
        class: 'tab-close', type: 'button', title: '关闭（Ctrl W / 鼠标中键）', 'aria-label': '关闭',
        textContent: '×',
        onclick: (e) => { e.stopPropagation(); closeTab(id); },
      }) : null);
  }));
}

/** 工具页顶栏：图标、名称、说明、后端状态、开发重载、左右分屏控制、来源标识、收藏。 */
function renderToolhead() {
  const p = isPage(app.active) ? null : byId(app.active);
  toolheadEl.classList.toggle('hidden', !p);
  if (!p) return;
  const fav = app.favorites.includes(p.id);
  const splitPlugin = app.splitId && app.splitId !== p.id ? byId(app.splitId) : null;

  const splitControls = splitPlugin
    ? el('div', { class: 'th-split-box' },
        icon('columns', 14),
        el('span', { textContent: `右侧：${splitPlugin.name}` }),
        el('button', {
          class: 'btn sm', type: 'button', title: '交换左右分屏工具', textContent: '互换',
          onclick: () => swapSplit(),
        }),
        el('button', {
          class: 'btn sm', type: 'button', title: '关闭右侧分屏（Ctrl+\\）', textContent: '退出分屏',
          onclick: () => closeSplit(),
        }))
    : el('button', {
        class: 'btn sm', type: 'button', title: '在右侧并排打开另一个工具（Ctrl+\\，或 Shift+点击标签）',
        onclick: () => toggleSplitPicker(),
      }, icon('columns', 13), ' 左右分屏');

  toolheadEl.replaceChildren(
    badge(p),
    el('div', { class: 'th-text' },
      el('div', { class: 'th-name' }, p.name, el('span', { class: 'muted small', textContent: ` v${p.version}` })),
      el('div', { class: 'th-desc', textContent: p.description })),
    p.backendRunning ? el('span', {
      class: 'tag ok',
      title: p.backendPid ? `后端进程运行中（PID: ${p.backendPid}）` : '后端进程运行中',
      textContent: p.backendPid ? `运行中 · PID ${p.backendPid}` : '运行中',
    }) : null,
    p.backendRunning ? el('button', {
      class: 'btn sm', type: 'button', title: '结束当前插件的后台进程', textContent: '停止后端',
      onclick: () => actions.stopBackend(p.id),
    }) : null,
    el('button', {
      class: 'btn sm',
      type: 'button',
      title: '开发热重载：重新加载插件界面并重启后端进程（Ctrl+R）',
      onclick: () => actions.reloadPlugin(p.id),
    }, icon('refresh', 13), ' 重载'),
    splitControls,
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

  const inSplit = !isPage(app.active) && !!app.splitId && app.splitId !== app.active && isUsable(byId(app.splitId));
  stageEl.classList.toggle('split-mode', inSplit);

  for (const [id, frame] of app.frames) {
    frame.classList.toggle('active', id === app.active);
    frame.classList.toggle('split', inSplit && id === app.splitId);
  }
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
  const running = app.plugins.filter((p) => p.backendRunning).length;
  const cur = byId(app.active);
  statusEl.replaceChildren(
    el('span', {},
      `已加载 ${total} 个插件 · 可用 ${on}`,
      bad ? el('span', { class: 'st-warn', textContent: ` · ${bad} 个加载失败` }) : null,
      running ? el('span', { class: 'st-ok', textContent: ` · ${running} 个后端运行中 ` }) : null,
      running ? el('button', { class: 'st-btn', type: 'button', textContent: '全部停止', onclick: () => actions.stopAllBackends() }) : null),
    el('span', { class: 'st-right' },
      cur ? el('span', { textContent: `${cur.name} v${cur.version}` }) : null,
      cur ? el('span', { class: 'st-sep' }) : null,
      el('span', { textContent: `NutHusk${app.version ? ' v' + app.version : ''}` })));
}

function renderAll() {
  renderTopbar();
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

/** 轻量同步插件列表（主要用于更新 backendRunning / backendPid 状态，不打断管理页搜索框焦点）。 */
async function syncPluginStates() {
  try {
    app.plugins = await invoke('list_plugins');
    renderToolhead();
    renderStatus();
    if (app.active === MANAGE) renderManage(manageEl, app, actions);
  } catch { /* 忽略状态刷新异常 */ }
}

async function refresh() {
  app.plugins = await invoke('list_plugins');
  for (const id of [...app.tabs]) {
    if (!isPage(id) && !isUsable(byId(id))) dropTab(id); // 被禁用、卸载或失效的插件，关闭其标签
  }
  if (app.splitId && !isUsable(byId(app.splitId))) app.splitId = null;
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
  setSplit,
  closeSplit,
  swapSplit,

  toggleFavorite(id) {
    app.favorites = S.toggleItem(app.favorites, id);
    S.saveList(localStorage, 'nuthusk.favorites', app.favorites);
    renderAll();
  },

  clearRecent() {
    app.recent = [];
    try {
      localStorage.removeItem('nuthusk.recent');
      localStorage.removeItem('toolbox.recent');
    } catch { /* 忽略 */ }
    renderAll();
    toast('已清空最近使用记录');
  },

  async setEnabled(id, enabled) {
    try {
      await invoke('set_plugin_enabled', { id, enabled });
    } catch (err) {
      toast(String(err));
    }
    await refresh();
  },

  async stopBackend(id) {
    const p = byId(id);
    try {
      await invoke('stop_plugin_backend', { id });
      toast(`已停止后端：${p?.name ?? id}`);
    } catch (err) {
      toast(`停止后端失败：${err}`);
    }
    await syncPluginStates();
  },

  async stopAllBackends() {
    try {
      const count = await invoke('stop_all_backends');
      toast(count ? `已停止 ${count} 个后台进程` : '当前没有运行中的后端进程');
    } catch (err) {
      toast(`停止失败：${err}`);
    }
    await syncPluginStates();
  },

  /** 开发模式热重载：停止插件旧后端、重新读取 plugin.json 清单并刷新插件 iframe。 */
  async reloadPlugin(id) {
    try {
      app.plugins = await invoke('reload_single_plugin', { id });
      const p = byId(id);
      if (!p || !isUsable(p)) {
        await refresh();
        return;
      }
      const frame = app.frames.get(id);
      if (frame && p.ui) {
        app.loadedFrames.delete(id);
        frame.src = `${pluginUrl(p.id, p.ui)}?t=${Date.now()}`;
      }
      renderAll();
      toast(`已重载插件：${p.name}`);
    } catch (err) {
      toast(`重载插件失败：${err}`);
    }
  },

  async saveCurrentWorkspace() {
    const toolTabs = app.tabs.filter((id) => !isPage(id));
    if (toolTabs.length === 0) {
      toast('请先打开至少一个工具标签，再保存为场景工作区');
      return;
    }
    await promptSaveWorkspace({
      toolCount: toolTabs.length,
      hasSplit: !!app.splitId,
      onSave: (name) => {
        const ws = {
          id: `ws-${Date.now()}`,
          name,
          tabs: [HOME, ...toolTabs],
          active: isPage(app.active) ? toolTabs[0] : app.active,
          splitId: app.splitId,
        };
        app.workspaces = S.upsertWorkspace(app.workspaces, ws);
        S.saveWorkspaces(localStorage, app.workspaces);
        renderSidebar();
        toast(`已保存场景工作区：${name}`);
      },
    });
  },

  switchWorkspace(ws) {
    switchToWorkspaceLayout(ws);
    toast(`已切换到工作区：${ws.name}`);
  },

  deleteWorkspace(id) {
    app.workspaces = S.removeWorkspace(app.workspaces, id);
    S.saveWorkspaces(localStorage, app.workspaces);
    renderSidebar();
    toast('已删除工作区');
  },

  createWorkflow() {
    openWorkflowBuilder(null, {
      plugins: app.plugins,
      onSave: (wf) => {
        app.workflows = S.upsertWorkflow(app.workflows, wf);
        S.saveWorkflows(localStorage, app.workflows);
        renderAll();
        toast(`已保存流水线：${wf.name}`);
      },
    });
  },

  editWorkflow(wf) {
    openWorkflowBuilder(wf, {
      plugins: app.plugins,
      onSave: (updated) => {
        app.workflows = S.upsertWorkflow(app.workflows, updated);
        S.saveWorkflows(localStorage, app.workflows);
        renderAll();
        toast(`已更新流水线：${updated.name}`);
      },
    });
  },

  deleteWorkflow(id) {
    app.workflows = S.removeWorkflow(app.workflows, id);
    S.saveWorkflows(localStorage, app.workflows);
    renderAll();
    toast('已删除流水线');
  },

  runWorkflow(wf, initialInput = '') {
    openWorkflowRunner(wf, initialInput, {
      plugins: app.plugins,
      executeAction: executePluginAction,
      onSendTo: (text, wfName) => {
        showPalette({
          payload: { text, sourceId: wf.id, sourceName: wfName },
          onOpen: (targetId, pl) => {
            closeActiveDialog(false);
            openPlugin(targetId, pl);
          },
          onSendToSplit: (targetId, pl) => {
            closeActiveDialog(false);
            deliverInputToPlugin(targetId, pl);
            toast(`已投递到右侧分屏：${byId(targetId)?.name ?? targetId}`);
          },
          onRunWorkflow: (nextWf, nextText) => {
            closeActiveDialog(false);
            actions.runWorkflow(nextWf, nextText);
          },
        });
      },
    });
  },

  async togglePin() {
    const next = !app.settings.alwaysOnTop;
    try {
      await invoke('set_always_on_top', { onTop: next });
      app.settings = { ...app.settings, alwaysOnTop: next };
      renderAll();
      toast(next ? '已开启窗口置顶' : '已取消窗口置顶');
    } catch (err) {
      toast(`切换窗口置顶失败：${err}`);
    }
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

  async exportProfile() {
    const dialog = window.__TAURI__.dialog;
    let path = null;
    if (dialog?.save) {
      try {
        path = await dialog.save({
          defaultPath: 'nuthusk-workspace.nutprofile',
          filters: [{ name: 'NutHusk 配置清单', extensions: ['nutprofile', 'json'] }],
        });
      } catch (err) {
        return toast(`无法打开保存文件框：${err}`);
      }
      if (!path) return;
    }
    const workspace = {
      favorites: app.favorites,
      tabs: app.tabs.filter((id) => id === HOME || !isPage(id)),
      active: isPage(app.active) ? HOME : app.active,
      splitId: app.splitId,
      savedWorkspaces: app.workspaces,
      workflows: app.workflows,
    };
    try {
      await invoke('export_profile', { path, name: 'NutHusk 工作台配置', workspace });
      toast(path ? `已导出配置清单：${path}` : '已生成配置清单');
    } catch (err) {
      toast(`导出配置清单失败：${err}`);
    }
  },

  async importProfile(rawProfile = null) {
    let profileData = rawProfile;
    if (!profileData) {
      const dialog = window.__TAURI__.dialog;
      if (!dialog?.open) return toast('当前环境不支持选择文件，请把 .nutprofile 清单文件直接拖进窗口');
      let picked;
      try {
        picked = await dialog.open({
          multiple: false,
          filters: [{ name: 'NutHusk 配置清单', extensions: ['nutprofile', 'json'] }],
        });
      } catch (err) {
        return toast(`无法打开文件选择框：${err}`);
      }
      const path = Array.isArray(picked) ? picked[0] : picked;
      if (!path) return;
      try {
        profileData = await invoke('read_profile_file', { path });
      } catch (err) {
        return toast(`读取配置清单失败：${err}`);
      }
    }
    await applyProfile(profileData, {
      app,
      invoke,
      refresh,
      importPackages,
      restoreWorkspace,
    });
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

  ...createDownloadActions({
    state: app.downloadState,
    invoke,
    render: () => { if (app.active === DOWNLOAD) renderAll(); },
    importPackages,
    refresh,
  }),
};

/* ---------- 快捷键与投递面板 ---------- */

function showPalette(extra = {}) {
  openPalette({
    plugins: app.plugins,
    favorites: app.favorites,
    recent: app.recent,
    workflows: app.workflows,
    workspaces: app.workspaces,
    activeId: app.active,
    splitId: app.splitId,
    onOpen: openPlugin,
    onRunWorkflow: (wf, text) => actions.runWorkflow(wf, text),
    onSwitchWorkspace: (ws) => actions.switchWorkspace(ws),
    onSendToSplit: (targetId, payload) => {
      deliverInputToPlugin(targetId, payload);
      toast(`已投递到右侧分屏：${byId(targetId)?.name ?? targetId}`);
    },
    ...extra,
  });
}

let lastSendTo = -Infinity;
function handleSendTo(sourceId, text, targetId = null) {
  const sourceFrame = app.frames.get(sourceId);
  if (!sourceFrame || !(sourceFrame.classList.contains('active') || sourceFrame.classList.contains('split'))) return;
  if (isDialogOpen() || document.querySelector('.pal-overlay')) return;
  const now = performance.now();
  if (now - lastSendTo < 500) return; // Shared limit: switching source frames cannot bypass it.
  if (targetId !== null && (typeof targetId !== 'string' || !isUsable(byId(targetId)))) return;
  const s = String(text ?? '');
  if (!s.trim()) return toast('没有可投递的内容');
  lastSendTo = now;
  const srcPlugin = byId(sourceId);
  const payload = { text: s, sourceId, sourceName: srcPlugin?.name ?? sourceId };
  if (targetId) {
    if (app.splitId === targetId) {
      deliverInputToPlugin(targetId, payload);
      toast(`已投递到右侧分屏：${byId(targetId)?.name ?? targetId}`);
    } else {
      openPlugin(targetId, payload);
    }
    return;
  }
  showPalette({ payload });
}

function handleShortcut(e) {
  if (isDialogOpen()) return false; // 对话框打开时不响应全局快捷键
  if (!(e.ctrlKey || e.metaKey)) return false;
  const k = String(e.key ?? '').toLowerCase();
  if (k === 'k') { showPalette(); return true; }
  if (k === '\\') { toggleSplitPicker(); return true; }
  if (k === 'r') {
    if (!isPage(app.active)) {
      actions.reloadPlugin(app.active);
      return true;
    }
    return false;
  }
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
$('open-palette').addEventListener('click', () => showPalette());
$('theme-toggle').addEventListener('click', () => actions.patchSettings({ theme: effectiveTheme() === 'dark' ? 'light' : 'dark' }));
$('pin-toggle')?.addEventListener('click', () => actions.togglePin());
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
    await syncPluginStates();
    return true;
  } catch (retryErr) {
    frame.contentWindow?.postMessage({ reqId: reqData.reqId, error: errorMessage(retryErr) }, '*');
    await syncPluginStates();
    return true;
  }
}

/* ---------- 全局事件监听 ---------- */

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

window.__TAURI__.event?.listen?.('backends-changed', () => {
  syncPluginStates();
});

/* ---------- 桥接：插件界面 → 壳 → Rust → 插件后端 ---------- */

window.addEventListener('message', async (e) => {
  const entry = [...app.frames].find(([, f]) => f.contentWindow === e.source);
  if (!entry) return; // 只接受来自已打开的插件 iframe 的消息
  const [id, frame] = entry;
  const d = e.data ?? {};

  if (d.type === 'key') {
    if (frame.classList.contains('active') || frame.classList.contains('split')) handleShortcut(d);
    return;
  } // SDK 转发的快捷键（焦点在插件内时）
  if (d.type === 'sendTo') { handleSendTo(id, d.text, d.targetId ?? null); return; } // 插件投递数据到其他工具
  if (d.type === 'actionResult') {
    const pending = app.pendingActions.get(d.reqId);
    if (pending && pending.pluginId === id) {
      app.pendingActions.delete(d.reqId);
      clearTimeout(pending.timer);
      if (d.error) pending.reject(new Error(d.error));
      else pending.resolve(d.result);
    }
    return;
  }
  if (d.type !== 'call') return;

  const wasRunning = !!byId(id)?.backendRunning;
  try {
    const result = await invoke('plugin_call', { id, method: String(d.method), params: d.params ?? null });
    frame.contentWindow?.postMessage({ reqId: d.reqId, result }, '*');
    if (!wasRunning) await syncPluginStates();
  } catch (err) {
    const errStr = errorMessage(err);
    if (err?.code === 'RUNTIME_MISSING' && err.detail && typeof err.detail === 'object') {
      const handled = await handleRuntimeMissing(id, d, frame, err.detail);
      if (handled) return;
    }
    frame.contentWindow?.postMessage({ reqId: d.reqId, error: errStr }, '*');
    if (!wasRunning) await syncPluginStates();
  }
});

/* ---------- 拖入插件包、配置清单或可处理的文件 ----------
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
  importDroppedFiles([...e.dataTransfer.files], {
    invoke,
    refresh,
    plugins: app.plugins,
    onImportProfile: (text) => actions.importProfile(text),
    onOpenFile: (payload, matched) => {
      if (matched.length === 1) {
        openPlugin(matched[0].id, payload);
        toast(`已用「${matched[0].name}」打开 ${payload.fileName}`);
      } else {
        showPalette({ payload: { ...payload, sourceName: payload.fileName } });
      }
    },
  });
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
