import test from 'node:test';
import assert from 'node:assert/strict';
import { createDom, tick, $$, $, key, makeTauri, mkPlugin, mkPackage, trust } from './helpers.js';
import { filterPlugins } from '../src/js/state.js';

const window = createDom();
const m = makeTauri({
  plugins: [
    mkPlugin('com.example.hash-calc', '哈希计算', { hasBackend: true, backendCommand: 'node main.js', group: 'program', groupLabel: '程序目录', path: '/app/plugins/hash-calc', shadowed: ['/data/plugins/old'] }),
    mkPlugin('com.example.uuid', 'UUID 生成器', { trust: trust.ok }),
    mkPlugin('invalid:oops', 'oops', { version: '?', ui: null, error: 'manifest 无效: 缺少字段', category: '其他', path: '/data/plugins/oops' }),
  ],
  logs: { 'com.example.hash-calc': ['[宿主] 启动: node main.js', 'got hash'] },
  pick: '/tmp/update.nut',
  packages: {
    '/tmp/new.nut': mkPackage('com.example.new', '新工具', '1.0.0', {
      manifest: { permissions: ['network', 'fs.read'] },
      info: { backendCommand: 'node main.js', backendFound: false, iconData: 'data:image/svg+xml;base64,PHN2Zy8+' },
    }, mkPlugin('com.example.new', '新工具', { permissions: ['network', 'fs.read'] })),
    '/tmp/update.nut': mkPackage('com.example.uuid', 'UUID 生成器', '1.1.0', { info: { installedVersion: '1.0.0', trust: trust.ok } }),
    '/tmp/future.nut': mkPackage('com.example.future', '未来', '1.0.0', { info: { compatError: '需要宿主 API ^2.0.0，当前为 1.0.0' } }),
    '/tmp/evil.nut': mkPackage('com.example.evil', '被篡改的', '1.0.0', { info: { trust: trust.bad } }),
    '/tmp/fail.nut': { ...mkPackage('com.example.fail', '会失败的', '1.0.0'), fail: '删除失败（可能没有权限，或文件被占用）' },
  },
});
window.__TAURI__ = m.tauri;

const rowOf = (name) => $$('.mrow').find((r) => r.querySelector('b').textContent === name);
const btn = (root, label) => [...root.querySelectorAll('button')].find((b) => b.textContent.trim() === label);
const dlgBtn = (label) => $$('.dlg-actions .btn').find((b) => b.textContent === label);
const dlgText = () => document.querySelector('.dlg-panel')?.textContent ?? '';
const goManage = async () => { $$('#nav-bottom .nav-item').find((b) => b.textContent.includes('插件管理')).click(); await tick(); };

await import('../src/js/main.js');
await tick();

test('filterPlugins：状态与关键词', () => {
  const ps = m.state.plugins;
  assert.deepEqual(filterPlugins(ps, '', 'error').map((p) => p.id), ['invalid:oops']);
  assert.deepEqual(filterPlugins(ps, 'uuid').map((p) => p.id), ['com.example.uuid']);
  assert.equal(filterPlugins(ps, '', 'all').length, 3);
  assert.deepEqual(filterPlugins([{ ...ps[0], enabled: false }, ps[1]], '', 'disabled').map((p) => p.id), ['com.example.hash-calc']);
});

test('管理页：不再区分内置/用户安装，改为显示所在目录、路径、来源可信度', async () => {
  await goManage();
  assert.equal($$('.tab.active .tab-title')[0].textContent, '插件管理');
  assert.equal($$('.mrow').length, 3);
  const hash = rowOf('哈希计算');
  assert.ok(!hash.textContent.includes('内置') && !hash.textContent.includes('用户安装'));
  assert.ok(hash.textContent.includes('程序目录'));
  assert.ok(hash.textContent.includes('/app/plugins/hash-calc'));
  assert.ok(hash.textContent.includes('后端：node main.js'));
  assert.ok(hash.textContent.includes('覆盖了另一目录中的同名插件'));
  assert.ok(hash.querySelector('.trust-unverified'));
  assert.ok(rowOf('UUID 生成器').querySelector('.trust-trusted'));
  assert.ok(rowOf('oops').classList.contains('error'));
  assert.ok(rowOf('oops').textContent.includes('加载失败'));
  await goManage();
  assert.equal($$('.tab').length, 2, '重复点击不会重复打开标签');
});

test('所有插件都有卸载按钮（两个目录里的一视同仁）', () => {
  for (const name of ['哈希计算', 'UUID 生成器', 'oops']) assert.ok(btn(rowOf(name), '卸载'), name);
});

test('筛选与搜索：输入时输入框保持不变（不丢焦点）', async () => {
  $$('.mchip').find((c) => c.textContent.startsWith('加载失败')).click();
  await tick();
  assert.deepEqual($$('.mrow b').map((b) => b.textContent), ['oops']);
  $$('.mchip').find((c) => c.textContent === '全部').click();
  await tick();
  const input = document.querySelector('.msearch');
  input.value = 'uuid';
  input.dispatchEvent(new window.Event('input'));
  assert.deepEqual($$('.mrow b').map((b) => b.textContent), ['UUID 生成器']);
  assert.strictEqual(document.querySelector('.msearch'), input);
  input.value = '';
  input.dispatchEvent(new window.Event('input'));
  assert.equal($$('.mrow').length, 3);
});

test('日志：后端日志、加载失败原因', async () => {
  btn(rowOf('哈希计算'), '日志').click();
  await tick();
  assert.ok(rowOf('哈希计算').querySelector('.logbox').textContent.includes('got hash'));
  btn(rowOf('oops'), '日志').click();
  await tick();
  assert.ok(rowOf('oops').querySelector('.logbox').textContent.includes('manifest 无效'));
  btn(rowOf('哈希计算'), '收起日志').click();
  await tick();
  assert.equal(rowOf('哈希计算').querySelector('.logbox'), null);
});

test('管理页的开关与「设置」链接', async () => {
  rowOf('UUID 生成器').querySelector('label.switch input').click();
  await tick();
  assert.deepEqual(m.called('set_plugin_enabled').pop()[1], { id: 'com.example.uuid', enabled: false });
  rowOf('UUID 生成器').querySelector('label.switch input').click();
  await tick();
  btn(document.getElementById('manage'), '设置').click();
  await tick();
  assert.equal($$('.tab.active .tab-title')[0].textContent, '设置');
  await goManage();
});

/* ---------- 导入：确认框的内容 ---------- */

const importPaths = async (paths) => {
  // 通过"选择文件"入口走完整流程
  m.state.pick = paths;
  $$('#nav .nav-item')[0].click();
  await tick();
  $$('#home .card.import')[0].click();
  await tick();
};

test('导入：确认框写明来源、权限、后端风险、安装位置，确认之前不会安装', async () => {
  await importPaths(['/tmp/new.nut']);
  assert.deepEqual(m.called('dialog.open').pop()[1].filters[0].extensions, ['nut', 'mytool', 'zip']);
  assert.deepEqual(m.called('inspect_plugin_package').pop()[1], { path: '/tmp/new.nut' });
  assert.equal(m.called('install_plugin_package').length, 0);
  const t = dlgText();
  assert.ok(t.includes('安装插件') && t.includes('新工具'));
  assert.ok(document.querySelector('.dlg-box.warn') && t.includes('未经安全验证'), '黄色：未经安全验证');
  assert.ok(t.includes('没有经过开发者的安全审查'));
  assert.ok(t.includes('访问网络') && t.includes('读取文件'));
  assert.ok(t.includes('以你当前用户的权限在本机运行') && t.includes('node main.js'));
  assert.ok(t.includes('没有找到这个程序'));
  assert.ok(t.includes('将安装到：AppData') && t.includes('/data/plugins'));
  assert.ok(t.includes('SHA-256 aaaaaaaaaaaaaaaa'));
  assert.equal(document.querySelector('.dlg-head img.badge-img').getAttribute('src'), 'data:image/svg+xml;base64,PHN2Zy8+', '确认框里显示包内图标');
  dlgBtn('取消').click();
  await tick();
  assert.equal(m.called('install_plugin_package').length, 0);
});

test('导入：确认后用确认过的 SHA-256 安装并刷新', async () => {
  await importPaths(['/tmp/new.nut']);
  dlgBtn('安装').click();
  await tick();
  assert.deepEqual(m.called('install_plugin_package').pop()[1], { path: '/tmp/new.nut', sha256: 'a'.repeat(64) });
  assert.ok($$('#home .card-name').some((n) => n.textContent === '新工具'));
  assert.ok($$('.toast').some((t) => t.textContent.includes('已安装：新工具（AppData）')));
});

test('导入：可信来源显示绿色；更新显示版本变化', async () => {
  await importPaths(['/tmp/update.nut']);
  assert.ok(document.querySelector('.dlg-box.ok'));
  assert.ok(dlgText().includes('更新插件') && dlgText().includes('可信来源') && dlgText().includes('由「NutHusk 官方」签名'));
  assert.ok(dlgText().includes('已安装 v1.0.0，将更新为 v1.1.0'));
  assert.ok(dlgBtn('更新'));
  key(document.activeElement ?? document.body, 'Escape');
  await tick();
  assert.equal(document.querySelector('.dlg-overlay'), null);
});

test('导入：签名无效（被篡改）→ 红色，禁止安装', async () => {
  const before = m.called('install_plugin_package').length;
  await importPaths(['/tmp/evil.nut']);
  assert.ok(document.querySelector('.dlg-box.err'));
  assert.ok(dlgText().includes('签名无效，已禁止安装'));
  assert.equal(dlgBtn('安装').disabled, true);
  dlgBtn('安装').click();
  await tick();
  assert.equal(m.called('install_plugin_package').length, before);
  dlgBtn('取消').click();
  await tick();
});

test('导入：宿主 API 不兼容 → 安装按钮不可用', async () => {
  await importPaths(['/tmp/future.nut']);
  assert.ok(dlgText().includes('无法安装：需要宿主 API ^2.0.0'));
  assert.equal(dlgBtn('安装').disabled, true);
  dlgBtn('取消').click();
  await tick();
});

test('导入：无效的包显示原因；安装失败给出提示', async () => {
  await importPaths(['/tmp/garbage.nut']);
  assert.ok(dlgText().includes('无法导入') && dlgText().includes('garbage.nut') && dlgText().includes('invalid Zip archive'));
  dlgBtn('知道了').click();
  await tick();

  await importPaths(['/tmp/fail.nut']);
  dlgBtn('安装').click();
  await tick();
  assert.ok($$('.toast').some((t) => t.textContent.includes('安装失败：删除失败')));
});

test('导入：另一目录里优先级更高的同名插件仍在生效时，如实告知', async () => {
  m.state.installResult = { id: 'com.example.new', path: '/x', groupLabel: 'AppData', active: false };
  m.state.plugins = m.state.plugins.filter((p) => p.id !== 'com.example.new');
  await importPaths(['/tmp/new.nut']);
  dlgBtn('安装').click();
  await tick();
  assert.ok($$('.toast').some((t) => t.textContent.includes('优先级更高的同名插件仍在生效')));
  m.state.installResult = null;
});

test('对话框打开时，全局快捷键不会再叠加弹出搜索面板', async () => {
  await importPaths(['/tmp/update.nut']);
  key(window, 'k', { ctrlKey: true });
  assert.equal($$('.pal-overlay').length, 0);
  dlgBtn('取消').click();
  await tick();
});

/* ---------- 卸载 ---------- */

test('卸载：先确认（显示路径）；取消不卸载；确认后相关标签关闭、列表刷新', async () => {
  $$('#nav .nav-item')[0].click();
  await tick();
  $$('#home .card').find((c) => c.querySelector('.card-name')?.textContent === 'UUID 生成器').click();
  await tick();
  assert.ok($$('.tab-title').some((t) => t.textContent === 'UUID 生成器'));
  await goManage();

  btn(rowOf('UUID 生成器'), '卸载').click();
  await tick();
  assert.ok(dlgText().includes('确定卸载「UUID 生成器」') && dlgText().includes('/data/plugins/com.example.uuid'));
  dlgBtn('取消').click();
  await tick();
  assert.equal(m.called('uninstall_plugin').length, 0);

  btn(rowOf('UUID 生成器'), '卸载').click();
  await tick();
  dlgBtn('卸载').click();
  await tick();
  assert.deepEqual(m.called('uninstall_plugin').pop()[1], { id: 'com.example.uuid' });
  assert.equal(rowOf('UUID 生成器'), undefined);
  assert.ok(!$$('.tab-title').some((t) => t.textContent === 'UUID 生成器'));
  assert.equal($$('iframe.frame').length, 0);
});

test('重新扫描；Ctrl+W 关闭管理页标签', async () => {
  btn(document.getElementById('manage'), '重新扫描').click();
  await tick();
  assert.equal(m.called('reload_plugins').length, 1);
  key(window, 'w', { ctrlKey: true });
  await tick();
  assert.ok(!$$('.tab-title').some((t) => t.textContent === '插件管理'));
});
