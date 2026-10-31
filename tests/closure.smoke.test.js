import test from 'node:test';
import assert from 'node:assert/strict';
import { createDom, tick, $$, makeTauri, mkPlugin } from './helpers.js';
import { applyProfile } from '../src/js/profile.js';

const window = createDom();
localStorage.setItem('nuthusk.workspaces', JSON.stringify([
  { id: 'empty', name: '空工作区', tabs: ['home'], active: 'home' },
]));
const m = makeTauri({ plugins: [mkPlugin('com.test.backend', '后端工具', { hasBackend: true })] });
window.__TAURI__ = m.tauri;
await import('../src/js/main.js');
await tick();

test('切换工作区关闭标签时遵守 stopOnClose 设置', async () => {
  for (const stopOnClose of [true, false]) {
    m.state.settings.stopOnClose = stopOnClose;
    // Reload settings through a profile, using the same path as the app.
    const profileFile = new window.File([JSON.stringify({ format: 'nuthusk-profile', version: 1,
      settings: { stopOnClose }, plugins: [] })], 'settings.nutprofile');
    const drop = new window.Event('drop', { bubbles: true, cancelable: true });
    Object.defineProperty(drop, 'dataTransfer', { value: { types: ['Files'], files: [profileFile] } });
    window.dispatchEvent(drop);
    await tick();
    $$('.dlg-actions .btn').find((b) => b.textContent === '开始同步与恢复').click();
    await tick();
    $$('#home .card').find((c) => c.textContent.includes('后端工具')).click();
    await tick();
    const before = m.called('stop_plugin_backend').length;
    $$('#nav .nav-ws-item')[0].click();
    await tick();
    assert.equal($$('iframe.frame').length, 0);
    assert.equal(m.called('stop_plugin_backend').length - before, stopOnClose ? 1 : 0);
  }
});

test('配置恢复报告失败、取消安装和版本不符，并优先查找清单版本', async () => {
  const app = { plugins: [mkPlugin('com.test.existing', '现有工具')], settings: {}, runtimes: [] };
  const downloads = [];
  const invoke = async (cmd, args) => {
    if (cmd === 'market_sources') return [{ id: 'one', official: true }, { id: 'two', official: true }];
    if (cmd === 'runtime_install') throw new Error('offline');
    if (cmd === 'market_index') return { entries: [{ id: 'com.test.missing', version: args.sourceId === 'one' ? '2.0.0' : '1.0.0', url: `https://example.com/${args.sourceId}.nut` }] };
    if (cmd === 'market_download') { downloads.push(args); return '/tmp/package.nut'; }
    if (cmd === 'inspect_plugin_package') return { manifest: { id: 'com.test.missing', version: '1.0.0' } };
    throw new Error(cmd);
  };
  const result = applyProfile({ format: 'nuthusk-profile', version: 1, runtimes: ['python'], plugins: [
    { id: 'com.test.existing', version: '2.0.0' },
    { id: 'com.test.missing', version: '1.0.0' },
  ] }, { app, invoke, refresh: async () => {}, importPackages: async () => {} });
  await tick();
  $$('.dlg-actions .btn').find((b) => b.textContent === '开始同步与恢复').click();
  assert.equal(await result, false);
  assert.equal(downloads[0].url, 'https://example.com/two.nut');
  const summary = $$('.toast').at(-1).textContent;
  assert.match(summary, /部分同步/);
  assert.match(summary, /运行环境 python/);
  assert.match(summary, /未安装/);
  assert.match(summary, /保留当前版本/);
});
