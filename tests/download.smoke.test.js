import test from 'node:test';
import assert from 'node:assert/strict';
import { createDom, tick, $$, $, makeTauri, mkPlugin, mkPackage, trust } from './helpers.js';

const window = createDom();

const marketEntry = {
  id: 'com.example.markettool',
  name: '市场工具',
  version: '1.0.0',
  description: '来自市场的示例插件',
  author: 'NutHusk Team',
  category: '开发',
  url: 'https://example.com/markettool.nut',
  sha256: 'b'.repeat(64),
  size: 1024,
};

const m = makeTauri({
  plugins: [mkPlugin('com.example.local', '本地工具')],
  marketSources: [
    { id: 'official', name: '官方源', url: 'https://raw.githubusercontent.com/nuthusk/plugins/main/index.json', official: true },
    { id: 'custom', name: '私有源', url: 'https://custom.example.com/index.json', official: false },
  ],
  marketIndex: {
    source: { id: 'official', name: '官方源', url: 'https://raw.githubusercontent.com/nuthusk/plugins/main/index.json', official: true },
    name: '官方源',
    entries: [marketEntry],
    trust: trust.ok,
    fromCache: false,
    error: null,
  },
  packages: {
    '/tmp/downloaded.nut': mkPackage('com.example.markettool', '市场工具', '1.0.0'),
  },
  runtimes: [
    { name: 'python', version: '3.13.2', downloadable: true, onSystem: false, installed: null, size: 15000000, description: 'Python 3.13', source: 'python.org', userRecipe: false },
  ],
});
window.__TAURI__ = m.tauri;

const goDownload = async () => {
  $$('#nav-bottom .nav-item').find((b) => b.textContent.includes('插件下载')).click();
  await tick(60);
};

await import('../src/js/main.js');
await tick();

test('插件下载页：侧边栏入口切换、展示源选择、搜索与卡片', async () => {
  await goDownload();
  assert.equal($$('.tab.active .tab-title')[0].textContent, '插件下载');
  assert.ok($('download').textContent.includes('官方源'));
  assert.ok($('download').textContent.includes('市场工具'));
  assert.ok($('download').textContent.includes('来自市场的示例插件'));

  // 搜索
  const searchInput = $('download').querySelector('.msearch');
  searchInput.value = '不存在的词';
  searchInput.dispatchEvent(new window.Event('input'));
  assert.ok($('download').textContent.includes('没有符合条件的插件'));

  searchInput.value = '市场';
  searchInput.dispatchEvent(new window.Event('input'));
  assert.ok($('download').textContent.includes('市场工具'));
});

test('从市场下载并调起安装确认框', async () => {
  await goDownload();
  const installBtn = [...$('download').querySelectorAll('button')].find((b) => b.textContent === '下载安装');
  assert.ok(installBtn, '应有下载安装按钮');
  installBtn.click();
  await tick(80);

  // 应该调用了 market_download
  assert.equal(m.called('market_download').length, 1);
  assert.deepEqual(m.called('market_download')[0][1], {
    url: marketEntry.url,
    sha256: marketEntry.sha256,
    sourceId: 'official',
  });

  // 随后调起了 importPackages 的安全确认框
  assert.ok(document.querySelector('.dlg-panel'));
  assert.ok(document.querySelector('.dlg-panel').textContent.includes('市场工具'));

  // 取消安装
  $$('.dlg-actions .btn').find((b) => b.textContent === '取消').click();
  await tick();
  assert.equal(document.querySelector('.dlg-panel'), null);
});

test('运行环境缺失拦截与自动下载重试', async () => {
  // 打开本地工具
  $$('#nav .nav-item')[0].click();
  await tick();
  $$('#home .card').find((c) => c.querySelector('.card-name')?.textContent === '本地工具').click();
  await tick();

  const frame = $$('iframe.frame')[0];
  assert.ok(frame, '插件 iframe 应已创建');

  let postMessageResult = null;
  frame.contentWindow.postMessage = (msg) => {
    postMessageResult = msg;
  };

  // 模拟插件前端发出调用
  let callCount = 0;
  m.state.onPluginCall = async (args) => {
    callCount++;
    if (callCount === 1) {
      // 第一次调用缺少环境
      throw { code: 'RUNTIME_MISSING', message: '缺少运行环境：python', detail: {
        name: 'python', version: '3.13.2', downloadable: true, source: 'python.org', size: 15000000,
      } };
    }
    return { success: true, count: 42 };
  };

  // 发送插件 call 请求
  window.dispatchEvent(new window.MessageEvent('message', {
    data: { type: 'call', reqId: 'req_123', method: 'count', params: { text: 'hello' } },
    source: frame.contentWindow,
  }));
  await tick(60);

  // 应弹出运行环境缺失确认框
  const dlg = document.querySelector('.dlg-panel');
  assert.ok(dlg, '应弹出确认对话框');
  assert.ok(dlg.textContent.includes('缺少运行环境'));
  assert.ok(dlg.textContent.includes('python'));

  // 点击「立即下载并继续」
  const confirmBtn = [...dlg.querySelectorAll('button')].find((b) => b.textContent.includes('立即下载并继续'));
  assert.ok(confirmBtn);
  confirmBtn.click();
  await tick(100);

  // 应触发了 runtime_install
  assert.equal(m.called('runtime_install').length, 1);
  assert.deepEqual(m.called('runtime_install')[0][1], { name: 'python' });

  // 随后应自动无缝重试原 plugin_call，并将成功结果发回给插件
  assert.equal(callCount, 2, 'plugin_call 应自动重试第 2 次');
  assert.ok(postMessageResult);
  assert.equal(postMessageResult.reqId, 'req_123');
  assert.deepEqual(postMessageResult.result, { success: true, count: 42 });
});
