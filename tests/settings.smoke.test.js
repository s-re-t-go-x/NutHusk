import test from 'node:test';
import assert from 'node:assert/strict';
import { createDom, tick, $$, $, makeTauri, mkPlugin, mkPackage } from './helpers.js';
import { renderSettings } from '../src/js/settings.js';

const window = createDom();
// 旧版本（改名前）留下的收藏和最近使用，启动时应被沿用
localStorage.setItem('toolbox.favorites', JSON.stringify(['com.example.a']));
localStorage.setItem('toolbox.recent', JSON.stringify(['com.example.a', 'com.example.b']));

const m = makeTauri({
  plugins: [mkPlugin('com.example.a', '工具甲'), mkPlugin('com.example.b', '工具乙'), mkPlugin('com.example.c', '工具丙')],
  settings: { theme: 'dark', showRecent: true, showFavorites: true, dirOrder: ['appdata', 'program'] },
  signers: [{ name: 'NutHusk 官方', keyId: '0123456789abcdef' }],
  packages: { '/tmp/nuthusk-drop/我的插件.nut': mkPackage('com.example.mine', '我的插件', '1.0.0') },
});
window.__TAURI__ = m.tauri;

const cardNames = () => $$('#home .card:not(.import) .card-name').map((n) => n.textContent);
const sectionTitles = () => $$('#home .sec h2').map((h) => h.textContent);
const dlgBtn = (label) => $$('.dlg-actions .btn').find((b) => b.textContent === label);
const seg = (label) => $$('#settings .segmented button').find((b) => b.textContent === label);
const rowOfSwitch = (label) => $$('#settings .set-row').find((r) => r.textContent.includes(label));
const goSettings = async () => { $$('#nav-bottom .nav-item').find((b) => b.textContent.includes('设置')).click(); await tick(60); };

await import('../src/js/main.js');
await tick();

test('启动时读取设置：深色主题立即生效，并在 localStorage 里留一份（下次启动防闪烁）', () => {
  assert.equal(document.documentElement.getAttribute('data-theme'), 'dark');
  assert.equal(localStorage.getItem('nuthusk.theme'), 'dark');
  assert.ok($('theme-toggle').title.includes('当前为深色'));
});

test('改名前的收藏和最近使用被沿用', () => {
  assert.ok($('nav').textContent.includes('收藏'));
  assert.equal($$('#nav .nav-item')[1].querySelector('.nav-count').textContent, '1');
  assert.deepEqual(sectionTitles(), ['最近使用', '收藏', '全部工具']);
});

test('设置页：分区与当前值', async () => {
  await goSettings();
  assert.equal($$('.tab.active .tab-title')[0].textContent, '设置');
  assert.deepEqual($$('#settings .set-sec > h2').map((h) => h.textContent), ['外观', '首页', '文件位置', '运行环境', '安全', '关于']);
  assert.equal(seg('深色').getAttribute('aria-checked'), 'true');
  assert.equal(seg('浅色').getAttribute('aria-checked'), 'false');
  assert.equal(rowOfSwitch('显示「最近使用」').querySelector('input').checked, true);
  assert.ok($('settings').textContent.includes('NutHusk v0.1.0') && $('settings').textContent.includes('轻壳之内，万千工具'));
});

test('切换主题：保存设置、立即生效、同步给已打开的插件界面', async () => {
  $$('#nav .nav-item')[0].click();
  await tick();
  $$('#home .card').find((c) => c.querySelector('.card-name')?.textContent === '工具甲').click();
  await tick();
  const frame = $$('iframe.frame')[0];
  const sent = [];
  frame.contentWindow.postMessage = (msg) => sent.push(msg);

  await goSettings();
  seg('浅色').click();
  await tick();
  assert.deepEqual(m.called('patch_settings').pop()[1], { patch: { theme: 'light' } });
  assert.equal(document.documentElement.getAttribute('data-theme'), 'light');
  assert.equal(localStorage.getItem('nuthusk.theme'), 'light');
  assert.deepEqual(sent.pop(), { type: 'theme', theme: 'light' });
  assert.equal(seg('浅色').getAttribute('aria-checked'), 'true');

  seg('跟随系统').click();
  await tick();
  assert.equal(document.documentElement.hasAttribute('data-theme'), false, '跟随系统时不设置 data-theme');
  assert.deepEqual(sent.pop(), { type: 'theme', theme: 'system' });
});

test('顶栏的主题按钮：在浅色和深色之间切换', async () => {
  seg('浅色').click();
  await tick();
  $('theme-toggle').click();
  await tick();
  assert.equal(m.state.settings.theme, 'dark');
  assert.equal(document.documentElement.getAttribute('data-theme'), 'dark');
  $('theme-toggle').click();
  await tick();
  assert.equal(m.state.settings.theme, 'light');
});

test('首页是否显示「最近使用」和「收藏」由设置控制（侧边栏的入口不受影响）', async () => {
  await goSettings();
  rowOfSwitch('显示「最近使用」').querySelector('input').click();
  await tick();
  assert.deepEqual(m.called('patch_settings').pop()[1], { patch: { showRecent: false } });
  $$('#nav .nav-item')[0].click();
  await tick();
  assert.deepEqual(sectionTitles(), ['收藏', '全部工具']);

  await goSettings();
  rowOfSwitch('显示「收藏」').querySelector('input').click();
  await tick();
  $$('#nav .nav-item')[0].click();
  await tick();
  assert.deepEqual(sectionTitles(), ['全部工具']);

  $$('#nav .nav-item')[1].click(); // 侧边栏的"收藏"页仍然可用
  await tick();
  assert.deepEqual(cardNames(), ['工具甲']);

  await goSettings();
  rowOfSwitch('显示「最近使用」').querySelector('input').click();
  rowOfSwitch('显示「收藏」').querySelector('input').click();
  await tick();
  $$('#nav .nav-item')[0].click();
  await tick();
  assert.deepEqual(sectionTitles(), ['最近使用', '收藏', '全部工具']);
});

test('插件目录：按优先级列出，标出只读；上移/下移会保存并重新读取插件', async () => {
  await goSettings();
  const rows = () => $$('#settings .dir-row');
  assert.equal(rows().length, 2);
  assert.ok(rows()[0].textContent.includes('AppData') && rows()[0].textContent.includes('/data/plugins'));
  assert.ok(rows()[1].textContent.includes('程序目录') && rows()[1].textContent.includes('只读'));
  assert.deepEqual(rows().map((r) => r.querySelector('.dir-no').textContent), ['1', '2']);
  const btns = (row) => [...row.querySelectorAll('.dir-btns button')];
  assert.equal(btns(rows()[0])[0].disabled, true, '第一个不能再上移');
  assert.equal(btns(rows()[1])[1].disabled, true, '最后一个不能再下移');
  assert.ok($('settings').textContent.includes('靠前的目录优先') && $('settings').textContent.includes('nutPlugins'));

  const listCalls = m.called('list_plugins').length;
  btns(rows()[0])[1].click(); // AppData 下移
  await tick(60);
  assert.deepEqual(m.called('set_dir_order').pop()[1], { order: ['program', 'appdata'] });
  assert.ok(rows()[0].textContent.includes('程序目录'), '界面顺序随之改变');
  assert.ok(m.called('list_plugins').length > listCalls, '优先级变化会影响哪份插件生效，所以重新读取');
  assert.deepEqual(m.state.settings.dirOrder, ['program', 'appdata']);
});

test('安全：列出受信任的签名者；没有配置时给出说明', async () => {
  assert.ok($('settings').textContent.includes('NutHusk 官方') && $('settings').textContent.includes('0123456789abcdef'));
  const root = document.createElement('div');
  renderSettings(root, { settings: m.state.settings, dirs: [], signers: [], version: '' }, {});
  assert.ok(root.textContent.includes('还没有配置受信任的签名密钥'));
  assert.ok(root.textContent.includes('未经安全验证') && root.textContent.includes('docs/SIGNING.md'));
});

/* ---------- 拖放导入（浏览器自带的拖放，不依赖 Tauri 的原生文件拖放） ---------- */

function fire(type, { files = [], types = ['Files'] } = {}) {
  const ev = new window.Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(ev, 'dataTransfer', { value: { types, files, dropEffect: '' } });
  document.body.dispatchEvent(ev);
  return ev;
}
const overlay = () => $('drop-overlay');
const pkgFile = (name, bytes = [1, 2, 3, 4]) => new window.File([new Uint8Array(bytes)], name);

test('拖入文件：显示提示层；离开后隐藏；不是文件的拖拽不拦截', () => {
  const enter = fire('dragenter');
  assert.equal(enter.defaultPrevented, true);
  assert.equal(overlay().classList.contains('hidden'), false);
  assert.equal(fire('dragover').defaultPrevented, true, '必须取消默认行为，否则浏览器会把文件当页面打开');
  fire('dragleave');
  assert.equal(overlay().classList.contains('hidden'), true);
  assert.equal(fire('dragover', { types: ['text/plain'] }).defaultPrevented, false);
});

test('拖入插件包：内容交给后端暂存 → 检查 → 确认框 → 取消后删除临时文件', async () => {
  const f = pkgFile('我的插件.nut');
  fire('dragenter');
  const drop = fire('drop', { files: [f] });
  assert.equal(drop.defaultPrevented, true);
  assert.equal(overlay().classList.contains('hidden'), true, '松手后提示层消失');
  await tick(60);

  const saved = m.called('save_dropped_package').pop();
  assert.ok(saved[1] instanceof Uint8Array && [...saved[1]].join() === '1,2,3,4', '以二进制方式传给后端');
  assert.equal(saved[2].headers['x-file-name'], encodeURIComponent('我的插件.nut'), '文件名经过编码，中文也能传');
  assert.deepEqual(m.called('inspect_plugin_package').pop()[1], { path: '/tmp/nuthusk-drop/我的插件.nut' });
  assert.ok(document.querySelector('.dlg-panel').textContent.includes('安装插件'));

  // 对话框还开着时再拖入一个：不会重复处理，也不会多存一份临时文件
  const savedBefore = m.state.saved.length;
  fire('drop', { files: [pkgFile('另一个.nut')] });
  await tick();
  assert.equal(m.state.saved.length, savedBefore);
  assert.ok($$('.toast').some((t) => t.textContent.includes('正在导入')));

  dlgBtn('取消').click();
  await tick(60);
  assert.deepEqual(m.state.discarded, ['/tmp/nuthusk-drop/我的插件.nut'], '处理完就删掉临时文件');
  assert.equal(m.called('install_plugin_package').filter((c) => c[1].path.includes('我的插件')).length, 0);
});

test('拖入插件包并确认安装：安装后同样删除临时文件', async () => {
  fire('drop', { files: [pkgFile('我的插件.nut')] });
  await tick(60);
  dlgBtn('安装').click();
  await tick(60);
  assert.deepEqual(m.called('install_plugin_package').pop()[1], { path: '/tmp/nuthusk-drop/我的插件.nut', sha256: 'a'.repeat(64) });
  assert.equal(m.state.discarded.length, 2);
});

test('拖入不是插件包的文件：提示，不暂存；混合时忽略多余的', async () => {
  const saved = m.state.saved.length;
  fire('drop', { files: [pkgFile('photo.png')] });
  await tick();
  assert.ok($$('.toast').some((t) => t.textContent.includes('只支持 .nut 或 .zip')));
  assert.equal(m.state.saved.length, saved);

  fire('drop', { files: [pkgFile('readme.txt'), pkgFile('我的插件.nut')] });
  await tick(60);
  assert.ok($$('.toast').some((t) => t.textContent.includes('已忽略 1 个不是插件包的文件')));
  assert.equal(m.state.saved.length, saved + 1);
  dlgBtn('取消').click();
  await tick(60);
});

test('拖入超大的文件：不会读进内存', async () => {
  const saved = m.state.saved.length;
  fire('drop', { files: [{ name: 'huge.nut', size: 400 * 1024 * 1024 }] });
  await tick();
  assert.ok($$('.toast').some((t) => t.textContent.includes('太大')));
  assert.equal(m.state.saved.length, saved);
});
