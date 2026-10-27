// NutHusk 插件界面端 SDK（宿主 API 1.1.0）。ui.html 中用 <script src="/_sdk/ui.js"></script> 引入。
//   nuthusk.call(method, params)       调用本插件自己的后端（返回 Promise）
//   nuthusk.onInput(fn)                接收宿主带入的数据（智能识别打开或从其他工具投递而来）：fn(text, meta)
//   nuthusk.onAction(fn)               注册流水线/无头动作处理器（供纯前端插件参与流水线）：fn(method, params) => result | Promise<result>
//   nuthusk.sendTo(text, targetId?)    将文本投递给其他插件（不传 targetId 时弹出工具选择面板）
// 它还会：跟随宿主的亮/暗主题；切换标签时自动聚焦主输入框；把全局快捷键转发给宿主。
(function () {
  var pending = new Map();
  var seq = 0;
  var inputListeners = [];
  var queuedInputs = [];
  var actionHandler = null;
  var queuedActions = [];

  function findPrimaryInput() {
    return document.querySelector(
      '[autofocus]:not(:disabled), textarea:not(:disabled):not([readonly]), input:not([type=hidden]):not([type=checkbox]):not([type=radio]):not([type=button]):not([type=submit]):not(:disabled):not([readonly])'
    );
  }

  function focusPrimaryInput() {
    var active = document.activeElement;
    if (active && active !== document.body && active !== document.documentElement) return;
    var target = findPrimaryInput();
    if (target && typeof target.focus === 'function') {
      try { target.focus(); } catch (_) {}
    }
  }

  function deliverInput(payload) {
    var data = typeof payload === 'string' ? { text: payload } : (payload || { text: '' });
    var text = String(data.text != null ? data.text : '');
    if (inputListeners.length > 0) {
      for (var i = 0; i < inputListeners.length; i++) {
        try { inputListeners[i](text, data); } catch (_) {}
      }
      return;
    }
    queuedInputs.push(data);
    // 兼容未显式调用 nuthusk.onInput 的 1.0 插件：若短时间内无人认领，自动填入主输入框
    setTimeout(function () {
      if (inputListeners.length > 0) return;
      var target = findPrimaryInput();
      if (target) {
        target.value = text;
        try { target.dispatchEvent(new Event('input', { bubbles: true })); } catch (_) {}
      }
    }, 30);
  }

  function runActionMessage(d) {
    if (!actionHandler) {
      queuedActions.push(d);
      setTimeout(function () {
        var idx = queuedActions.indexOf(d);
        if (idx >= 0) {
          queuedActions.splice(idx, 1);
          parent.postMessage({ type: 'actionResult', reqId: d.reqId, error: '该插件未注册 onAction 处理器' }, '*');
        }
      }, 200);
      return;
    }
    Promise.resolve()
      .then(function () { return actionHandler(d.method, d.params || {}); })
      .then(function (result) {
        parent.postMessage({ type: 'actionResult', reqId: d.reqId, result: result }, '*');
      })
      .catch(function (err) {
        parent.postMessage({ type: 'actionResult', reqId: d.reqId, error: String((err && err.message) || err) }, '*');
      });
  }

  window.addEventListener('message', function (e) {
    var d = e.data || {};
    if (d.type === 'theme') {
      // 'system' 表示跟随系统：去掉 data-theme，交给 prefers-color-scheme
      if (d.theme === 'light' || d.theme === 'dark') document.documentElement.setAttribute('data-theme', d.theme);
      else document.documentElement.removeAttribute('data-theme');
      return;
    }
    if (d.type === 'focus') {
      focusPrimaryInput();
      return;
    }
    if (d.type === 'input') {
      deliverInput(d.payload);
      return;
    }
    if (d.type === 'action') {
      runActionMessage(d);
      return;
    }
    var p = pending.get(d.reqId);
    if (!p) return;
    pending.delete(d.reqId);
    d.error ? p.reject(new Error(d.error)) : p.resolve(d.result);
  });

  // 焦点在插件里时键盘事件到不了宿主，这里把全局快捷键转发出去；Ctrl/Cmd+Enter 触发主按钮
  window.addEventListener('keydown', function (e) {
    if (!(e.ctrlKey || e.metaKey)) return;
    var k = String(e.key || '').toLowerCase();
    if (k === 'k' || k === 'w' || k === 'r' || k === 'tab' || k === '\\' || /^[1-9]$/.test(k)) {
      e.preventDefault();
      parent.postMessage({ type: 'key', key: e.key, ctrlKey: e.ctrlKey, metaKey: e.metaKey, shiftKey: e.shiftKey }, '*');
      return;
    }
    if (k === 'enter' && !e.defaultPrevented) {
      var btn = document.querySelector('[data-primary]:not(:disabled), .nh-primary:not(:disabled), button.primary:not(:disabled), #go:not(:disabled), #fmt:not(:disabled)');
      if (btn && typeof btn.click === 'function') {
        e.preventDefault();
        btn.click();
      }
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
    onInput: function (fn) {
      if (typeof fn !== 'function') return;
      inputListeners.push(fn);
      while (queuedInputs.length > 0) {
        var data = queuedInputs.shift();
        try { fn(String(data.text != null ? data.text : ''), data); } catch (_) {}
      }
    },
    onAction: function (fn) {
      if (typeof fn !== 'function') return;
      actionHandler = fn;
      while (queuedActions.length > 0) {
        runActionMessage(queuedActions.shift());
      }
    },
    sendTo: function (text, targetId) {
      parent.postMessage({
        type: 'sendTo',
        text: String(text != null ? text : ''),
        targetId: targetId || null,
      }, '*');
    },
  };
  window.nuthusk = api;
  window.toolbox = api; // 旧名字，保持兼容
})();
