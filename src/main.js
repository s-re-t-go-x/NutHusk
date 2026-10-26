const { invoke } = window.__TAURI__.core;

// Windows / Android 上自定义协议会被映射为 http://<scheme>.localhost
const pluginBase = /Windows|Android/.test(navigator.userAgent) ? 'http://plugin.localhost' : 'plugin://localhost';

const nav = document.getElementById('nav');
const stage = document.getElementById('stage');
const search = document.getElementById('search');
let plugins = [];
let current = null; // { id, frame }

function el(tag, props = {}, ...kids) {
  const n = Object.assign(document.createElement(tag), props);
  n.append(...kids);
  return n;
}

function showMessage(text) {
  stage.replaceChildren(el('div', { className: 'msg', textContent: text }));
  current = null;
}

function open(p) {
  if (p.error) return showMessage(`${p.name} 加载失败：${p.error}`);
  if (!p.enabled) return showMessage(`${p.name} 已禁用`);
  if (!p.ui) return showMessage(`${p.name} 没有界面`);

  const frame = el('iframe');
  frame.setAttribute('sandbox', 'allow-scripts'); // 无 allow-same-origin：插件拿不到任何宿主权限
  frame.src = `${pluginBase}/${encodeURIComponent(p.id)}/${p.ui.split('/').map(encodeURIComponent).join('/')}`;
  const head = el('div', { className: 'head' }, el('b', { textContent: p.name }), el('small', { textContent: p.description }));
  stage.replaceChildren(head, frame);
  current = { id: p.id, frame };
  render();
}

function render() {
  const q = search.value.trim().toLowerCase();
  const visible = plugins.filter((p) =>
    !q || [p.name, p.description, ...p.keywords].some((s) => s.toLowerCase().includes(q)));

  const rows = [];
  let lastCategory = null;
  for (const p of visible) {
    if (p.category !== lastCategory) {
      rows.push(el('div', { className: 'group', textContent: p.category }));
      lastCategory = p.category;
    }
    const toggle = el('input', { type: 'checkbox', checked: p.enabled, disabled: !!p.error, title: '启用 / 禁用' });
    toggle.onclick = async (ev) => {
      ev.stopPropagation();
      await invoke('set_plugin_enabled', { id: p.id, enabled: toggle.checked });
      await reload();
    };
    const row = el('div', {
      className: 'item' + (p.error ? ' error' : '') + (p.enabled ? '' : ' off') + (current?.id === p.id ? ' active' : ''),
      onclick: () => open(p),
    }, el('span', { className: 'badge', textContent: p.name.slice(0, 1) }), el('span', { className: 'name', textContent: p.name }), toggle);
    rows.push(row);
  }
  nav.replaceChildren(...rows);
}

async function reload() {
  plugins = await invoke('list_plugins');
  render();
}

// 桥接：插件界面 --postMessage--> 壳 --IPC--> Rust --stdio JSON-RPC--> 插件后端
window.addEventListener('message', async (e) => {
  const cur = current;
  if (!cur || e.source !== cur.frame.contentWindow) return; // 只接受当前插件 iframe 的消息
  const { type, reqId, method, params } = e.data ?? {};
  if (type !== 'call') return;
  try {
    const result = await invoke('plugin_call', { id: cur.id, method: String(method), params: params ?? null });
    cur.frame.contentWindow.postMessage({ reqId, result }, '*');
  } catch (err) {
    cur.frame.contentWindow.postMessage({ reqId, error: String(err) }, '*');
  }
});

search.addEventListener('input', render);
window.addEventListener('keydown', (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault();
    search.focus();
    search.select();
  }
});

reload();
