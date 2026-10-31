import test from 'node:test';
import assert from 'node:assert/strict';
import { createDom } from './helpers.js';
import { createDownloadState } from '../src/js/download.js';
import { createDownloadActions } from '../src/js/download-actions.js';

createDom();
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((ok, fail) => { resolve = ok; reject = fail; });
  return { promise, resolve, reject };
};

test('切换插件源时，较早发出的请求不能覆盖当前源，也不能提前结束加载', async () => {
  const state = createDownloadState();
  const requests = { first: deferred(), second: deferred(), third: deferred(), fourth: deferred() };
  const actions = createDownloadActions({ state, render() {},
    invoke: async (_, { sourceId }) => requests[sourceId].promise });
  const first = actions.loadMarketSource('first');
  const second = actions.loadMarketSource('second');
  requests.first.resolve({ entries: [{ id: 'old' }] });
  await first;
  assert.equal(state.loading, true);
  assert.equal(state.index, null);
  requests.second.resolve({ entries: [{ id: 'current' }] });
  await second;
  assert.equal(state.loading, false);
  assert.equal(state.index.entries[0].id, 'current');

  const stale = actions.loadMarketSource('third');
  const latest = actions.loadMarketSource('fourth');
  requests.fourth.resolve({ entries: [{ id: 'latest' }] });
  await latest;
  requests.third.reject(new Error('offline'));
  await stale;
  assert.equal(state.selectedSource, 'fourth');
  assert.equal(state.index.entries[0].id, 'latest');
  assert.equal(state.error, null);
});

test('市场与直链共用下载流程：保留来源参数，确认安装前清理进度', async () => {
  const state = createDownloadState();
  state.selectedSource = 'custom';
  const calls = [];
  let imported = 0;
  const actions = createDownloadActions({ state, render() {}, refresh() {},
    invoke: async (cmd, args) => {
      calls.push([cmd, args]);
      assert.ok(state.downloading.has(`pkg:${args.url}`));
      return '/tmp/downloaded.nut';
    },
    importPackages: async (paths) => {
      assert.equal(state.downloading.size, 0);
      assert.deepEqual(paths, ['/tmp/downloaded.nut']);
      imported++;
    },
  });
  const entry = { url: 'https://example.com/market.nut', sha256: 'a'.repeat(64), size: 42 };
  await actions.installFromMarket(entry);
  await actions.installDirect('https://example.com/direct.nut');
  assert.equal(imported, 2);
  assert.deepEqual(calls, [
    ['market_download', { url: entry.url, sha256: entry.sha256, sourceId: 'custom' }],
    ['market_download', { url: 'https://example.com/direct.nut' }],
  ]);
});

test('下载失败清理进度并保留重试能力，不进入安装确认', async () => {
  const state = createDownloadState();
  let imported = false;
  const actions = createDownloadActions({ state, render() {},
    invoke: async () => { throw new Error('cancelled'); },
    importPackages: async () => { imported = true; },
  });
  await actions.installDirect('https://example.com/a.nut');
  assert.equal(state.downloading.size, 0);
  assert.equal(imported, false);
});
