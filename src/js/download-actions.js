import { toast } from './util.js';

/** 插件源与下载操作；render 由宿主决定当前页面是否需要重绘。 */
export function createDownloadActions({ state, invoke, render, importPackages, refresh }) {
  let indexRequest = 0;

  async function downloadAndImport(request, total = 0) {
    const key = `pkg:${request.url}`;
    state.downloading.set(key, { downloaded: 0, total });
    render();
    try {
      let tmpPath;
      try {
        tmpPath = await invoke('market_download', request);
      } finally {
        state.downloading.delete(key);
        render();
      }
      await importPackages([tmpPath], { invoke, refresh });
    } catch (err) {
      toast(`下载插件失败：${err}`);
    }
  }

  const actions = {
    async loadMarketSources() {
      try {
        state.sources = await invoke('market_sources');
        if (!state.sources.some((s) => s.id === state.selectedSource)) {
          state.selectedSource = state.sources[0]?.id ?? 'official';
        }
        await actions.loadMarketSource(state.selectedSource);
      } catch (err) {
        toast(`加载插件源失败：${err}`);
      }
    },

    async loadMarketSource(id) {
      const request = ++indexRequest;
      state.index = null;
      state.trust = null;
      state.fromCache = false;
      state.selectedSource = id;
      state.loading = true;
      state.error = null;
      render();
      try {
        const res = await invoke('market_index', { sourceId: id });
        if (request !== indexRequest) return;
        state.index = res.index ?? { name: res.name, entries: res.entries ?? res.plugins ?? [] };
        state.trust = res.trust;
        state.fromCache = !!res.fromCache;
        state.error = res.error ?? null;
      } catch (err) {
        if (request !== indexRequest) return;
        state.error = String(err);
        toast(`获取插件索引失败：${err}`);
      } finally {
        if (request === indexRequest) {
          state.loading = false;
          render();
        }
      }
    },

    async addSource(url, name) {
      try {
        await invoke('source_add', { url, name });
        toast('已添加插件源');
        await actions.loadMarketSources();
      } catch (err) {
        toast(`添加失败：${err}`);
      }
    },

    async removeSource(id) {
      try {
        await invoke('source_remove', { id });
        toast('已删除插件源');
        await actions.loadMarketSources();
      } catch (err) {
        toast(`删除失败：${err}`);
      }
    },

    installFromMarket(entry) {
      return downloadAndImport({
        url: entry.url,
        sha256: entry.sha256,
        sourceId: state.selectedSource,
      }, entry.size || 0);
    },

    installDirect(url) {
      return downloadAndImport({ url });
    },

    async cancelDownload(key) {
      try {
        await invoke('download_cancel', { key });
        state.downloading.delete(key);
        render();
      } catch (err) {
        toast(`取消失败：${err}`);
      }
    },
  };
  return actions;
}
