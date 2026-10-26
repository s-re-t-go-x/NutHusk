import { el, badge } from './util.js';
import { searchPlugins } from './state.js';

let current = null;

export function isPaletteOpen() {
  return current !== null;
}

export function closePalette() {
  current?.close();
}

/** Ctrl/Cmd+K 命令面板。再次调用会关闭已打开的面板。 */
export function openPalette({ plugins, favorites, recent, onOpen }) {
  if (current) return closePalette();

  const prevFocus = document.activeElement;
  let results = [];
  let rows = [];
  let index = 0;

  const input = el('input', { class: 'pal-input', type: 'text', placeholder: '搜索工具…', spellcheck: false, 'aria-label': '搜索工具' });
  const list = el('div', { class: 'pal-list', role: 'listbox' });
  const panel = el('div', { class: 'pal-panel', role: 'dialog', 'aria-modal': 'true', 'aria-label': '搜索工具' }, input, list);
  const overlay = el('div', { class: 'pal-overlay', onmousedown: (e) => { if (e.target === overlay) close(); } }, panel);

  function close() {
    overlay.remove();
    current = null;
    prevFocus?.focus?.();
  }

  function choose(p) {
    if (!p) return;
    close();
    onOpen(p.id);
  }

  function select(i) {
    if (!rows.length) return;
    index = Math.max(0, Math.min(rows.length - 1, i));
    rows.forEach((r, n) => r.classList.toggle('sel', n === index));
    rows[index].scrollIntoView?.({ block: 'nearest' });
  }

  function update() {
    results = searchPlugins(plugins, input.value, { favorites, recent }).slice(0, 30);
    rows = results.map((p, i) => el('div', {
      class: 'pal-row' + (i === 0 ? ' sel' : ''),
      role: 'option',
      onmousemove: () => { if (index !== i) select(i); },
      onclick: () => choose(p),
    },
      badge(p, true),
      el('div', { class: 'pal-main' },
        el('div', { class: 'pal-name', textContent: p.name }),
        el('div', { class: 'pal-desc', textContent: p.description })),
      el('span', { class: 'tag', textContent: p.category })));
    index = 0;
    list.replaceChildren(...(rows.length ? rows : [el('div', { class: 'pal-empty', textContent: '没有匹配的工具' })]));
  }

  input.addEventListener('input', update);
  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); select(index + 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); select(index - 1); }
    else if (e.key === 'Enter') { e.preventDefault(); choose(results[index]); }
    else if (e.key === 'Escape') { e.preventDefault(); close(); }
  });

  current = { close };
  document.body.append(overlay);
  update();
  input.focus();
}
