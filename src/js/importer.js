import { el, toast, fmtSize, icon, badge, trustTag, PERM_LABEL } from './util.js';
import { showDialog } from './dialog.js';

let busy = false;
export const isImporting = () => busy;

const MAX_PACKAGE_BYTES = 300 * 1024 * 1024;
const box = (cls, ...kids) => el('div', { class: 'dlg-box ' + cls }, ...kids);
const boxTitle = (iconName, text) => el('div', { class: 'box-title' }, icon(iconName, 16), text);

/** 来源可信度：绿色「可信来源」/ 黄色「未经安全验证」/ 红色「签名无效」。 */
function trustBox(trust) {
  if (trust.level === 'trusted') {
    return box('ok', boxTitle('shieldCheck', '可信来源'), el('div', { textContent: trust.reason }));
  }
  if (trust.level === 'invalid') {
    return box('err', boxTitle('shieldAlert', '签名无效，已禁止安装'), el('div', { textContent: trust.reason }));
  }
  return box('warn', boxTitle('shieldAlert', '未经安全验证'),
    el('div', { textContent: `${trust.reason}。这个插件没有经过开发者的安全审查，请确认来源可靠后再安装。` }));
}

/** 安装确认框的内容：清楚告诉用户这个包是什么、来自谁、会做什么。 */
export function buildInstallBody(info) {
  const m = info.manifest;
  const parts = [
    el('div', { class: 'dlg-head' },
      badge({ id: m.id, name: m.name }, false, info.iconData),
      el('div', {}, el('b', { textContent: m.name }), el('span', { class: 'muted', textContent: ` v${m.version}` }),
        el('div', { class: 'muted small', textContent: m.id }))),
  ];
  if (m.description) parts.push(el('p', { textContent: m.description }));
  parts.push(trustBox(info.trust));

  if (info.installedVersion) {
    parts.push(box('info', info.installedVersion === m.version
      ? `已安装相同版本 v${info.installedVersion}，将重新安装并覆盖。`
      : `已安装 v${info.installedVersion}，将更新为 v${m.version}。`));
  }
  if (info.compatError) {
    parts.push(box('err', `无法安装：${info.compatError}。请换用匹配的版本，或升级 NutHusk。`));
  }

  if (info.backendCommand) {
    parts.push(box('warn',
      boxTitle('shieldAlert', '带有后端程序'),
      el('div', { textContent: '它会以你当前用户的权限在本机运行，能读写你能访问的任何文件。' }),
      el('code', { class: 'cmd', textContent: info.backendCommand })));
    if (info.backendFound === false) {
      if (info.runtimeHint) {
        parts.push(box('info', info.runtimeHint));
      } else {
        parts.push(box('err', '在本机没有找到这个程序，安装后它将无法运行。（从桌面图标启动的应用，看到的 PATH 可能比终端里的短。）'));
      }
    }
  }

  const perms = m.permissions ?? [];
  parts.push(el('div', { class: 'dlg-sec' },
    el('div', { class: 'muted small', textContent: '声明的权限' }),
    perms.length
      ? el('div', { class: 'mrow-tags' }, ...perms.map((p) => el('span', { class: 'tag' + (p === 'network' ? ' warn' : ''), textContent: PERM_LABEL[p] ?? p })))
      : el('div', { class: 'small', textContent: '未声明任何权限' }),
    el('div', { class: 'muted small', textContent: '权限由插件自行声明；目前只有「访问网络」会对插件界面强制限制，带后端的插件不受这些限制。' })));

  if (info.installTarget) {
    parts.push(el('div', { class: 'muted small' }, `将安装到：${info.installTargetLabel}　`, el('span', { class: 'mrow-path', textContent: info.installTarget })));
  }
  parts.push(el('div', { class: 'muted small dlg-meta' },
    `${info.fileCount} 个文件 · 解压后 ${fmtSize(info.unpackedSize)} · `,
    el('span', { title: info.sha256, textContent: `SHA-256 ${info.sha256.slice(0, 16)}…` })));
  return el('div', {}, ...parts);
}

/**
 * 依次处理要导入的插件包：检查 → 用户确认 → 安装。
 * items 里每一项是路径字符串，或 { path, name, temp }（temp 表示是拖放时暂存的临时文件，处理完会删除）。
 * 安装前一定会弹出确认框；确认的是哪个文件（SHA-256），安装的就是哪个文件。
 */
export async function importPackages(items, { invoke, refresh }) {
  if (busy) return toast('正在导入，请先完成当前这个');
  busy = true;
  let changed = false;
  try {
    for (const raw of items) {
      const item = typeof raw === 'string' ? { path: raw } : raw;
      const fileName = item.name ?? String(item.path).split(/[\\/]/).pop();
      try {
        let info;
        try {
          info = await invoke('inspect_plugin_package', { path: item.path });
        } catch (err) {
          await showDialog({
            title: '无法导入',
            body: el('div', {}, el('b', { textContent: fileName }), el('p', { class: 'err-text', textContent: String(err) })),
            buttons: [{ label: '知道了', value: true, primary: true }],
          });
          continue;
        }

        const isUpdate = !!info.installedVersion;
        const blocked = !!info.compatError || info.trust.level === 'invalid';
        const ok = await showDialog({
          title: isUpdate ? '更新插件' : '安装插件',
          body: buildInstallBody(info),
          buttons: [
            { label: '取消', value: false },
            { label: isUpdate ? '更新' : '安装', value: true, primary: true, disabled: blocked },
          ],
        });
        if (!ok) continue;

        try {
          const r = await invoke('install_plugin_package', { path: item.path, sha256: info.sha256 });
          changed = true;
          const where = r?.groupLabel ? `（${r.groupLabel}）` : '';
          toast(r && r.active === false
            ? `已安装${where}，但另一目录里优先级更高的同名插件仍在生效。可在设置里调整目录优先级。`
            : `${isUpdate ? '已更新' : '已安装'}：${info.manifest.name}${where}`);
        } catch (err) {
          toast(`安装失败：${err}`);
        }
      } finally {
        if (item.temp) {
          try { await invoke('discard_dropped_package', { path: item.path }); } catch { /* 临时文件，退出时也会清理 */ }
        }
      }
    }
  } finally {
    busy = false;
  }
  if (changed) await refresh();
}

function readBytes(file) {
  if (typeof file.arrayBuffer === 'function') return file.arrayBuffer();
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(r.result);
    r.onerror = () => reject(r.error);
    r.readAsArrayBuffer(file);
  });
}

/**
 * 处理拖进窗口的文件：浏览器只给文件内容、不给路径，所以先把内容交给后端存成临时文件，
 * 之后和"选择文件"走完全相同的检查、确认、安装流程。
 */
export async function importDroppedFiles(files, { invoke, refresh }) {
  if (busy) return toast('正在导入，请先完成当前这个');
  const packages = files.filter((f) => /\.(nut|mytool|zip)$/i.test(f.name));
  if (!packages.length) return toast('只支持 .nut 或 .zip 格式的插件包（兼容旧的 .mytool）');
  if (packages.length < files.length) toast(`已忽略 ${files.length - packages.length} 个不是插件包的文件`);

  const items = [];
  for (const f of packages) {
    if (f.size > MAX_PACKAGE_BYTES) {
      toast(`${f.name} 太大，不像是插件包`);
      continue;
    }
    try {
      const bytes = new Uint8Array(await readBytes(f));
      const path = await invoke('save_dropped_package', bytes, { headers: { 'x-file-name': encodeURIComponent(f.name) } });
      items.push({ path, name: f.name, temp: true });
    } catch (err) {
      toast(`无法读取 ${f.name}：${err}`);
    }
  }
  if (items.length) await importPackages(items, { invoke, refresh });
}
