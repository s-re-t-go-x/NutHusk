import test from 'node:test';
import assert from 'node:assert/strict';
import * as S from '../src/js/state.js';

const P = (id, name, extra = {}) => ({
  id, name, description: '', category: '开发', keywords: [], enabled: true, error: null, ...extra,
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

test('loadListWithFallback：新键没有数据时沿用旧键', () => {
  const store = (data) => ({ getItem: (k) => data[k] ?? null });
  assert.deepEqual(S.loadListWithFallback(store({ new: '["a"]', old: '["b"]' }), 'new', 'old'), ['a']);
  assert.deepEqual(S.loadListWithFallback(store({ old: '["b"]' }), 'new', 'old'), ['b']);
  assert.deepEqual(S.loadListWithFallback(store({}), 'new', 'old'), []);
});
