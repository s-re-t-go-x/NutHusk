import { el, badge, icon, switchEl, trustTag, PERM_LABEL } from './util.js';
import { filterPlugins } from './state.js';

const FILTERS = [['all', '全部'], ['enabled', '已启用'], ['disabled', '已禁用'], ['running', '运行中'], ['error', '加载失败']];

function logPanel(p, app, actions) {
  const lines = app.manage.logs.get(p.id);
  const parts = [];
  if (p.error) parts.push(`加载失败：${p.error}`);
  if (lines === undefined) parts.push('读取中…');
  else if (lines.length) parts.push(...lines);
  else if (!p.error) parts.push('暂无输出。后端在第一次被调用时才会启动。');
  return el('div', { class: 'logpanel' },
    el('pre', { class: 'logbox', textContent: parts.join('\n') }),
    el('div', { class: 'logbtns' },
      p.hasBackend ? el('button', { class: 'btn sm', type: 'button', textContent: '刷新', onclick: () => actions.loadLogs(p.id) }) : null));
}

function row(p, app, actions) {
  const logsOpen = app.manage.openLogs.has(p.id);
  const canLog = p.hasBackend || !!p.error;
  return el('div', { class: 'mrow' + (p.error ? ' error' : '') + (p.enabled ? '' : ' off') },
    el('div', { class: 'mrow-main' },
      badge(p),
      el('div', { class: 'mrow-text' },
        el('div', { class: 'mrow-title' },
          el('b', { textContent: p.name }),
          el('span', { class: 'muted small', textContent: `v${p.version}` }),
          trustTag(p.trust),
          p.backendRunning ? el('span', {
            class: 'tag ok',
            title: p.backendPid ? `后端进程运行中（PID: ${p.backendPid}）` : '后端进程运行中',
            textContent: p.backendPid ? `运行中 · PID ${p.backendPid}` : '运行中',
          }) : null,
          p.error ? el('span', { class: 'tag err', textContent: '加载失败' }) : null),
        el('div', { class: 'muted small', textContent: p.error ?? p.description }),
        el('div', { class: 'mrow-tags' },
          el('span', { class: 'tag', title: p.path, textContent: p.groupLabel }),
          ...p.permissions.map((x) => el('span', { class: 'tag' + (x === 'network' ? ' warn' : ''), textContent: PERM_LABEL[x] ?? x })),
          p.hasBackend ? el('span', { class: 'tag warn', title: '以当前用户的权限在本机运行的程序', textContent: `后端：${p.backendCommand}` }) : null,
          p.shadowed.length ? el('span', { class: 'tag', title: p.shadowed.join('\n'), textContent: `覆盖了另一目录中的同名插件` }) : null),
        el('div', { class: 'muted small mrow-path', textContent: p.path })),
      el('div', { class: 'mrow-actions' },
        p.backendRunning ? el('button', { class: 'btn sm', type: 'button', textContent: '停止后端', onclick: () => actions.stopBackend(p.id) }) : null,
        canLog ? el('button', { class: 'btn sm', type: 'button', textContent: logsOpen ? '收起日志' : '日志', onclick: () => actions.toggleLogs(p.id) }) : null,
        el('button', { class: 'btn sm danger', type: 'button', textContent: '卸载', onclick: () => actions.uninstall(p) }),
        switchEl({
          checked: p.enabled,
          disabled: !!p.error,
          title: p.enabled ? '已启用，点击禁用' : '已禁用，点击启用',
          label: '启用或禁用',
          onclick: (e) => actions.setEnabled(p.id, e.target.checked),
        }))),
    logsOpen ? logPanel(p, app, actions) : null);
}

export function renderManage(root, app, actions) {
  const m = app.manage;
  const total = app.plugins.length;
  const bad = app.plugins.filter((p) => p.error).length;
  const running = app.plugins.filter((p) => p.backendRunning).length;

  const listEl = el('div', { class: 'mlist' });
  const drawList = () => {
    const rows = filterPlugins(app.plugins, m.query, m.filter);
    listEl.replaceChildren(...(rows.length
      ? rows.map((p) => row(p, app, actions))
      : [el('div', { class: 'empty', textContent: total ? '没有符合条件的插件' : '还没有任何插件' })]));
  };

  const search = el('input', {
    class: 'msearch', type: 'search', placeholder: '搜索插件名称、id、关键词', value: m.query, 'aria-label': '搜索插件',
    oninput: (e) => { m.query = e.target.value; drawList(); }, // 只重绘列表，输入框保持焦点
  });
  const chips = el('div', { class: 'mchips' }, ...FILTERS.map(([key, label]) => el('button', {
    class: 'mchip' + (m.filter === key ? ' active' : ''),
    type: 'button',
    textContent: label + (key === 'error' && bad ? ` ${bad}` : '') + (key === 'running' && running ? ` ${running}` : ''),
    onclick: () => { m.filter = key; renderManage(root, app, actions); },
  })));

  root.replaceChildren(
    el('div', { class: 'mhead' },
      el('div', {},
        el('h2', { class: 'mtitle', textContent: '插件管理' }),
        el('div', { class: 'muted small', textContent: `共 ${total} 个插件` + (running ? `，${running} 个后端运行中` : '') + (bad ? `，${bad} 个加载失败` : '') })),
      el('div', { class: 'mbtns' },
        running ? el('button', { class: 'btn', type: 'button', textContent: `停止所有后端 (${running})`, onclick: () => actions.stopAllBackends() }) : null,
        el('button', { class: 'btn', type: 'button', onclick: () => actions.rescan() }, icon('refresh', 14), ' 重新扫描'),
        el('button', { class: 'btn primary', type: 'button', textContent: '导入插件包…', onclick: () => actions.pickAndImport() }))),
    el('div', { class: 'dropzone', textContent: '把 .nut 插件包拖进窗口即可导入（在首页或本页）。安装前会先让你确认来源和权限，不会直接安装。' }),
    el('div', { class: 'mtools' }, search, chips),
    listEl,
    el('div', { class: 'muted small mfoot' },
      '插件放在两个目录里，地位相同、都会被读取。目录位置和优先级在 ',
      el('button', { class: 'linkbtn', type: 'button', textContent: '设置', onclick: () => actions.openSettings() }),
      ' 里查看和调整。'));
  drawList();
}
