import { el } from './util.js';

let activeDone = null;

export function closeActiveDialog(value = null) {
  if (activeDone) {
    const fn = activeDone;
    activeDone = null;
    fn(value);
  }
}

/**
 * 通用模态对话框。返回被点击按钮的 value；按 Esc 或点击遮罩返回 null。
 * buttons: [{ label, value, primary?, danger?, disabled? }]
 */
export function showDialog({ title, body, buttons }) {
  return new Promise((resolve) => {
    const prevFocus = document.activeElement;

    function done(value) {
      if (activeDone === done) activeDone = null;
      overlay.remove();
      document.removeEventListener('keydown', onKey, true);
      prevFocus?.focus?.();
      resolve(value);
    }
    activeDone = done;
    function onKey(e) {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        done(null);
      }
    }

    const btns = buttons.map((b) => el('button', {
      class: 'btn' + (b.primary ? ' primary' : '') + (b.danger ? ' danger' : ''),
      type: 'button',
      disabled: !!b.disabled,
      textContent: b.label,
      onclick: () => done(b.value),
    }));
    const panel = el('div', { class: 'dlg-panel', role: 'dialog', 'aria-modal': 'true', 'aria-label': title },
      el('h3', { class: 'dlg-title', textContent: title }),
      el('div', { class: 'dlg-body' }, body),
      el('div', { class: 'dlg-actions' }, ...btns));
    const overlay = el('div', { class: 'dlg-overlay', onmousedown: (e) => { if (e.target === overlay) done(null); } }, panel);

    document.addEventListener('keydown', onKey, true);
    document.body.append(overlay);
    const first = btns.find((_, i) => (buttons[i].primary || buttons[i].danger) && !buttons[i].disabled) ?? btns[0];
    first?.focus();
  });
}

export const isDialogOpen = () => document.querySelector('.dlg-overlay') !== null;
