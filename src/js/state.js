// 纯函数：不依赖 DOM，方便单元测试。

export const HOME = 'home';
export const DOWNLOAD = 'download';
export const MANAGE = 'manage';
export const SETTINGS = 'settings';

/** 设置的默认值（与 Rust 侧一致；真正的值从 get_settings 读取）。 */
export const DEFAULT_SETTINGS = {
  theme: 'system',
  showRecent: true,
  showFavorites: true,
  stopOnClose: true,
  idleTimeoutMinutes: 5,
  closeToTray: false,
  alwaysOnTop: false,
  dirOrder: ['appdata', 'program'],
};
export const MAX_RECENT = 8;

export function pushRecent(list, id, max = MAX_RECENT) {
  return [id, ...list.filter((x) => x !== id)].slice(0, max);
}

export function toggleItem(list, id) {
  return list.includes(id) ? list.filter((x) => x !== id) : [...list, id];
}

export function loadList(storage, key) {
  try {
    const v = JSON.parse(storage.getItem(key) ?? '[]');
    return Array.isArray(v) ? v.filter((x) => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

/** 读取列表；新键没有数据时回退到旧键（改名后保留用户的收藏和最近使用）。 */
export function loadListWithFallback(storage, key, oldKey) {
  const v = loadList(storage, key);
  return v.length ? v : loadList(storage, oldKey);
}

export function saveList(storage, key, list) {
  try {
    storage.setItem(key, JSON.stringify(list));
  } catch {
    /* 存储不可用时静默忽略，只是不记忆 */
  }
}

export function openTab(tabs, id) {
  return tabs.includes(id) ? tabs : [...tabs, id];
}

/** 关闭标签。首页标签不可关闭；关闭当前标签后切到右侧邻居，没有则切到左侧。 */
export function closeTab(tabs, active, id) {
  const i = tabs.indexOf(id);
  if (id === HOME || i < 0) return { tabs, active };
  const next = tabs.filter((t) => t !== id);
  if (active !== id) return { tabs: next, active };
  return { tabs: next, active: next[Math.min(i, next.length - 1)] };
}

/** 拖拽调整标签顺序。首页标签固定在第一位，不参与移动。 */
export function moveTab(tabs, fromId, toId) {
  if (!fromId || !toId || fromId === toId || fromId === HOME || toId === HOME) return tabs;
  const fromIdx = tabs.indexOf(fromId);
  const toIdx = tabs.indexOf(toId);
  if (fromIdx <= 0 || toIdx <= 0) return tabs;
  const next = tabs.filter((t) => t !== fromId);
  next.splice(toIdx, 0, fromId);
  return next;
}

/** 循环切换标签。 */
export function cycleTab(tabs, active, dir) {
  const i = tabs.indexOf(active);
  return tabs[(i + dir + tabs.length) % tabs.length];
}

export function score(p, q) {
  const name = p.name.toLowerCase();
  const keys = (p.keywords ?? []).map((k) => k.toLowerCase());
  if (name === q) return 100;
  if (name.startsWith(q)) return 90;
  if (name.includes(q)) return 70;
  if (keys.some((k) => k.startsWith(q))) return 60;
  if (keys.some((k) => k.includes(q))) return 50;
  if ((p.category ?? '').toLowerCase().includes(q)) return 30;
  if ((p.description ?? '').toLowerCase().includes(q)) return 20;
  return 0;
}

/** 搜索可用（已启用且无错误）的插件。空查询时按"最近 → 收藏 → 其他"排序。 */
export function searchPlugins(plugins, query, { favorites = [], recent = [] } = {}) {
  const q = query.trim().toLowerCase();
  const usable = plugins.filter((p) => p.enabled && !p.error);
  const byName = (a, b) => a.name.localeCompare(b.name, 'zh');

  if (!q) {
    const rank = (p) => {
      const r = recent.indexOf(p.id);
      if (r >= 0) return r;
      const f = favorites.indexOf(p.id);
      return f >= 0 ? 100 + f : 1000;
    };
    return [...usable].sort((a, b) => rank(a) - rank(b) || byName(a, b));
  }

  return usable
    .map((p) => ({ p, s: score(p, q) }))
    .filter((x) => x.s > 0)
    .map((x) => ({ ...x, s: x.s + (favorites.includes(x.p.id) ? 5 : 0) + (recent.includes(x.p.id) ? 3 : 0) }))
    .sort((a, b) => b.s - a.s || byName(a.p, b.p))
    .map((x) => x.p);
}

function looksLikeJson(text) {
  const t = text.trim();
  if (t.length < 2) return false;
  if (!((t.startsWith('{') && t.endsWith('}')) || (t.startsWith('[') && t.endsWith(']')))) return false;
  try {
    JSON.parse(t);
    return true;
  } catch {
    return false;
  }
}

/**
 * 根据输入文本智能匹配可处理该内容的插件。
 * 返回 [{ plugin, label, specific, score }]，按精确匹配优先排序。
 */
export function matchPluginsForText(plugins, rawText) {
  const text = String(rawText ?? '');
  const trimmed = text.trim();
  if (!trimmed) return [];

  const isJson = looksLikeJson(trimmed);
  const usable = plugins.filter((p) => p.enabled && !p.error && p.ui);
  const out = [];

  for (const p of usable) {
    const m = p.match;
    let matched = false;
    let specific = false;
    let label = m?.label || '处理此内容';

    if (m) {
      if (m.minLength != null && text.length < m.minLength) continue;
      if (m.maxLength != null && text.length > m.maxLength) continue;

      const hasPrefixes = Array.isArray(m.prefixes) && m.prefixes.length > 0;
      const hasRegex = Array.isArray(m.regex) && m.regex.length > 0;

      if (hasPrefixes && m.prefixes.some((pfx) => trimmed.toLowerCase().startsWith(String(pfx).toLowerCase()))) {
        matched = true;
        specific = true;
      }
      if (!matched && hasRegex) {
        for (const pat of m.regex) {
          try {
            if (new RegExp(pat).test(trimmed)) {
              matched = true;
              specific = true;
              break;
            }
          } catch { /* 忽略无效正则 */ }
        }
      }
      if (!hasPrefixes && !hasRegex && (m.minLength != null || m.maxLength != null)) {
        matched = true;
        specific = false;
      }
    }

    // 内置兜底启发式：若文本是合法 JSON 且插件含 json 关键词（兼容未声明 match 的旧插件）
    if (!specific && isJson) {
      const kws = [p.id, ...(p.keywords ?? [])].map((s) => String(s).toLowerCase());
      if (kws.some((k) => k.includes('json'))) {
        matched = true;
        specific = true;
        label = m?.label || '识别为 JSON 文本';
      }
    }

    if (matched) {
      out.push({
        plugin: p,
        label,
        specific,
        score: specific ? 100 : 20,
      });
    }
  }

  return out.sort((a, b) => b.score - a.score || a.plugin.name.localeCompare(b.plugin.name, 'zh'));
}

/** 根据文件名匹配声明了对应 extensions 的插件。 */
export function matchPluginsForFile(plugins, fileName) {
  const name = String(fileName ?? '').trim();
  const dot = name.lastIndexOf('.');
  if (dot < 0 || dot === name.length - 1) return [];
  const ext = name.slice(dot + 1).toLowerCase();
  return plugins.filter((p) =>
    p.enabled && !p.error && p.ui &&
    Array.isArray(p.match?.extensions) &&
    p.match.extensions.some((x) => String(x).toLowerCase() === ext)
  );
}

/** 插件管理页的筛选：status 取 all / enabled / disabled / error / running。 */
export function filterPlugins(plugins, query = '', status = 'all') {
  const q = query.trim().toLowerCase();
  return plugins.filter((p) => {
    if (status === 'enabled' && !(p.enabled && !p.error)) return false;
    if (status === 'disabled' && p.enabled) return false;
    if (status === 'error' && !p.error) return false;
    if (status === 'running' && !p.backendRunning) return false;
    if (!q) return true;
    return [p.name, p.id, p.description, ...(p.keywords ?? [])].some((s) => String(s ?? '').toLowerCase().includes(q));
  });
}
