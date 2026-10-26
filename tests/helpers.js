import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';

/** 用真实的 index.html 创建 DOM，并把 window / document 等挂到全局，供 src/js 里的模块使用。 */
export function createDom() {
  const html = readFileSync(new URL('../src/index.html', import.meta.url), 'utf8').replace(/<script[^>]*><\/script>/g, '');
  const dom = new JSDOM(html, { url: 'http://localhost/', pretendToBeVisual: true });
  const { window } = dom;
  const define = (k, v) => Object.defineProperty(globalThis, k, { value: v, configurable: true, writable: true });
  define('window', window);
  define('document', window.document);
  define('navigator', window.navigator);
  define('localStorage', window.localStorage);
  define('FileReader', window.FileReader); // 浏览器里是全局的，Node 里要补上
  return window;
}

export const tick = (ms = 25) => new Promise((r) => setTimeout(r, ms));
export const $$ = (sel) => [...document.querySelectorAll(sel)];
export const $ = (id) => document.getElementById(id);
export const key = (target, k, opts = {}) =>
  target.dispatchEvent(new window.KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...opts }));

export const trust = {
  none: { level: 'unverified', reason: '没有签名', signer: null, keyId: null },
  ok: { level: 'trusted', reason: '由「NutHusk 官方」签名，内容未被改动', signer: 'NutHusk 官方', keyId: 'abcd1234abcd1234' },
  bad: { level: 'invalid', reason: '签名与内容对不上，文件可能被篡改，或签名无效', signer: null, keyId: 'abcd1234abcd1234' },
};

export const mkPlugin = (id, name, extra = {}) => ({
  id, name, version: '1.0.0', description: name + ' 的说明', icon: null, category: '开发', keywords: [],
  ui: 'ui.html', permissions: [], enabled: true, error: null, hasBackend: false, backendCommand: null,
  group: 'appdata', groupLabel: 'AppData', path: `/data/plugins/${id}`, trust: trust.none, shadowed: [], ...extra,
});

export const mkDirs = () => [
  { key: 'appdata', label: 'AppData', pluginCount: 2, paths: [{ path: '/data/plugins', exists: true, writable: true }] },
  { key: 'program', label: '程序目录', pluginCount: 1, paths: [{ path: '/app/plugins', exists: true, writable: false }] },
];

/** 模拟 Tauri：记录所有调用，并在内存里维护插件、设置、目录等状态。 */
export function makeTauri(init = {}) {
  const state = {
    plugins: [], settings: { theme: 'system', showRecent: true, showFavorites: true, dirOrder: ['appdata', 'program'] },
    dirs: mkDirs(), signers: [], packages: {}, saved: [], discarded: [], pick: [], version: '0.1.0', installResult: null,
    ...init,
  };
  const calls = [];
  const tauri = {
    core: {
      invoke: async (cmd, args, options) => {
        calls.push([cmd, args, options]);
        switch (cmd) {
          case 'list_plugins': return state.plugins.map((p) => ({ ...p }));
          case 'reload_plugins': return state.plugins;
          case 'get_settings': return { ...state.settings };
          case 'patch_settings': Object.assign(state.settings, args.patch); return { ...state.settings };
          case 'plugin_dirs': return state.dirs.map((d) => ({ ...d }));
          case 'set_dir_order':
            state.dirs = args.order.map((k) => state.dirs.find((d) => d.key === k));
            state.settings.dirOrder = args.order;
            return state.dirs;
          case 'trusted_signers': return state.signers;
          case 'set_plugin_enabled': state.plugins.find((p) => p.id === args.id).enabled = args.enabled; return;
          case 'plugin_logs': return state.logs?.[args.id] ?? [];
          case 'plugin_call':
            if (state.onPluginCall) return await state.onPluginCall(args);
            if (args.method === 'boom') throw 'plugin exploded';
            return 'RESULT:' + args.method;
          case 'inspect_plugin_package':
            if (state.packages[args.path]) return state.packages[args.path].info;
            throw '不是有效的插件包（需要 zip 格式）: invalid Zip archive';
          case 'install_plugin_package': {
            const pkg = state.packages[args.path];
            if (pkg.fail) throw pkg.fail;
            if (pkg.plugin) state.plugins.push(pkg.plugin);
            return state.installResult ?? { id: pkg.info.manifest.id, path: '/data/plugins/x', groupLabel: 'AppData', active: true };
          }
          case 'uninstall_plugin': state.plugins = state.plugins.filter((p) => p.id !== args.id); return;
          case 'save_dropped_package': {
            const name = decodeURIComponent(options.headers['x-file-name']);
            state.saved.push({ name, bytes: args });
            return `/tmp/nuthusk-drop/${name}`;
          }
          case 'discard_dropped_package': state.discarded.push(args.path); return;
          case 'runtime_status': return state.runtimes ?? [
            { name: 'python', version: '3.13.2', downloadable: true, onSystem: false, installed: null, size: 10000000, description: 'Python 3.13', source: 'python.org', userRecipe: false },
            { name: 'node', version: '22.14.0', downloadable: true, onSystem: true, installed: null, size: 20000000, description: 'Node.js', source: 'nodejs.org', userRecipe: false },
          ];
          case 'runtime_install': return;
          case 'runtime_remove': return;
          case 'market_sources': return state.marketSources ?? [{ id: 'official', name: '官方源', url: 'https://example.com/index.json', official: true }];
          case 'market_index': return state.marketIndex ?? { name: '官方源', entries: [], trust: trust.ok, fromCache: false, error: null };
          case 'source_add': return;
          case 'source_remove': return;
          case 'market_download': return '/tmp/downloaded.nut';
          case 'download_cancel': return;
        }
      },
    },
    event: { listen: async () => () => {} },
    dialog: { open: async (opts) => { calls.push(['dialog.open', opts]); return state.pick; } },
    app: { getVersion: async () => state.version },
  };
  const called = (name) => calls.filter((c) => c[0] === name);
  return { tauri, calls, state, called };
}

export const mkPackage = (id, name, version, extra = {}, plugin = null) => ({
  info: {
    manifest: { id, name, version, description: `${name} 的说明`, permissions: [], ...(extra.manifest ?? {}) },
    sha256: 'a'.repeat(64), packageSize: 1000, fileCount: 3, unpackedSize: 2048, compatError: null,
    backendCommand: null, backendFound: null, installedVersion: null, trust: trust.none, iconData: null,
    installTarget: '/data/plugins', installTargetLabel: 'AppData', ...extra.info,
  },
  plugin,
  ...(extra.pkg ?? {}),
});
