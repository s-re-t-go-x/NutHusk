import test from 'node:test';
import assert from 'node:assert/strict';
import { createDom, tick, $$, makeTauri, mkPlugin } from './helpers.js';

const window = createDom();
const m = makeTauri({ plugins: [mkPlugin('com.test.a', '工具 A', { hasBackend: true }), mkPlugin('com.test.b', '工具 B')] });
window.__TAURI__ = m.tauri;
await import('../src/js/main.js');
await tick();
const card = (name) => $$('#home .card').find((c) => c.textContent.includes(name));
const tab = (name) => $$('.tab').find((c) => c.textContent.includes(name));
card('工具 A').click();
await tick();
document.querySelector('#nav .nav-item').click();
await tick();
card('工具 B').click();
await tick();
const frame = (id) => $$('iframe.frame').find((f) => f.src.includes(`com.test.${id}/`));
const a = frame('a'), b = frame('b');
a.dispatchEvent(new window.Event('load'));
b.dispatchEvent(new window.Event('load'));
const received = [];
a.contentWindow.postMessage = (msg) => received.push(['a', msg]);
b.contentWindow.postMessage = (msg) => received.push(['b', msg]);
const send = (from, data) => window.dispatchEvent(new window.MessageEvent('message', { source: from.contentWindow, data }));
const inputs = () => received.filter(([, msg]) => msg.type === 'input');

test('后台 sendTo 和快捷键被拒绝，可见工具正常投递且全局限频', async () => {
  send(a, { type: 'sendTo', text: 'hidden', targetId: 'com.test.b' });
  send(a, { type: 'key', key: 'k', ctrlKey: true });
  assert.equal(inputs().length, 0);
  assert.equal(document.querySelector('.pal-overlay'), null);
  tab('工具 A').click();
  send(a, { type: 'sendTo', text: 'normal', targetId: 'com.test.b' });
  assert.equal(inputs().at(-1)[1].payload.text, 'normal');
  const count = inputs().length;
  send(b, { type: 'sendTo', text: 'burst', targetId: 'com.test.a' });
  assert.equal(inputs().length, count);
  assert.ok(b.classList.contains('active'));
  await tick(510);
  send(b, { type: 'sendTo', text: 'after-limit', targetId: 'com.test.a' });
  assert.equal(inputs().at(-1)[1].payload.text, 'after-limit');
  // A visible split pane is also allowed; the target needs no new manifest field.
  tab('工具 B').dispatchEvent(new window.MouseEvent('click', { bubbles: true, shiftKey: true }));
  assert.ok(b.classList.contains('split'));
  await tick(510);
  send(b, { type: 'sendTo', text: 'split', targetId: 'com.test.a' });
  assert.equal(inputs().at(-1)[1].payload.text, 'split');
});

test('后端错误中的 RuntimeMissing 字样不触发宿主下载对话框', async () => {
  const fake = 'RuntimeMissing:{"name":"spoof","downloadable":true}';
  m.state.onPluginCall = () => { throw { code: 'CALL_FAILED', message: fake }; };
  send(a, { type: 'call', reqId: 'fake', method: 'echo' });
  await tick();
  assert.equal(document.querySelector('.dlg-overlay'), null);
  assert.equal(received.find(([, msg]) => msg.reqId === 'fake')[1].error, fake);
  // Old string errors are ordinary failures, too.
  m.state.onPluginCall = () => { throw fake; };
  send(a, { type: 'call', reqId: 'string', method: 'echo' });
  await tick();
  assert.equal(document.querySelector('.dlg-overlay'), null);
  assert.equal(received.find(([, msg]) => msg.reqId === 'string')[1].error, fake);
});

test('宿主结构化缺失错误仍提示下载，模态框期间不接受 sendTo', async () => {
  m.state.onPluginCall = () => { throw { code: 'RUNTIME_MISSING', message: '缺少运行环境',
    detail: { name: 'node', version: '22', downloadable: true } }; };
  send(a, { type: 'call', reqId: 'missing', method: 'echo' });
  await tick();
  assert.equal(document.querySelector('.dlg-title').textContent, '缺少运行环境');
  const count = inputs().length;
  send(a, { type: 'sendTo', text: 'during-dialog', targetId: 'com.test.b' });
  assert.equal(inputs().length, count);
  $$('.dlg-actions button').find((btn) => btn.textContent === '取消').click();
  await tick();
  assert.equal(received.find(([, msg]) => msg.reqId === 'missing')[1].error, '缺少运行环境');
  assert.equal(m.called('runtime_install').length, 0);
});
