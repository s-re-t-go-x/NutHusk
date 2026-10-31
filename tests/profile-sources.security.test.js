import test from 'node:test';
import assert from 'node:assert/strict';
import { applyProfile } from '../src/js/profile.js';
import { createDom, tick, $$ } from './helpers.js';
createDom();
const official = { id: 'official', name: '官方源', official: true, url: 'https://official.example/index.json' };
const privateSource = { id: 'private', name: '自称官方', official: false, url: 'https://private.example/index.json' };
const entry = (version = '1.0.0') => ({ id: 'com.test.tool', version, url: `https://packages.example/${version}.nut`, sha256: 'a'.repeat(64) });
const click = (label) => $$('.dlg-actions button').find((btn) => btn.textContent === label).click();
function start({ sources = [official], indexes = {}, profile = {}, actual = {}, failDownload = false } = {}) {
  const calls = [], imported = [];
  const app = { plugins: [], settings: {}, runtimes: [{ name: 'node', onSystem: true }] };
  let currentSources = [...sources];
  const invoke = async (cmd, args) => {
    calls.push([cmd, args]);
    if (cmd === 'market_sources') return [...currentSources];
    if (cmd === 'source_add') { const added = { ...args, id: 'added', official: false }; currentSources.push(added); return added; }
    if (cmd === 'market_index') return { entries: indexes[args.sourceId] ?? [] };
    if (cmd === 'market_download') { if (failDownload) throw 'offline'; return '/tmp/review.nut'; }
    if (cmd === 'inspect_plugin_package') return { manifest: { id: 'com.test.tool', version: '1.0.0', ...actual } };
    if (cmd === 'discard_dropped_package') return;
    throw new Error(cmd);
  };
  const result = applyProfile({ format: 'nuthusk-profile', version: 1,
    plugins: [{ id: 'com.test.tool', version: '1.0.0' }], ...profile }, {
    app, invoke, refresh: async () => {}, importPackages: async (items) => {
      imported.push(...items); app.plugins = [{ id: 'com.test.tool', version: '1.0.0' }];
    },
  });
  return { calls, imported, result, called: (name) => calls.filter(([cmd]) => cmd === name) };
}

test('相同 ID 和版本优先官方源，不受源列表原顺序影响', async () => {
  const run = start({ sources: [privateSource, official], indexes: { private: [entry()], official: [entry()] } });
  await tick(); click('开始同步与恢复');
  assert.equal(await run.result, true);
  assert.equal(run.called('market_download')[0][1].sourceId, 'official');
  assert.equal(run.imported.length, 1);
});

test('指定版本不存在时不回退、不下载其他版本', async () => {
  const run = start({ indexes: { official: [entry('2.0.0')] } });
  await tick(); click('开始同步与恢复');
  assert.equal(await run.result, false);
  assert.equal(run.called('market_download').length, 0);
  assert.equal(run.imported.length, 0);
});

test('清单新增源展示完整 URL 和来源标记，拒绝来源确认后不下载', async () => {
  const run = start({ profile: { sources: [privateSource] }, indexes: { added: [entry()] } });
  await tick();
  assert.ok(document.querySelector('.dlg-body').textContent.includes(privateSource.url));
  assert.match(document.querySelector('.dlg-body').textContent, /由本次配置清单添加/);
  click('开始同步与恢复'); await tick();
  const body = document.querySelector('.dlg-body').textContent;
  assert.ok(body.includes(privateSource.url));
  assert.ok(body.includes(entry().url));
  assert.match(body, /此源由本次配置清单添加/);
  assert.equal(run.called('market_download').length, 0);
  click('跳过此插件');
  assert.equal(await run.result, false);
  assert.equal(run.called('market_download').length, 0);
});

test('私有源已确认仍核对包内版本，不符则丢弃且不安装', async () => {
  const run = start({ sources: [privateSource], indexes: { private: [entry()] }, actual: { version: '2.0.0' } });
  await tick(); click('开始同步与恢复'); await tick(); click('确认来源并下载');
  assert.equal(await run.result, false);
  assert.equal(run.called('discard_dropped_package').length, 1);
  assert.equal(run.imported.length, 0);
});

test('清单直链需要独立确认，包内 ID 不符时拒绝导入', async () => {
  const url = 'https://direct.example/custom.nut';
  const run = start({ profile: { plugins: [{ id: 'com.test.tool', version: '1.0.0', url }] },
    indexes: { official: [entry()] }, actual: { id: 'com.test.other' } });
  await tick(); click('开始同步与恢复'); await tick();
  assert.ok(document.querySelector('.dlg-body').textContent.includes(url));
  click('确认来源并下载');
  assert.equal(await run.result, false);
  assert.equal(run.called('market_download')[0][1].sha256, null, '不挪用另一个源条目的哈希');
  assert.equal(run.imported.length, 0);
  assert.equal(run.called('discard_dropped_package').length, 1);
});
