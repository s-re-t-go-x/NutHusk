import { el, switchEl } from './util.js';

/**
 * 设置页由下面这份清单生成。以后加设置项，往 SECTIONS 里加一项即可：
 *   { type: 'segmented', key, label, desc, options: [[值, 文字], …] }
 *   { type: 'switch', key, label, desc }
 *   { custom: (app, actions) => 节点 }   —— 需要特殊界面的，自己画
 * 设置的读写和校验在 Rust 侧（get_settings / patch_settings），键名保持一致。
 */
const THEME_OPTIONS = [['system', '跟随系统'], ['light', '浅色'], ['dark', '深色']];

const SECTIONS = [
  {
    title: '外观',
    items: [{ type: 'segmented', key: 'theme', label: '主题', desc: '跟随系统，或固定为浅色 / 深色。插件界面会同步。', options: THEME_OPTIONS }],
  },
  {
    title: '首页',
    items: [
      { type: 'switch', key: 'showRecent', label: '显示「最近使用」', desc: '在首页顶部显示最近打开过的工具。' },
      { type: 'switch', key: 'showFavorites', label: '显示「收藏」', desc: '在首页显示收藏的工具。' },
    ],
  },
  { title: '文件位置', custom: renderDirs },
  { title: '运行环境', custom: renderRuntimes },
  { title: '安全', custom: renderSigners },
  { title: '关于', custom: renderAbout },
];

function rowWith(label, desc, control) {
  return el('div', { class: 'set-row' },
    el('div', { class: 'set-label' }, el('div', { textContent: label }), desc ? el('div', { class: 'set-desc', textContent: desc }) : null),
    control);
}

function renderItem(item, app, actions) {
  const value = app.settings[item.key];
  if (item.type === 'segmented') {
    return rowWith(item.label, item.desc, el('div', { class: 'segmented', role: 'radiogroup', 'aria-label': item.label },
      ...item.options.map(([v, text]) => el('button', {
        type: 'button', role: 'radio', 'aria-checked': String(value === v),
        class: value === v ? 'active' : '', textContent: text,
        onclick: () => actions.patchSettings({ [item.key]: v }),
      }))));
  }
  return rowWith(item.label, item.desc, switchEl({
    checked: !!value, label: item.label, title: item.label,
    onclick: (e) => actions.patchSettings({ [item.key]: e.target.checked }),
  }));
}

function renderDirs(app, actions) {
  const dirs = app.dirs;
  if (!dirs.length) return el('div', { class: 'set-box' }, el('div', { class: 'set-note', textContent: '读取中…' }));
  return el('div', { class: 'set-box' },
    ...dirs.map((d, i) => el('div', { class: 'set-row dir-row' },
      el('span', { class: 'dir-no', textContent: String(i + 1) }),
      el('div', { class: 'set-label' },
        el('div', {}, d.label,
          el('span', { class: 'muted small', textContent: `　${d.pluginCount ?? 0} 个插件` }),
          d.runtimeCount ? el('span', { class: 'muted small', textContent: ` · ${d.runtimeCount} 个运行时` }) : null),
        el('div', { class: 'dir-paths' },
          ...d.paths.map((p) => el('div', { class: 'dir-path' },
            p.path,
            !p.exists ? el('span', { class: 'tag', textContent: '目录不存在' }) : null,
            p.exists && !p.writable ? el('span', { class: 'tag warn', title: '无法在这里写入新文件', textContent: '只读' }) : null)),
          el('div', { class: 'muted small', style: 'margin-top: 2px;' },
            `子目录：${(d.subdirs || ['nutPlugins', 'nutRuntimes', 'nutConfig']).join(' / ')}`))),
      el('div', { class: 'dir-btns' },
        el('button', { class: 'btn sm', type: 'button', textContent: '上移', disabled: i === 0, 'aria-label': `上移${d.label}`, onclick: () => actions.moveDir(d.key, -1) }),
        el('button', { class: 'btn sm', type: 'button', textContent: '下移', disabled: i === dirs.length - 1, 'aria-label': `下移${d.label}`, onclick: () => actions.moveDir(d.key, 1) })))),
    el('div', { class: 'set-note', textContent: '程序包含两个根目录，每个根下有插件（nutPlugins）、运行时环境（nutRuntimes）与配置（nutConfig）。靠前的目录优先；新下载的插件和运行时会保存到靠前且可写的目录中。' }));
}

function renderRuntimes(app, actions) {
  const runtimes = app.runtimes ?? [];
  if (!runtimes.length) {
    return el('div', { class: 'set-box' }, el('div', { class: 'set-note', textContent: '正在检测运行环境…' }));
  }
  return el('div', { class: 'set-box' },
    ...runtimes.map((r) => {
      let statusTag;
      let btn = null;
      if (r.installed) {
        statusTag = el('span', { class: 'tag trust trust-trusted', textContent: `已安装 v${r.installed.version}` });
        btn = el('button', { class: 'btn sm danger', type: 'button', textContent: '卸载', onclick: () => actions.removeRuntime(r.name) });
      } else if (r.onSystem) {
        statusTag = el('span', { class: 'tag', textContent: '系统自带' });
      } else if (r.downloadable) {
        statusTag = el('span', { class: 'tag', textContent: '可按需下载' });
        btn = el('button', { class: 'btn sm primary', type: 'button', textContent: '下载安装', onclick: () => actions.installRuntime(r.name) });
      } else {
        statusTag = el('span', { class: 'tag warn', textContent: '需自行安装' });
      }

      const desc = r.installed
        ? `路径：${r.installed.bin}`
        : (r.description || (r.downloadable ? `来源：${r.source || '官方'}，约 ${(r.size / 1024 / 1024).toFixed(1)} MB` : '当前系统未自带且无自动下载包'));

      return el('div', { class: 'set-row' },
        el('div', { class: 'set-label' },
          el('div', { style: 'display: flex; align-items: center; gap: 8px;' },
            el('b', { textContent: r.name }),
            el('span', { class: 'muted small', textContent: `v${r.version}` }),
            r.userRecipe ? el('span', { class: 'tag', textContent: '用户配方' }) : null,
            statusTag),
          el('div', { class: 'set-desc', textContent: desc })),
        btn);
    }),
    el('div', { class: 'set-note', textContent: '当插件需要特定运行环境（如 Python、Node 等）时，点击使用插件时会自动弹出提示并一键下载配置。也可以在配置目录的 runtimes.d/ 放入 JSON 配方以支持更多语言。' }));
}

function renderSigners(app) {
  return el('div', { class: 'set-box' },
    ...(app.signers.length
      ? app.signers.map((s) => rowWith(s.name, `密钥 ID：${s.keyId}`, el('span', { class: 'tag trust trust-trusted', textContent: '受信任' })))
      : [el('div', { class: 'set-note', textContent: '还没有配置受信任的签名密钥，所有插件都会显示为「未经安全验证」。配置方法见 docs/SIGNING.md。' })]),
    el('div', { class: 'set-note', textContent: '只有由上面这些密钥签名、且内容没被改动的插件，才会标注为绿色的「可信来源」。' }));
}

function renderAbout(app) {
  return el('div', { class: 'set-box set-about' },
    el('div', { class: 'about-name', textContent: `NutHusk${app.version ? ' v' + app.version : ''}` }),
    el('div', { class: 'muted', textContent: '轻壳之内，万千工具' }));
}

export function renderSettings(root, app, actions) {
  root.replaceChildren(
    el('div', { class: 'mhead' }, el('div', {}, el('h2', { class: 'mtitle', textContent: '设置' }))),
    ...SECTIONS.map((sec) => el('section', { class: 'set-sec' },
      el('h2', { textContent: sec.title }),
      sec.custom ? sec.custom(app, actions) : el('div', { class: 'set-box' }, ...sec.items.map((it) => renderItem(it, app, actions))))));
}
