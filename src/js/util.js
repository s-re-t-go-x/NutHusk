/** 创建 DOM 元素。所有文本一律用 textContent，不使用 innerHTML。 */
export function el(tag, props = {}, ...kids) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v === undefined || v === null) continue;
    if (k === 'class') n.className = v;
    else if (k === 'style' || k === 'role' || k.startsWith('aria-')) n.setAttribute(k, v);
    else if (k.startsWith('on') && typeof v === 'function') n.addEventListener(k.slice(2), v);
    else n[k] = v;
  }
  n.append(...kids.flat().filter((x) => x !== null && x !== undefined && x !== false));
  return n;
}

function hue(s) {
  let h = 0;
  for (const c of s) h = (h * 31 + c.codePointAt(0)) >>> 0;
  return h % 360;
}

/** 插件文件的访问地址。Windows / Android 上自定义协议会被映射为 http://<scheme>.localhost。 */
export function pluginUrl(id, relPath) {
  const base = /Windows|Android/.test(navigator.userAgent) ? 'http://plugin.localhost' : 'plugin://localhost';
  const path = relPath.split('/').map(encodeURIComponent).join('/');
  return `${base}/${encodeURIComponent(id)}/${path}`;
}

/**
 * 插件图标：有 icon 就显示图片；没有或加载失败，就用名称首字加一个由 id 决定的颜色。
 * `src` 可以直接指定图片地址（安装确认框里用包内图标的 data URI）。
 */
export function badge(p, small = false, src = null) {
  const cls = 'badge' + (small ? ' sm' : '');
  const letter = [...(p.name || '?')][0];
  const color = `background:hsl(${hue(p.id)} 55% 42%)`;
  const url = src ?? (p.icon ? pluginUrl(p.id, p.icon) : null);
  if (!url) return el('span', { class: cls, style: color, textContent: letter });

  const img = el('img', { class: 'badge-img', alt: '', src: url, draggable: false });
  const box = el('span', { class: cls + ' has-img' }, img);
  img.addEventListener('error', () => {
    box.classList.remove('has-img');
    box.setAttribute('style', color);
    box.replaceChildren(letter);
  });
  return box;
}

export function toast(text) {
  const t = el('div', { class: 'toast', role: 'status', textContent: text });
  document.body.append(t);
  setTimeout(() => t.remove(), 2600);
}

/** 权限的中文说明（界面上展示用）。 */
export const PERM_LABEL = { clipboard: '读写剪贴板', 'fs.read': '读取文件', 'fs.write': '写入文件', network: '访问网络' };

export function fmtSize(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/* ---------- 图标（内联 SVG，不依赖字体或网络） ---------- */

const SHIELD = '<path d="M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z"/>';
const ICONS = {
  home: '<path d="M3 11l9-8 9 8"/><path d="M5 10v10h5v-6h4v6h5V10"/>',
  star: '<path d="M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  plugin: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><path d="M17.5 14v7M14 17.5h7"/>',
  settings: '<path d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1"/><circle cx="15" cy="6" r="2"/><circle cx="9" cy="12" r="2"/><circle cx="17" cy="18" r="2"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.4 1.4M17.6 17.6L19 19M5 19l1.4-1.4M17.6 6.4L19 5"/>',
  moon: '<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/>',
  pin: '<path d="M15 4.5l-4 4L7 10l-1.5 1.5 7 7L14 17l1.5-4 4-4M9 15l-4.5 4.5M14.5 4L20 9.5"/>',
  columns: '<rect x="3" y="4" width="18" height="16" rx="2"/><line x1="12" y1="4" x2="12" y2="20"/>',
  zap: '<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>',
  layers: '<polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/>',
  send: '<path d="M22 2L11 13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2"/>',
  shield: SHIELD,
  shieldCheck: SHIELD + '<path d="M9 12l2 2 4-4"/>',
  shieldAlert: SHIELD + '<path d="M12 8v5M12 16v.5"/>',
  x: '<path d="M6 6l12 12M18 6L6 18"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  up: '<path d="M12 19V5M6 11l6-6 6 6"/>',
  down: '<path d="M12 5v14M6 13l6 6 6-6"/>',
  refresh: '<path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/>',
  download: '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/>',
};

export function icon(name, size = 16) {
  const NS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(NS, 'svg');
  for (const [k, v] of Object.entries({
    viewBox: '0 0 24 24', width: size, height: size, fill: 'none', stroke: 'currentColor',
    'stroke-width': 2, 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'aria-hidden': 'true',
  })) svg.setAttribute(k, String(v));
  svg.setAttribute('class', 'ico');
  svg.innerHTML = ICONS[name] ?? ''; // 只来自上面的静态字符串
  return svg;
}

/* ---------- 开关与信任标识 ---------- */

/** 开关。点击事件不会冒泡，所以放在卡片里也不会误触"打开工具"。 */
export function switchEl({ checked, disabled = false, title = '', label = '', onclick }) {
  const input = el('input', { type: 'checkbox', checked, disabled, 'aria-label': label || title, role: 'switch', onclick });
  return el('label', { class: 'switch', title, onclick: (e) => e.stopPropagation() }, input, el('span', { class: 'slider' }));
}

const TRUST_VIEW = {
  trusted: { icon: 'shieldCheck', text: '可信来源' },
  unverified: { icon: 'shieldAlert', text: '未经安全验证' },
  invalid: { icon: 'shieldAlert', text: '签名无效' },
};

/** 来源标识：绿色「可信来源」/ 黄色「未经安全验证」/ 红色「签名无效」。 */
export function trustTag(trust) {
  const v = TRUST_VIEW[trust?.level] ?? TRUST_VIEW.unverified;
  const level = TRUST_VIEW[trust?.level] ? trust.level : 'unverified';
  return el('span', { class: `tag trust trust-${level}`, title: trust?.reason ?? '' }, icon(v.icon, 12), v.text);
}
