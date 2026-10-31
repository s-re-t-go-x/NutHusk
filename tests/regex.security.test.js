import test from 'node:test';
import assert from 'node:assert/strict';
import { BrowserWorkerAdapter } from './regex-worker-adapter.js';
import { matchRegexTasks } from '../src/js/regex-matcher.js';
import { matchPluginsForTextAsync } from '../src/js/state.js';
import { openPalette, closePalette } from '../src/js/palette.js';
import { createDom, tick, mkPlugin } from './helpers.js';

const createWorker = () => new BrowserWorkerAdapter();

test('正则在独立线程执行：慢表达式被终止，后续规则继续，UI 定时器不被阻塞', async () => {
  let ticks = 0;
  const timer = setInterval(() => { ticks++; }, 5);
  const tasks = [
    { key: 'slow', pattern: '^(a+)+$' },
    { key: 'invalid', pattern: '[' },
    { key: 'later', pattern: 'a+!' },
  ];
  try {
    const matches = await matchRegexTasks(tasks, 'a'.repeat(28) + '!', { createWorker, timeoutMs: 40 });
    assert.deepEqual([...matches], ['later']);
    assert.ok(ticks >= 2, 'main event loop remains responsive');
  } finally { clearInterval(timer); }
});

test('完整识别保留正则语法、多个规则、长度限制与前缀匹配', async () => {
  const plugins = [
    mkPlugin('test.regex', 'Regex', { match: { regex: ['[', '^(?=hello)hello.*world$'], label: 'regex hit' } }),
    mkPlugin('test.prefix', 'Prefix', { match: { prefixes: ['hello'], label: 'prefix hit' } }),
    mkPlugin('test.short', 'Short', { match: { regex: ['hello'], maxLength: 3 } }),
    mkPlugin('test.off', 'Disabled', { enabled: false, match: { regex: ['hello'] } }),
  ];
  const matches = await matchPluginsForTextAsync(plugins, 'hello world', { createWorker });
  assert.deepEqual(matches.map((m) => m.plugin.id).sort(), ['test.prefix', 'test.regex']);
  assert.ok(matches.every((m) => m.specific));
});

test('输入取消立即结束正在匹配的 worker，不发布后续结果', async () => {
  const controller = new AbortController();
  const workers = [];
  let updates = 0;
  const promise = matchRegexTasks([{ key: 'slow', pattern: '^(a+)+$' }], 'a'.repeat(28) + '!', {
    signal: controller.signal, onMatch: () => { updates++; },
    createWorker: () => { const worker = createWorker(); workers.push(worker); return worker; },
  });
  controller.abort();
  assert.deepEqual([...(await promise)], []);
  await tick(60);
  assert.equal(updates, 0);
  assert.equal(workers.length, 1);
});

test('命令面板的纯正则插件可带内容打开，关闭和新查询不会被旧结果覆盖', async () => {
  const window = createDom();
  const previousWorker = globalThis.Worker;
  globalThis.Worker = BrowserWorkerAdapter;
  try {
    const opened = [];
    openPalette({ plugins: [mkPlugin('test.regex', 'Pattern tool', { match: { regex: ['^hello$'], label: 'matched' } })],
      onOpen: (...args) => opened.push(args) });
    const input = window.document.querySelector('.pal-input');
    input.value = 'hello';
    input.dispatchEvent(new window.Event('input'));
    const deadline = Date.now() + 2000;
    while (!window.document.querySelector('.pal-row') && Date.now() < deadline) await tick(10);
    assert.ok(window.document.querySelector('.pal-row').textContent.includes('matched'));
    input.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'Enter' }));
    assert.deepEqual(opened, [['test.regex', { text: 'hello' }]]);
    // Pressing Enter before the worker result arrives must still open a regex-only match.
    openPalette({ plugins: [mkPlugin('test.regex', 'Pattern tool', { match: { regex: ['^hello$'] } })],
      onOpen: (...args) => opened.push(args) });
    const fastInput = window.document.querySelector('.pal-input');
    fastInput.value = 'hello';
    fastInput.dispatchEvent(new window.Event('input'));
    fastInput.dispatchEvent(new window.KeyboardEvent('keydown', { key: 'Enter' }));
    const fastDeadline = Date.now() + 2000;
    while (opened.length < 2 && Date.now() < fastDeadline) await tick(10);
    assert.equal(opened.length, 2);
    assert.deepEqual(opened[1], ['test.regex', { text: 'hello' }]);


    openPalette({ plugins: [mkPlugin('test.regex', 'Pattern tool', { match: { regex: ['^hello$'] } })], onOpen() {} });
    const latestInput = window.document.querySelector('.pal-input');
    latestInput.value = 'hello';
    latestInput.dispatchEvent(new window.Event('input'));
    latestInput.value = 'different';
    latestInput.dispatchEvent(new window.Event('input'));
    await tick(100);
    assert.equal(window.document.querySelectorAll('.pal-row').length, 0);
    closePalette();
    await tick(30);
    assert.equal(window.document.querySelector('.pal-overlay'), null);
  } finally {
    closePalette();
    globalThis.Worker = previousWorker;
  }
});
