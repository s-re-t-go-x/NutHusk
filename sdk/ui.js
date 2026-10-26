// NutHusk 插件界面端 SDK。ui.html 中用 <script src="/_sdk/ui.js"></script> 引入。
//   nuthusk.call(method, params)  调用本插件自己的后端（返回 Promise）
// 它还会：跟随宿主的亮/暗主题；把全局快捷键（Ctrl/Cmd + K / W / Tab / 1-9）转发给宿主。
// 搭配 <link rel="stylesheet" href="/_sdk/ui.css"> 使用，主题才会同步生效。
(function () {
  var pending = new Map();
  var seq = 0;

  window.addEventListener('message', function (e) {
    var d = e.data || {};
    if (d.type === 'theme') {
      // 'system' 表示跟随系统：去掉 data-theme，交给 prefers-color-scheme
      if (d.theme === 'light' || d.theme === 'dark') document.documentElement.setAttribute('data-theme', d.theme);
      else document.documentElement.removeAttribute('data-theme');
      return;
    }
    var p = pending.get(d.reqId);
    if (!p) return;
    pending.delete(d.reqId);
    d.error ? p.reject(new Error(d.error)) : p.resolve(d.result);
  });

  // 焦点在插件里时键盘事件到不了宿主，这里把全局快捷键转发出去
  window.addEventListener('keydown', function (e) {
    if (!(e.ctrlKey || e.metaKey)) return;
    var k = String(e.key || '').toLowerCase();
    if (k === 'k' || k === 'w' || k === 'tab' || /^[1-9]$/.test(k)) {
      e.preventDefault();
      parent.postMessage({ type: 'key', key: e.key, ctrlKey: e.ctrlKey, metaKey: e.metaKey, shiftKey: e.shiftKey }, '*');
    }
  });

  var api = {
    call: function (method, params) {
      return new Promise(function (resolve, reject) {
        var reqId = ++seq;
        pending.set(reqId, { resolve: resolve, reject: reject });
        parent.postMessage({ type: 'call', reqId: reqId, method: method, params: params === undefined ? null : params }, '*');
      });
    },
  };
  window.nuthusk = api;
  window.toolbox = api; // 旧名字，保持兼容
})();
