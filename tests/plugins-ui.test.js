// 示例插件自己的界面逻辑：用 jsdom 真正执行它们的 ui.html 里的脚本。
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';

function load(relPath, nuthusk = null) {
  const html = readFileSync(new URL(relPath, import.meta.url), 'utf8')
    .replace(/<script src="\/_sdk\/ui\.js"><\/script>/, '') // 真实环境里由宿主提供，这里用替身
    .replace(/<link[^>]*>/g, '');
  const dom = new JSDOM(html, { runScripts: 'dangerously', url: 'http://localhost/', pretendToBeVisual: true,
    beforeParse(w) { if (nuthusk) w.nuthusk = nuthusk; } });
  const w = dom.window;
  const $ = (id) => w.document.getElementById(id);
  return { w, $ };
}

test('JSON 格式化：格式化 / 压缩 / 校验 / 缩进 / 错误提示 / 清空', () => {
  const { $ } = load('../nutPlugins/json-formatter/ui.html');
  const click = (id) => $(id).click();

  $('input').value = '{"name":"nuthusk","list":[1,2],"ok":true}';
  click('fmt');
  assert.equal($('output').value, '{\n  "name": "nuthusk",\n  "list": [\n    1,\n    2\n  ],\n  "ok": true\n}');
  assert.ok($('status').classList.contains('ok') && $('status').textContent.includes('JSON 格式正确'));
  assert.ok($('status').textContent.includes('输入 41 字符') && $('status').textContent.includes('耗时'));

  $('indent').value = '4';
  click('fmt');
  assert.ok($('output').value.includes('\n    "name"'));
  $('indent').value = 'tab';
  click('fmt');
  assert.ok($('output').value.includes('\n\t"name"'));

  click('min');
  assert.equal($('output').value, '{"name":"nuthusk","list":[1,2],"ok":true}');

  const before = $('output').value;
  click('chk');
  assert.equal($('output').value, before, '校验不改动输出');
  assert.ok($('status').classList.contains('ok'));

  $('input').value = '{"broken": ';
  click('fmt');
  assert.ok($('status').classList.contains('err') && $('status').textContent.includes('不是合法的 JSON'));
  assert.equal($('output').value, '');

  $('input').value = '   ';
  click('fmt');
  assert.equal($('status').textContent, '请先在左侧输入 JSON');

  $('input').value = '[1]';
  click('fmt');
  click('clear');
  assert.equal($('input').value + $('output').value, '');
});

test('JSON 格式化：Ctrl+Enter 执行；没有内容时复制会提示', () => {
  const { w, $ } = load('../nutPlugins/json-formatter/ui.html');
  $('input').value = '[1,2]';
  $('input').dispatchEvent(new w.KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true }));
  assert.equal($('output').value, '[\n  1,\n  2\n]');
  $('output').value = '';
  $('copy').click();
  assert.equal($('status').textContent, '没有可复制的内容');
});

test('UUID 生成器：数量、格式、大写、上限', () => {
  const { $ } = load('../examples/src/uuid-gen/ui.html');
  const v4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
  assert.equal($('out').value.split('\n').length, 5, '打开时默认生成 5 个');
  $('n').value = '20';
  $('go').click();
  const lines = $('out').value.split('\n');
  assert.equal(lines.length, 20);
  assert.ok(lines.every((l) => v4.test(l)));
  assert.equal(new Set(lines).size, 20, '互不相同');
  $('upper').checked = true;
  $('go').click();
  assert.ok($('out').value.split('\n').every((l) => l === l.toUpperCase()));
  $('n').value = '99999';
  $('go').click();
  assert.equal($('out').value.split('\n').length, 500, '上限 500');
  $('n').value = 'abc';
  $('go').click();
  assert.equal($('out').value.split('\n').length, 1);
});

test('哈希计算：调用后端并显示结果；后端出错时显示错误', async () => {
  const calls = [];
  const { $ } = load('../nutPlugins/hash-calc/ui.html', {
    call: async (method, params) => {
      calls.push([method, params]);
      if (params.algo === 'md5') throw new Error('不支持的算法: md5');
      return 'deadbeef';
    },
  });
  $('text').value = 'abc';
  $('go').click();
  await new Promise((r) => setTimeout(r, 10));
  assert.equal(JSON.stringify(calls[0]), JSON.stringify(['hash', { text: 'abc', algo: 'sha256' }])); // 跨 jsdom/Node 环境，按 JSON 比较
  assert.equal($('out').textContent, 'deadbeef');
  assert.ok($('status').classList.contains('ok'));

  $('algo').value = 'md5';
  $('go').click();
  await new Promise((r) => setTimeout(r, 10));
  assert.equal($('out').textContent, '');
  assert.ok($('status').classList.contains('err') && $('status').textContent.includes('不支持的算法'));
});

test('字数统计（Python 后端）：显示统计结果和错误', async () => {
  const { $ } = load('../examples/src/py-wordcount/ui.html', {
    call: async (method, params) => {
      if (!params.text) throw new Error('没有文本');
      return { chars: 14, words: 3, lines: 2, bytes: 18 };
    },
  });
  $('text').value = '你好 world\nline2';
  $('go').click();
  await new Promise((r) => setTimeout(r, 10));
  assert.equal($('result').textContent, '14 字符 · 3 个单词 · 2 行 · 18 字节');
  $('text').value = '';
  $('go').click();
  await new Promise((r) => setTimeout(r, 10));
  assert.ok($('result').classList.contains('err'));
});

test('所有示例插件的页面都引入了统一样式和 SDK（以便跟随主题、转发快捷键）', () => {
  for (const p of ['../nutPlugins/json-formatter/ui.html', '../nutPlugins/hash-calc/ui.html', '../examples/src/uuid-gen/ui.html', '../examples/src/py-wordcount/ui.html']) {
    const html = readFileSync(new URL(p, import.meta.url), 'utf8');
    assert.ok(html.includes('href="/_sdk/ui.css"') && html.includes('src="/_sdk/ui.js"'), p);
  }
});
