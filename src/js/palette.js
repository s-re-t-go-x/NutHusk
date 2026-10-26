import { el, badge } from './util.js';
import { searchPlugins, matchPluginsForText } from './state.js';

let current = null;

export function isPaletteOpen() {
  return current !== null;
}

export function closePalette() {
  current?.close();
}

function previewText(s, max = 48) {
  const oneLine = String(s ?? '').replace(/\s+/g, ' ').trim();
  return oneLine.length > max ? oneLine.slice(0, max) + '…' : oneLine;
}

/**
 * 命令面板（Ctrl/Cmd+K）：
 * - 普通模式：搜索工具名称/关键词，粘贴或输入文本时自动嗅探匹配的插件并支持「带内容打开」
 * - 投递模式（传入 payload: { text, sourceId, sourceName }）：把当前工具的内容投递给另一个工具或右侧分屏
 * - 分屏选择模式（mode: 'split'）：选择要在右侧并排打开的工具
 */
export function openPalette({
  plugins,
  favorites = [],
  recent = [],
  onOpen,
  payload = null,
  mode = 'search',
  activeId = null,
  splitId = null,
  onSendToSplit = null,
}) {
  if (current) closePalette();

  const prevFocus = document.activeElement;
  let items = []; // [{ plugin, payload, hint, onSelect }]
  let rows = [];
  let index = 0;

  const placeholder = payload
    ? `选择接收「${previewText(payload.text, 24)}」的工具…`
    : mode === 'split'
      ? '选择要在右侧分屏并排打开的工具…'
      : '搜索工具，或直接粘贴文本智能匹配工具…';

  const banner = payload
    ? el('div', { class: 'pal-banner' },
        el('span', { class: 'tag ok', textContent: payload.sourceName ? `来自：${payload.sourceName}` : '投递内容' }),
        el('span', { class: 'pal-banner-text', textContent: `${previewText(payload.text, 60)}（${payload.text.length} 字符）` }))
    : null;

  const input = el('input', {
    class: 'pal-input',
    type: 'text',
    placeholder,
    spellcheck: false,
    'aria-label': '搜索工具',
  });
  const list = el('div', { class: 'pal-list', role: 'listbox' });
  const panel = el('div', { class: 'pal-panel', role: 'dialog', 'aria-modal': 'true', 'aria-label': '搜索工具' }, banner, input, list);
  const overlay = el('div', { class: 'pal-overlay', onmousedown: (e) => { if (e.target === overlay) close(); } }, panel);

  function close() {
    overlay.remove();
    current = null;
    prevFocus?.focus?.();
  }

  function choose(item) {
    if (!item) return;
    close();
    if (item.onSelect) item.onSelect();
    else onOpen(item.plugin.id, item.payload ?? null);
  }

  function select(i) {
    if (!rows.length) return;
    index = Math.max(0, Math.min(rows.length - 1, i));
    rows.forEach((r, n) => r.classList.toggle('sel', n === index));
    rows[index].scrollIntoView?.({ block: 'nearest' });
  }

  function buildItems(rawQuery) {
    const q = rawQuery.trim();

    // 1. 投递模式（Send To）
    if (payload) {
      const out = [];
      const splitPlugin = splitId ? plugins.find((p) => p.id === splitId && p.enabled && !p.error) : null;
      if (splitPlugin && splitPlugin.id !== payload.sourceId && onSendToSplit && (!q || searchPlugins([splitPlugin], q).length)) {
        out.push({
          plugin: splitPlugin,
          payload,
          hint: '⚡ 直接投递到右侧分屏',
          hintClass: 'tag ok',
          onSelect: () => onSendToSplit(splitPlugin.id, payload),
        });
      }
      const matchedMap = new Map(
        matchPluginsForText(plugins, payload.text).map((m) => [m.plugin.id, m])
      );
      const candidates = searchPlugins(plugins, q, { favorites, recent })
        .filter((p) => p.ui && p.id !== payload.sourceId);
      candidates.sort((a, b) => {
        const ma = matchedMap.get(a.id)?.score ?? 0;
        const mb = matchedMap.get(b.id)?.score ?? 0;
        return mb - ma;
      });
      for (const p of candidates) {
        const m = matchedMap.get(p.id);
        out.push({
          plugin: p,
          payload,
          hint: m?.specific ? `✨ ${m.label}` : '接收投递',
          hintClass: m?.specific ? 'tag ok' : 'tag',
        });
      }
      return out.slice(0, 30);
    }

    // 2. 分屏选择模式
    if (mode === 'split') {
      return searchPlugins(plugins, q, { favorites, recent })
        .filter((p) => p.ui && p.id !== activeId)
        .slice(0, 30)
        .map((p) => ({ plugin: p, payload: null, hint: '在右侧分屏打开', hintClass: 'tag' }));
    }

    // 3. 普通搜索 + 智能内容识别模式
    const nameMatches = searchPlugins(plugins, q, { favorites, recent }).slice(0, 30);
    if (!q) {
      return nameMatches.map((p) => ({ plugin: p, payload: null, hint: p.category, hintClass: 'tag' }));
    }

    const contentMatches = matchPluginsForText(plugins, rawQuery);
    const specificMatches = contentMatches.filter((m) => m.specific);
    const out = [];
    const seen = new Set();

    // 精确特征命中（如粘贴了一段 JSON）：置顶显示「✨ 识别为 JSON 文本 · 带内容打开」
    for (const m of specificMatches) {
      seen.add(m.plugin.id);
      out.push({
        plugin: m.plugin,
        payload: { text: rawQuery },
        hint: `✨ ${m.label} · 带内容打开`,
        hintClass: 'tag ok',
      });
    }

    for (const p of nameMatches) {
      if (seen.has(p.id)) continue;
      seen.add(p.id);
      out.push({ plugin: p, payload: null, hint: p.category, hintClass: 'tag' });
    }

    // 如果按名称没搜到任何插件，但输入是一段文本，则推荐所有可处理文本的插件带内容打开
    if (out.length === 0 && q.length >= 2) {
      for (const m of contentMatches) {
        if (seen.has(m.plugin.id)) continue;
        seen.add(m.plugin.id);
        out.push({
          plugin: m.plugin,
          payload: { text: rawQuery },
          hint: `✨ ${m.label} · 带内容打开`,
          hintClass: 'tag ok',
        });
      }
    }

    return out.slice(0, 30);
  }

  function update() {
    items = buildItems(input.value);
    rows = items.map((item, i) => el('div', {
      class: 'pal-row' + (i === 0 ? ' sel' : ''),
      role: 'option',
      onmousemove: () => { if (index !== i) select(i); },
      onclick: () => choose(item),
    },
      badge(item.plugin, true),
      el('div', { class: 'pal-main' },
        el('div', { class: 'pal-name', textContent: item.plugin.name }),
        el('div', { class: 'pal-desc', textContent: item.plugin.description })),
      el('span', { class: item.hintClass || 'tag', textContent: item.hint })));
    index = 0;
    list.replaceChildren(...(rows.length ? rows : [el('div', { class: 'pal-empty', textContent: '没有匹配的工具' })]));
  }

  input.addEventListener('input', update);
  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); select(index + 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); select(index - 1); }
    else if (e.key === 'Enter') { e.preventDefault(); choose(items[index]); }
    else if (e.key === 'Escape') { e.preventDefault(); close(); }
  });

  current = { close };
  document.body.append(overlay);
  update();
  input.focus();
}
