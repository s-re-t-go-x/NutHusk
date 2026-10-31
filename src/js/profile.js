import { el, toast } from './util.js';
import { showDialog } from './dialog.js';

export const PROFILE_FORMAT = 'nuthusk-profile';
const MAX_PROFILE_BYTES = 2 * 1024 * 1024;

/**
 * 校验并解析 `.nutprofile` JSON 文本或对象。
 */
export function parseProfile(raw) {
  let obj = raw;
  if (typeof raw === 'string') {
    if (new TextEncoder().encode(raw).length > MAX_PROFILE_BYTES) throw new Error('配置清单文件过大（上限 2 MB）');
    try {
      obj = JSON.parse(raw);
    } catch (err) {
      throw new Error(`不是有效的 JSON 格式：${err.message}`);
    }
  }
  if (!obj || typeof obj !== 'object' || Array.isArray(obj)) {
    throw new Error('配置清单内容无效');
  }
  if (obj.format !== PROFILE_FORMAT) {
    throw new Error(`不是有效的 NutHusk 配置清单（format 应为 ${PROFILE_FORMAT}）`);
  }
  if (obj.version !== 1) {
    throw new Error(`不支持的配置清单版本 v${obj.version}（当前支持 v1）`);
  }
  for (const [key, limit] of [['sources', 50], ['plugins', 500], ['runtimes', 50]]) {
    if (Array.isArray(obj[key]) && obj[key].length > limit) throw new Error('配置清单中的条目数量过多');
  }
  return {
    format: PROFILE_FORMAT,
    version: 1,
    name: typeof obj.name === 'string' ? obj.name : null,
    settings: obj.settings && typeof obj.settings === 'object' ? obj.settings : {},
    sources: Array.isArray(obj.sources) ? obj.sources.filter((s) => s && typeof s.url === 'string') : [],
    runtimes: Array.isArray(obj.runtimes) ? obj.runtimes.filter((r) => typeof r === 'string') : [],
    plugins: Array.isArray(obj.plugins) ? obj.plugins.filter((p) => p && typeof p.id === 'string') : [],
    workspace: obj.workspace && typeof obj.workspace === 'object' ? obj.workspace : null,
  };
}

/**
 * 对比当前环境与待导入的配置清单，计算需要新增的插件源、待下载的运行时、待下载安装的插件与状态变更。
 */
export function diffProfile(profile, { plugins = [], runtimes = [], sources = [] }) {
  const existingSourceUrls = new Set(sources.map((s) => s.url));
  const newSources = profile.sources.filter((s) => !existingSourceUrls.has(s.url));

  const installedRuntimeNames = new Set(
    runtimes.filter((r) => r.installed || r.onSystem).map((r) => r.name)
  );
  const missingRuntimes = profile.runtimes.filter((name) => !installedRuntimeNames.has(name));

  const installedPluginMap = new Map(plugins.map((p) => [p.id, p]));
  const missingPlugins = [];
  const stateUpdates = [];

  for (const want of profile.plugins) {
    const cur = installedPluginMap.get(want.id);
    if (!cur) {
      missingPlugins.push(want);
    } else if (typeof want.enabled === 'boolean' && cur.enabled !== want.enabled) {
      stateUpdates.push({ id: want.id, name: cur.name || want.name || want.id, enabled: want.enabled });
    }
  }

  return {
    newSources,
    missingRuntimes,
    missingPlugins,
    stateUpdates,
    hasSettings: Object.keys(profile.settings || {}).length > 0,
    hasWorkspace: !!profile.workspace,
  };
}

function buildProfileConfirmBody(profile, diff) {
  const parts = [];
  if (profile.name) {
    parts.push(el('div', { class: 'dlg-head' }, el('b', { textContent: profile.name })));
  }
  parts.push(el('p', {
    textContent: '将根据此配置加载清单自动同步你的工作台。缺失的插件和运行环境将从对应的源自动下载，插件包在安装前仍会逐一弹出安全确认框。',
  }));

  const summaryItems = [];
  if (diff.hasSettings) {
    summaryItems.push('应用外观、生命周期与目录优先级设置');
  }
  if (diff.newSources.length) {
    summaryItems.push(`新增 ${diff.newSources.length} 个插件源：${diff.newSources.map((s) => `${s.name || '未命名源'}（${s.url}，由本次配置清单添加）`).join('、')}`);
  }
  if (diff.missingRuntimes.length) {
    summaryItems.push(`自动下载并配置 ${diff.missingRuntimes.length} 个便携运行环境：${diff.missingRuntimes.join('、')}`);
  }
  if (diff.missingPlugins.length) {
    summaryItems.push(`从插件源查找并下载 ${diff.missingPlugins.length} 个缺失插件：${diff.missingPlugins.map((p) => p.name || p.id).join('、')}`);
  }
  if (diff.stateUpdates.length) {
    summaryItems.push(`同步 ${diff.stateUpdates.length} 个已安装插件的启用/禁用状态`);
  }
  if (diff.hasWorkspace) {
    summaryItems.push('恢复收藏列表与打开的工具标签页');
  }

  if (!summaryItems.length) {
    parts.push(el('div', { class: 'dlg-box ok' }, '当前环境已包含清单中的所有插件与运行环境，无需额外下载。'));
  } else {
    parts.push(el('div', { class: 'dlg-box info' },
      el('div', { class: 'box-title', textContent: '即将执行的同步操作：' }),
      ...summaryItems.map((line) => el('div', { class: 'small', textContent: `• ${line}` }))));
  }

  return el('div', {}, ...parts);
}

/**
 * 执行配置清单导入流程：
 * 1. 解析并对比当前状态，弹窗展示变更摘要供用户确认；
 * 2. 应用软件设置与目录顺序；
 * 3. 添加缺失的插件源；
 * 4. 下载并安装缺失的便携运行时；
 * 5. 从所有插件源索引（或插件条目自带的 url）查找缺失插件，下载到临时目录后调用 `importPackages` 走标准安全确认与安装流程；
 * 6. 同步插件启用/禁用状态，恢复收藏、打开的标签页与分屏状态。
 */
export async function applyProfile(rawProfile, { app, invoke, refresh, importPackages, restoreWorkspace }) {
  let profile;
  try {
    profile = parseProfile(rawProfile);
  } catch (err) {
    toast(`配置清单无效：${err.message || err}`);
    return false;
  }

  let sources = [];
  let runtimes = app.runtimes ?? [];
  try {
    sources = await invoke('market_sources');
  } catch { /* 忽略 */ }
  if (!runtimes.length) {
    try { runtimes = await invoke('runtime_status'); } catch { /* 忽略 */ }
  }

  const diff = diffProfile(profile, { plugins: app.plugins, runtimes, sources });
  const ok = await showDialog({
    title: '导入配置清单（.nutprofile）',
    body: buildProfileConfirmBody(profile, diff),
    buttons: [
      { label: '取消', value: false },
      { label: '开始同步与恢复', value: true, primary: true },
    ],
  });
  if (!ok) return false;
  const failures = [];

  // 1. 恢复设置与目录顺序
  if (diff.hasSettings) {
    const { dirOrder, ...restSettings } = profile.settings;
    if (Object.keys(restSettings).length > 0) {
      try {
        app.settings = { ...app.settings, ...(await invoke('patch_settings', { patch: restSettings })) };
      } catch (err) {
        failures.push('设置');
        toast(`部分设置恢复失败：${err}`);
      }
    }
    if (Array.isArray(dirOrder) && dirOrder.length > 0) {
      try {
        app.dirs = await invoke('set_dir_order', { order: dirOrder });
        app.settings = { ...app.settings, dirOrder };
      } catch { failures.push('目录顺序'); }
    }
  }

  // 2. 添加缺失的插件源
  for (const src of diff.newSources) {
    try {
      await invoke('source_add', { url: src.url, name: src.name || null });
    } catch (err) {
      failures.push(`插件源 ${src.name || src.url}`);
      toast(`添加插件源「${src.name || src.url}」失败：${err}`);
    }
  }

  // 3. 下载并配置缺失的运行环境
  for (const rtName of diff.missingRuntimes) {
    toast(`正在根据配置清单下载运行环境：${rtName}…`);
    try {
      await invoke('runtime_install', { name: rtName });
      toast(`运行环境 ${rtName} 安装完成`);
    } catch (err) {
      failures.push(`运行环境 ${rtName}`);
      toast(`运行环境 ${rtName} 安装失败：${err}`);
    }
  }

  // 4. 查找并下载缺失的插件包
  if (diff.missingPlugins.length > 0) {
    const allSources = await invoke('market_sources').catch(() => []);
    allSources.sort((a, b) => Number(!!b.official) - Number(!!a.official));
    const addedSourceUrls = new Set(diff.newSources.map((s) => s.url));
    const indexMap = new Map(); // pluginId -> [{ entry, source }]
    for (const s of allSources) {
      try {
        const res = await invoke('market_index', { sourceId: s.id });
        const entries = res?.index?.entries ?? res?.index?.plugins ?? res?.entries ?? res?.plugins ?? [];
        for (const e of entries) {
          if (e?.id) {
            const candidates = indexMap.get(e.id) ?? [];
            candidates.push({ entry: e, source: s });
            indexMap.set(e.id, candidates);
          }
        }
      } catch { /* 某个源拉取失败时继续尝试其他源 */ }
    }

    const downloadedItems = [];
    const notFound = [];

    for (const want of diff.missingPlugins) {
      const candidates = indexMap.get(want.id) ?? [];
      const found = candidates.find(({ entry }) => !want.version || entry.version === want.version);
      const url = want.url || found?.entry?.url;
      const sha256 = want.url ? (want.sha256 || null) : (found?.entry?.sha256 || null);
      const source = want.url ? null : found?.source;
      const sourceId = source?.id ?? null;

      if (!url) {
        notFound.push(`${want.name || want.id}${want.version ? `（需要版本 ${want.version}，未选择其他版本）` : ''}`);
        continue;
      }

      if (!source?.official) {
        const confirmed = await showDialog({
          title: '确认配置清单的插件下载来源',
          body: el('div', {},
            el('p', { textContent: `插件：${want.name || want.id}${want.version ? ` / ${want.version}` : ''}` }),
            el('p', { textContent: source
              ? `私有插件源：${source.name || source.id}（${addedSourceUrls.has(source.url) ? '此源由本次配置清单添加' : '已配置的插件源'}）`
              : '此下载地址由配置清单直接指定，不代表官方插件源。' }),
            source ? el('p', { style: 'overflow-wrap: anywhere;', textContent: `插件源完整 URL：${source.url}` }) : null,
            el('p', { style: 'overflow-wrap: anywhere;', textContent: `插件包完整 URL：${url}` }),
            el('p', { textContent: '仅在信任此来源时继续。下载后仍需确认插件包权限与签名。' })),
          buttons: [{ label: '跳过此插件', value: false }, { label: '确认来源并下载', value: true, primary: true }],
        });
        if (!confirmed) {
          failures.push(`跳过插件 ${want.name || want.id}`);
          continue;
        }
      }

      toast(`正在下载插件：${want.name || want.id}…`);
      let tmpPath;
      try {
        tmpPath = await invoke('market_download', { url, sha256, sourceId });
        const info = await invoke('inspect_plugin_package', { path: tmpPath });
        if (info.manifest.id !== want.id || (want.version && info.manifest.version !== want.version)) {
          throw new Error(`下载包的插件 ID 或版本与配置清单不符（需要 ${want.id}${want.version ? ` / ${want.version}` : ''}）`);
        }
        downloadedItems.push({ path: tmpPath, name: `${want.name || want.id}.nut`, temp: true });
      } catch (err) {
        failures.push(`插件 ${want.name || want.id}`);
        if (tmpPath) await invoke('discard_dropped_package', { path: tmpPath }).catch(() => {});
        toast(`下载插件「${want.name || want.id}」失败：${err}`);
      }
    }

    if (notFound.length) {
      toast(`未能从当前插件源找到：${notFound.join('、')}`);
    }
    if (downloadedItems.length) {
      await importPackages(downloadedItems, { invoke, refresh });
    }
  }

  // 5. 同步插件启用/禁用状态（包括刚刚新安装的插件若在清单中被标记为 enabled: false）
  await refresh();
  const currentMap = new Map(app.plugins.map((p) => [p.id, p]));
  for (const want of profile.plugins) {
    const cur = currentMap.get(want.id);
    if (cur && typeof want.enabled === 'boolean' && cur.enabled !== want.enabled) {
      try {
        await invoke('set_plugin_enabled', { id: want.id, enabled: want.enabled });
      } catch { failures.push(`插件状态 ${want.name || want.id}`); }
    }
  }

  // 6. 恢复收藏与工作区标签
  if (profile.workspace && restoreWorkspace) {
    restoreWorkspace(profile.workspace);
  }

  await refresh();
  const incomplete = profile.plugins.flatMap((want) => {
    const cur = app.plugins.find((p) => p.id === want.id);
    if (!cur) return [`${want.name || want.id}（未安装）`];
    if (want.version && cur.version !== want.version) {
      return [`${want.name || want.id}（需要 ${want.version}，当前 ${cur.version}；保留当前版本）`];
    }
    return [];
  });
  const issues = [...failures, ...incomplete];
  toast(issues.length
    ? `配置清单部分同步，未完成：${issues.join('、')}`
    : '已根据配置清单完成同步');
  return issues.length === 0;
}
