import test from 'node:test';
import assert from 'node:assert/strict';
import { createDom, tick, $$, $, key, makeTauri, mkPlugin, trust } from './helpers.js';

const window = createDom();
const m = makeTauri({
  plugins: [
    mkPlugin('com.example.json-formatter', 'JSON 格式化', {
      keywords: ['json'], icon: 'icon.svg', trust: trust.ok,
      match: { regex: ['^\\s*[\\{\\[][\\s\\S]*[\\}\\]]\\s*$'], prefixes: ['{', '['], extensions: ['json'], minLength: 2, label: '识别为 JSON 文本' },
      actions: [{ id: 'format', name: 'JSON 格式化', method: 'format', defaultParams: { indent: 2 } }],
    }),
    mkPlugin('com.example.hash-calc', '哈希计算', {
      keywords: ['hash'], permissions: ['network'], hasBackend: true, backendCommand: 'node main.js',
      match: { minLength: 1, maxLength: 10000, label: '计算文本哈希' },
      actions: [{ id: 'sha256', name: '计算 SHA-256', method: 'hash', defaultParams: { algo: 'sha256' } }],
    }),
    mkPlugin('com.example.pic', '图片压缩', { category: '图片' }),
    mkPlugin('com.example.future', '未来插件', { category: '其他', error: '需要宿主 API ^2.0.0，当前为 1.0.0', icon: null }),
  ],
});
window.__TAURI__ = m.tauri;
const cardOf = (name) => $$('#home .card').find((c) => c.querySelector('.card-name')?.textContent === name);
const navItem = (label) => $$('#nav .nav-item').find((n) => n.querySelector('.nav-label').textContent === label);

await import('../src/js/main.js');
await tick();

test('首页：侧边栏、卡片、信任标识、权限标签、导入卡片', () => {
  const labels = $$('#nav .nav-label').map((n) => n.textContent);
  assert.deepEqual(labels.slice(0, 3), ['首页', '收藏', '最近使用']);
  assert.deepEqual(labels.slice(3).sort(), ['其他', '图片', '开发'].sort());
  assert.equal($$('#home .card:not(.import)').length, 4);
  assert.equal($$('#home .card.import').length, 1);
  assert.ok(cardOf('JSON 格式化').textContent.includes('可信来源'));
  assert.ok(cardOf('JSON 格式化').querySelector('.trust-trusted'));
  assert.ok(cardOf('哈希计算').textContent.includes('未经安全验证'));
  assert.ok(cardOf('哈希计算').querySelector('.trust-unverified'));
  assert.ok(cardOf('哈希计算').textContent.includes('访问网络'));
  assert.ok(cardOf('未来插件').classList.contains('error'));
  assert.ok(cardOf('未来插件').textContent.includes('需要宿主 API'));
  assert.deepEqual($$('#nav-bottom .nav-label').map((n) => n.textContent), ['插件下载', '插件管理', '设置']);
  assert.ok($('nav-bottom').textContent.includes('⚠ 1'), '有插件加载失败时在管理入口提示');
});

test('开关是 switch 样式，且带无障碍属性', () => {
  const sw = cardOf('哈希计算').querySelector('label.switch');
  assert.ok(sw && sw.querySelector('input[type=checkbox][role=switch]'));
  assert.ok(sw.querySelector('.slider'));
  assert.equal(cardOf('未来插件').querySelector('label.switch input').disabled, true);
});

test('插件图标：有 icon 显示图片，加载失败时退回首字母', async () => {
  const badge = cardOf('JSON 格式化').querySelector('.badge');
  const img = badge.querySelector('img.badge-img');
  assert.ok(badge.classList.contains('has-img'));
  assert.equal(img.getAttribute('src'), 'plugin://localhost/com.example.json-formatter/icon.svg');
  img.dispatchEvent(new window.Event('error'));
  assert.ok(!badge.classList.contains('has-img'));
  assert.equal(badge.textContent, 'J');
  assert.equal(cardOf('图片压缩').querySelector('.badge img'), null, '没有 icon 时是首字母');
});

test('状态栏：左侧插件数量，右侧应用版本', () => {
  const st = $('status').textContent;
  assert.ok(st.includes('已加载 4 个插件'), st);
  assert.ok(st.includes('可用 3') && st.includes('1 个加载失败'), st);
  assert.ok(st.includes('NutHusk v0.1.0'), st);
});

test('出错的插件点击不会打开标签', async () => {
  cardOf('未来插件').click();
  await tick();
  assert.equal($$('.tab').length, 1);
  assert.equal($$('.toast').length, 1);
});

test('点击卡片：新标签 + 沙箱 iframe + 工具页顶栏 + 状态栏显示当前插件', async () => {
  cardOf('JSON 格式化').click();
  await tick();
  assert.equal($$('.tab').length, 2);
  const frames = $$('iframe.frame');
  assert.equal(frames.length, 1);
  assert.equal(frames[0].getAttribute('sandbox'), 'allow-scripts');
  assert.equal(frames[0].src, 'plugin://localhost/com.example.json-formatter/ui.html');
  assert.ok(frames[0].classList.contains('active'));
  assert.ok($('home').classList.contains('hidden'));
  const head = $('toolhead');
  assert.ok(!head.classList.contains('hidden'));
  assert.ok(head.textContent.includes('JSON 格式化') && head.textContent.includes('JSON 格式化 的说明'));
  assert.ok(head.querySelector('.trust-trusted'));
  assert.ok($('status').textContent.includes('JSON 格式化 v1.0.0'));
});

test('iframe 加载完成后，宿主把当前主题发给它', async () => {
  const frame = $$('iframe.frame')[0];
  const sent = [];
  frame.contentWindow.postMessage = (msg) => sent.push(msg);
  frame.dispatchEvent(new window.Event('load'));
  assert.deepEqual(sent, [{ type: 'theme', theme: 'system' }]);
});

test('工具页顶栏的星标可以收藏，状态保存在 nuthusk.* 键下', async () => {
  $('toolhead').querySelector('.star').click();
  await tick();
  assert.deepEqual(JSON.parse(localStorage.getItem('nuthusk.favorites')), ['com.example.json-formatter']);
  assert.ok($('toolhead').querySelector('.star.on'));
  assert.equal(localStorage.getItem('toolbox.favorites'), null, '不再写旧的键');
});

test('再次打开同一工具：聚焦已有标签，不重复创建', async () => {
  navItem('首页').click();
  await tick();
  assert.ok(!$('home').classList.contains('hidden'));
  assert.ok($('toolhead').classList.contains('hidden'), '首页不显示工具顶栏');
  assert.ok(!$$('iframe.frame')[0].classList.contains('active'), '切走后 iframe 保留但隐藏');
  cardOf('JSON 格式化').click();
  await tick();
  assert.equal($$('.tab').length, 2);
  assert.equal($$('iframe.frame').length, 1);
});

test('桥接：iframe 的 call 转发到 plugin_call，结果与错误都能回传；来源不明的消息被忽略', async () => {
  const frame = $$('iframe.frame')[0];
  const sent = [];
  frame.contentWindow.postMessage = (msg) => sent.push(msg);
  const post = (data, source = frame.contentWindow) => window.dispatchEvent(new window.MessageEvent('message', { data, source }));

  post({ type: 'call', reqId: 1, method: 'format', params: { a: 1 } });
  post({ type: 'call', reqId: 2, method: 'boom', params: null });
  await tick();
  assert.deepEqual(m.called('plugin_call').map((c) => c[1]), [
    { id: 'com.example.json-formatter', method: 'format', params: { a: 1 } },
    { id: 'com.example.json-formatter', method: 'boom', params: null },
  ]);
  assert.deepEqual(sent, [{ reqId: 1, result: 'RESULT:format' }, { reqId: 2, error: 'plugin exploded' }]);

  const before = m.calls.length;
  post({ type: 'call', reqId: 9, method: 'x' }, window);
  await tick();
  assert.equal(m.calls.length, before);
});

test('Ctrl+K 打开搜索面板，过滤后回车打开；面板里没有出错的插件', async () => {
  key(window, 'k', { ctrlKey: true });
  assert.equal($$('.pal-overlay').length, 1);
  assert.equal($$('.pal-row').length, 3);
  assert.ok(!$$('.pal-name').some((n) => n.textContent === '未来插件'));
  const input = document.querySelector('.pal-input');
  input.value = 'hash';
  input.dispatchEvent(new window.Event('input'));
  assert.equal($$('.pal-row').length, 1);
  key(input, 'Enter');
  await tick();
  assert.equal($$('.tab.active .tab-title')[0].textContent, '哈希计算');
});

test('智能内容识别 + 跨工具投递（sendTo） + 左右双栏分屏联动', async () => {
  const jsonFrame = $$('iframe.frame').find((f) => f.src.includes('json-formatter'));
  const hashFrame = $$('iframe.frame').find((f) => f.src.includes('hash-calc'));
  const jsonReceived = [];
  const hashReceived = [];
  jsonFrame.contentWindow.postMessage = (msg) => jsonReceived.push(msg);
  hashFrame.contentWindow.postMessage = (msg) => hashReceived.push(msg);
  hashFrame.dispatchEvent(new window.Event('load'));
  hashReceived.length = 0;

  // 1. 在 Ctrl+K 面板粘贴一段 JSON，智能识别并带内容打开 JSON 格式化
  key(window, 'k', { ctrlKey: true });
  const input = document.querySelector('.pal-input');
  input.value = '{"hello":"world"}';
  input.dispatchEvent(new window.Event('input'));
  assert.ok($$('.pal-row')[0].textContent.includes('识别为 JSON 文本 · 带内容打开'));
  key(input, 'Enter');
  await tick();
  assert.equal($$('.tab.active .tab-title')[0].textContent, 'JSON 格式化');
  assert.deepEqual(
    jsonReceived.find((msg) => msg.type === 'input'),
    { type: 'input', payload: { text: '{"hello":"world"}' } },
  );

  // 2. 开启左右双栏分屏（Shift+点击「哈希计算」标签）
  const hashTab = $$('.tab').find((t) => t.textContent.includes('哈希计算'));
  hashTab.dispatchEvent(new window.MouseEvent('click', { bubbles: true, shiftKey: true }));
  await tick();
  assert.ok($('stage').classList.contains('split-mode'), '舞台进入左右双栏分屏模式');
  assert.ok(hashFrame.classList.contains('split'), '右侧分屏展示哈希计算');
  assert.ok($('toolhead').textContent.includes('右侧：哈希计算'));

  // 3. 从左侧 JSON 格式化点击「投递结果」（触发 sendTo 消息），面板置顶推荐「直接投递到右侧分屏」
  window.dispatchEvent(new window.MessageEvent('message', {
    data: { type: 'sendTo', text: '{\n  "hello": "world"\n}' },
    source: jsonFrame.contentWindow,
  }));
  assert.equal($$('.pal-overlay').length, 1);
  assert.ok(document.querySelector('.pal-banner').textContent.includes('来自：JSON 格式化'));
  assert.ok($$('.pal-row')[0].textContent.includes('直接投递到右侧分屏'));
  key(document.querySelector('.pal-input'), 'Enter');
  await tick();
  assert.deepEqual(hashReceived.pop(), {
    type: 'input',
    payload: { text: '{\n  "hello": "world"\n}', sourceId: 'com.example.json-formatter', sourceName: 'JSON 格式化' },
  });

  // 4. 退出双栏分屏
  key(window, '\\', { ctrlKey: true });
  await tick();
  assert.equal($('stage').classList.contains('split-mode'), false);
});

test('开发热重载（Ctrl+R）、保存/切换场景工作区与快捷流水线组合', async () => {
  // 1. 当前处于「JSON 格式化」标签，按 Ctrl+R 触发开发热重载
  key(window, 'r', { ctrlKey: true });
  await tick();
  assert.deepEqual(m.called('reload_single_plugin').pop()[1], { id: 'com.example.json-formatter' });
  const jsonFrame = $$('iframe.frame').find((f) => f.src.includes('json-formatter'));
  assert.ok(jsonFrame.src.includes('?t='), '热重载追加时间戳刷新 iframe');
  jsonFrame.dispatchEvent(new window.Event('load'));

  // 2. 保存当前标签布局为场景工作区「接口调试」
  document.querySelector('.nav-group-btn').click();
  await tick();
  const wsInput = document.querySelector('.dlg-panel input');
  wsInput.value = '接口调试';
  $$('.dlg-actions .btn').find((b) => b.textContent === '保存工作区').click();
  await tick();
  assert.ok($$('#nav .nav-ws-item').some((el) => el.textContent.includes('接口调试')));

  // 3. 切回首页，新建一条「JSON 格式化 → 计算 SHA-256」流水线并运行
  navItem('首页').click();
  await tick();
  const addWfBtn = $$('#home .sec-head .btn').find((b) => b.textContent.includes('新建流水线'));
  assert.ok(addWfBtn);
  addWfBtn.click();
  await tick();
  $$('.dlg-actions .btn').find((b) => b.textContent === '保存流水线').click();
  await tick();
  assert.equal($$('#home .wf-card').length, 1);

  // 模拟前端插件 JSON 格式化处理 action 消息
  jsonFrame.contentWindow.postMessage = (msg) => {
    if (msg.type === 'action') {
      window.dispatchEvent(new window.MessageEvent('message', {
        data: { type: 'actionResult', reqId: msg.reqId, result: '{\n  "ok": 1\n}' },
        source: jsonFrame.contentWindow,
      }));
    }
  };

  $$('#home .wf-card')[0].click();
  await tick();
  const runnerInput = document.querySelector('.wf-runner textarea');
  runnerInput.value = '{"ok":1}';
  $$('.wf-actions-bar .btn').find((b) => b.textContent.includes('执行流水线')).click();
  await tick(50);
  const runnerOutput = $$('.wf-runner textarea')[1];
  assert.equal(runnerOutput.value, 'RESULT:hash');

  // 4. 在流水线弹窗内点击「投递到工具…」：按 Esc 仅关闭投递面板而不关闭流水线弹窗；选择目标工具后自动关闭流水线弹窗并打开目标工具
  const sendBtn = $$('.wf-actions-bar .btn').find((b) => b.textContent.includes('投递到工具'));
  sendBtn.click();
  await tick();
  assert.equal($$('.pal-overlay').length, 1);
  assert.equal($$('.dlg-overlay').length, 1);
  key(document.querySelector('.pal-input'), 'Escape');
  await tick();
  assert.equal($$('.pal-overlay').length, 0, 'Esc 仅关闭顶层投递面板');
  assert.equal($$('.dlg-overlay').length, 1, '流水线对话框保持打开');

  sendBtn.click();
  await tick();
  const palInput = document.querySelector('.pal-input');
  palInput.value = 'JSON';
  palInput.dispatchEvent(new window.Event('input'));
  key(palInput, 'Enter');
  await tick();
  assert.equal($$('.pal-overlay').length, 0);
  assert.equal($$('.dlg-overlay').length, 0, '选择投递目标后自动关闭流水线对话框');
  assert.equal($$('.tab.active .tab-title')[0].textContent, 'JSON 格式化');
});

test('快捷键：Ctrl+数字切换、Ctrl+W 关闭并移除 iframe、首页不可关；SDK 转发的快捷键也有效', async () => {
  key(window, '2', { ctrlKey: true });
  await tick();
  assert.equal($$('.tab.active .tab-title')[0].textContent, 'JSON 格式化');
  key(window, 'w', { ctrlKey: true });
  await tick();
  assert.equal($$('.tab').length, 2);
  assert.equal($$('iframe.frame').length, 1);
  key(window, '1', { ctrlKey: true });
  key(window, 'w', { ctrlKey: true });
  assert.equal($$('.tab').length, 2, '首页不会被关掉');

  const frame = $$('iframe.frame')[0];
  window.dispatchEvent(new window.MessageEvent('message', { data: { type: 'key', key: 'k', ctrlKey: true }, source: frame.contentWindow }));
  assert.equal($$('.pal-overlay').length, 1);
  key(document.querySelector('.pal-input'), 'Escape');
});

test('顶栏窗口置顶按钮与后端进程状态管控（启动、显示 PID、关闭标签自动回收）', async () => {
  // 切换窗口置顶
  assert.equal($('pin-toggle').classList.contains('active'), false);
  $('pin-toggle').click();
  await tick();
  assert.deepEqual(m.called('set_always_on_top').pop()[1], { onTop: true });
  assert.equal($('pin-toggle').classList.contains('active'), true);

  // 切到哈希计算标签并触发一次后端调用
  key(window, '2', { ctrlKey: true });
  await tick();
  const hashFrame = $$('iframe.frame')[0];
  window.dispatchEvent(new window.MessageEvent('message', {
    data: { type: 'call', reqId: 10, method: 'hash', params: { text: 'hi' } },
    source: hashFrame.contentWindow,
  }));
  await tick(50);

  // 工具顶栏和状态栏立即显示后端运行中与 PID
  assert.ok($('toolhead').textContent.includes('运行中 · PID 4321'));
  assert.ok($('status').textContent.includes('1 个后端运行中'));

  // 关闭标签页时（默认 stopOnClose=true）自动结束该插件的后端进程
  key(window, 'w', { ctrlKey: true });
  await tick(50);
  assert.deepEqual(m.called('stop_plugin_backend').pop()[1], { id: 'com.example.hash-calc' });
  assert.ok(!$('status').textContent.includes('后端运行中'));
});

test('禁用正在使用的插件：标签与 iframe 一并关闭；已禁用的不能打开', async () => {
  navItem('首页').click();
  await tick();
  cardOf('哈希计算').click();
  await tick();
  navItem('首页').click();
  await tick();
  assert.equal($$('iframe.frame').length, 1);
  cardOf('哈希计算').querySelector('label.switch input').click();
  await tick();
  assert.deepEqual(m.called('set_plugin_enabled').pop()[1], { id: 'com.example.hash-calc', enabled: false });
  assert.equal($$('.tab').length, 1);
  assert.equal($$('iframe.frame').length, 0);
  assert.ok(cardOf('哈希计算').classList.contains('off'));
  const toasts = $$('.toast').length;
  cardOf('哈希计算').click();
  await tick();
  assert.equal($$('.tab').length, 1);
  assert.equal($$('.toast').length, toasts + 1);
});
