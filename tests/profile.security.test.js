import test from 'node:test';
import assert from 'node:assert/strict';
import { parseProfile } from '../src/js/profile.js';
import { importDroppedFiles } from '../src/js/importer.js';
import { createDom } from './helpers.js';

createDom();
test('拖放配置清单遵守与 Rust 文件导入相同的大小、条目和版本限制', async () => {
  const profile = { format: 'nuthusk-profile', version: 1 };
  for (const version of [-1, '1', {}, true]) {
    assert.throws(() => parseProfile({ ...profile, version }));
  }
  for (const [key, size] of [['sources', 51], ['plugins', 501], ['runtimes', 51]]) {
    assert.throws(() => parseProfile({ ...profile, [key]: Array(size).fill({}) }));
  }
  assert.throws(() => parseProfile(JSON.stringify({ ...profile, name: 'x'.repeat(2 * 1024 * 1024) })));
  let read = false, imported = false;
  await importDroppedFiles([{ name: 'oversized.nutprofile', size: 3 * 1024 * 1024,
    text: async () => { read = true; return '{}'; },
  }], { onImportProfile: () => { imported = true; } });
  assert.equal(read, false);
  assert.equal(imported, false);
  assert.equal(parseProfile(profile).version, 1);
});
