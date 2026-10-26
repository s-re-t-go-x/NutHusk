// 在页面绘制之前就应用上次选择的主题，避免先闪一下浅色再变深色。
// （必须是独立的脚本文件：内容安全策略不允许内联脚本。）
try {
  var t = localStorage.getItem('nuthusk.theme');
  if (t === 'light' || t === 'dark') document.documentElement.setAttribute('data-theme', t);
} catch (e) { /* 存储不可用就跟随系统 */ }
