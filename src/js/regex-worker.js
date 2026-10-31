// Self-contained bootstrap: used in a Blob worker to support custom WebView schemes.
export function regexWorkerMain() {
  self.onmessage = ({ data }) => {
    const matched = new Set(data.matched);
    for (let index = 0; index < data.tasks.length; index++) {
      const task = data.tasks[index];
      if (matched.has(task.key)) continue;
      self.postMessage({ type: 'testing', index });
      try {
        if (new RegExp(task.pattern).test(data.text)) {
          matched.add(task.key);
          self.postMessage({ type: 'match', index });
        }
      } catch { /* Invalid patterns do not prevent other plugins from matching. */ }
    }
    self.postMessage({ type: 'done' });
  };
  self.postMessage({ type: 'ready' });
}
