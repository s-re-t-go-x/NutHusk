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

/** 插件管理页的筛选：status 取 all / enabled / disabled / error。 */
export function filterPlugins(plugins, query = '', status = 'all') {
  const q = query.trim().toLowerCase();
  return plugins.filter((p) => {
    if (status === 'enabled' && !(p.enabled && !p.error)) return false;
    if (status === 'disabled' && p.enabled) return false;
    if (status === 'error' && !p.error) return false;
    if (!q) return true;
    return [p.name, p.id, p.description, ...(p.keywords ?? [])].some((s) => String(s ?? '').toLowerCase().includes(q));
  });
}
