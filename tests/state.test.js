import test from 'node:test';
import assert from 'node:assert/strict';
import * as S from '../src/js/state.js';

const P = (id, name, extra = {}) => ({
  id, name, description: '', category: '开发', keywords: [], ui: 'ui.html', enabled: true, error: null, ...extra,
});

test('pushRecent 去重、置顶、限长', () => {
  assert.deepEqual(S.pushRecent(['a', 'b', 'c'], 'b'), ['b', 'a', 'c']);
  assert.equal(S.pushRecent(Array.from({ length: 8 }, (_, i) => 'x' + i), 'new').length, 8);
});

test('toggleItem 添加与移除', () => {
  assert.deepEqual(S.toggleItem(['a'], 'b'), ['a', 'b']);
  assert.deepEqual(S.toggleItem(['a', 'b'], 'a'), ['b']);
});

test('loadList 容忍坏数据', () => {
  const mk = (v) => ({ getItem: () => v });
  assert.deepEqual(S.loadList(mk('not json'), 'k'), []);
  assert.deepEqual(S.loadList(mk('{"a":1}'), 'k'), []);
  assert.deepEqual(S.loadList(mk('["a",1,"b"]'), 'k'), ['a', 'b']);
  assert.deepEqual(S.loadList(mk(null), 'k'), []);
  assert.deepEqual(S.loadList({ getItem: () => { throw new Error('x'); } }, 'k'), []);
});

test('closeTab：首页不可关；关闭当前标签切到右邻，否则左邻', () => {
  const tabs = [S.HOME, 'a', 'b', 'c'];
  assert.deepEqual(S.closeTab(tabs, 'a', S.HOME), { tabs, active: 'a' });
  assert.deepEqual(S.closeTab(tabs, 'b', 'b'), { tabs: [S.HOME, 'a', 'c'], active: 'c' });
  assert.deepEqual(S.closeTab(tabs, 'c', 'c'), { tabs: [S.HOME, 'a', 'b'], active: 'b' });
  assert.deepEqual(S.closeTab(tabs, 'a', 'c'), { tabs: [S.HOME, 'a', 'b'], active: 'a' });
  assert.deepEqual(S.closeTab([S.HOME, 'a'], 'a', 'a'), { tabs: [S.HOME], active: S.HOME });
  assert.deepEqual(S.closeTab(tabs, 'a', 'zzz'), { tabs, active: 'a' });
});

test('moveTab：拖拽调整标签顺序，首页固定在第一位', () => {
  const tabs = [S.HOME, 'a', 'b', 'c'];
  assert.deepEqual(S.moveTab(tabs, 'c', 'a'), [S.HOME, 'c', 'a', 'b']);
  assert.deepEqual(S.moveTab(tabs, 'a', 'c'), [S.HOME, 'b', 'c', 'a']);
  assert.deepEqual(S.moveTab(tabs, S.HOME, 'b'), tabs, '首页不可移动');
  assert.deepEqual(S.moveTab(tabs, 'b', S.HOME), tabs, '不可移到首页之前');
});

test('openTab 不重复；cycleTab 循环', () => {
  assert.deepEqual(S.openTab([S.HOME, 'a'], 'a'), [S.HOME, 'a']);
  assert.deepEqual(S.openTab([S.HOME], 'a'), [S.HOME, 'a']);
  const tabs = [S.HOME, 'a', 'b'];
  assert.equal(S.cycleTab(tabs, 'b', 1), S.HOME);
  assert.equal(S.cycleTab(tabs, S.HOME, -1), 'b');
});

test('搜索：名称前缀 > 包含 > 关键词 > 描述，并排除禁用/出错的插件', () => {
  const plugins = [
    P('x.json', 'JSON 格式化', { keywords: ['json'] }),
    P('x.hash', '哈希计算', { keywords: ['hash', 'md5'], description: '计算 json 的哈希' }),
    P('x.off', 'JSON 对比', { enabled: false }),
    P('x.err', 'JSON 校验', { error: 'bad' }),
  ];
  assert.deepEqual(S.searchPlugins(plugins, 'json').map((p) => p.id), ['x.json', 'x.hash']);
  assert.deepEqual(S.searchPlugins(plugins, 'md5').map((p) => p.id), ['x.hash']);
  assert.deepEqual(S.searchPlugins(plugins, '不存在'), []);
});

test('搜索：空查询时最近使用排最前，其次收藏', () => {
  const plugins = [P('a', 'A'), P('b', 'B'), P('c', 'C'), P('d', 'D')];
  const ids = S.searchPlugins(plugins, '', { favorites: ['d'], recent: ['c'] }).map((p) => p.id);
  assert.deepEqual(ids, ['c', 'd', 'a', 'b']);
});

test('filterPlugins：支持 running 筛选运行中的后端', () => {
  const plugins = [
    P('a', 'A', { backendRunning: true }),
    P('b', 'B', { backendRunning: false }),
  ];
  assert.deepEqual(S.filterPlugins(plugins, '', 'running').map((p) => p.id), ['a']);
});

test('matchPluginsForText 与 matchPluginsForFile：智能识别输入文本和文件扩展名', () => {
  const plugins = [
    P('x.json', 'JSON 格式化', {
      match: { regex: ['^\\s*[\\{\\[][\\s\\S]*[\\}\\]]\\s*$'], prefixes: ['{', '['], extensions: ['json', 'geojson'], minLength: 2, label: '识别为 JSON 文本' },
    }),
    P('x.hash', '哈希计算', {
      match: { minLength: 1, maxLength: 100, label: '计算文本哈希' },
    }),
    P('x.off', '已禁用', { enabled: false, match: { prefixes: ['{'] } }),
  ];

  const jsonMatch = S.matchPluginsForText(plugins, '{"hello": 1}');
  assert.equal(jsonMatch[0].plugin.id, 'x.json');
  assert.equal(jsonMatch[0].specific, true);
  assert.equal(jsonMatch[0].label, '识别为 JSON 文本');
  assert.equal(jsonMatch[1].plugin.id, 'x.hash');
  assert.equal(jsonMatch[1].specific, false);

  const plainMatch = S.matchPluginsForText(plugins, 'hello world');
  assert.deepEqual(plainMatch.map((m) => m.plugin.id), ['x.hash']);

  const fileMatch = S.matchPluginsForFile(plugins, 'data.GeoJSON');
  assert.deepEqual(fileMatch.map((p) => p.id), ['x.json']);
  assert.deepEqual(S.matchPluginsForFile(plugins, 'image.png'), []);
});

test('loadListWithFallback：新键没有数据时沿用旧键', () => {
  const store = (data) => ({ getItem: (k) => data[k] ?? null });
  assert.deepEqual(S.loadListWithFallback(store({ new: '["a"]', old: '["b"]' }), 'new', 'old'), ['a']);
  assert.deepEqual(S.loadListWithFallback(store({ old: '["b"]' }), 'new', 'old'), ['b']);
  assert.deepEqual(S.loadListWithFallback(store({}), 'new', 'old'), []);
});

test('场景工作区（Workspaces）与流水线（Workflows）增删改查与动作提取', () => {
  let wsList = [];
  wsList = S.upsertWorkspace(wsList, {
    id: 'ws-1',
    name: '前端联调',
    tabs: ['com.example.json-formatter', 'com.example.hash-calc'],
    active: 'com.example.json-formatter',
    splitId: 'com.example.hash-calc',
  });
  assert.equal(wsList.length, 1);
  assert.deepEqual(wsList[0].tabs, ['home', 'com.example.json-formatter', 'com.example.hash-calc']);
  assert.equal(wsList[0].splitId, 'com.example.hash-calc');

  // 同名更新不新增条目
  wsList = S.upsertWorkspace(wsList, {
    id: 'ws-new',
    name: '前端联调',
    tabs: ['com.example.json-formatter'],
    active: 'com.example.json-formatter',
    splitId: null,
  });
  assert.equal(wsList.length, 1);
  assert.equal(wsList[0].id, 'ws-1');
  assert.equal(wsList[0].splitId, null);

  wsList = S.removeWorkspace(wsList, 'ws-1');
  assert.equal(wsList.length, 0);

  let wfList = [];
  wfList = S.upsertWorkflow(wfList, {
    id: 'wf-1',
    name: 'JSON 转 SHA256',
    description: 'JSON 格式化 → 计算 SHA-256',
    steps: [
      { pluginId: 'com.example.json-formatter', actionId: 'format' },
      { pluginId: 'com.example.hash-calc', actionId: 'sha256' },
    ],
  });
  assert.equal(wfList.length, 1);
  assert.equal(wfList[0].steps.length, 2);

  const plugins = [
    P('com.example.json-formatter', 'JSON 格式化', { actions: [{ id: 'format', name: 'JSON 格式化' }] }),
    P('com.example.off', '禁用工具', { enabled: false, actions: [{ id: 'x', name: 'X' }] }),
  ];
  const acts = S.listAvailableActions(plugins);
  assert.equal(acts.length, 1);
  assert.equal(acts[0].key, 'com.example.json-formatter:format');
});
