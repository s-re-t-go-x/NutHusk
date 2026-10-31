import { regexWorkerMain } from './regex-worker.js';

const workerUrls = new WeakMap();
function createRegexWorker() {
  const url = URL.createObjectURL(new Blob([`(${regexWorkerMain.toString()})();`], { type: 'text/javascript' }));
  try {
    const worker = new Worker(url);
    workerUrls.set(worker, url);
    return worker;
  } catch (err) {
    URL.revokeObjectURL(url);
    throw err;
  }
}

/** Evaluate plugin regexes off the UI thread; terminate only the slow pattern, then resume. */
export function matchRegexTasks(tasks, text, {
  signal,
  onMatch,
  timeoutMs = 150,
  startupTimeoutMs = 2000,
  createWorker = createRegexWorker,
} = {}) {
  return new Promise((resolve) => {
    const matched = new Set();
    let worker = null, timer = null, offset = 0, finished = false;

    function stopWorker() {
      clearTimeout(timer);
      if (worker) {
        worker.onmessage = null;
        worker.onerror = null;
        worker.terminate();
        const url = workerUrls.get(worker);
        if (url) URL.revokeObjectURL(url);
        worker = null;
      }
    }
    function finish() {
      if (finished) return;
      finished = true;
      stopWorker();
      signal?.removeEventListener('abort', finish);
      resolve(matched);
    }
    function start() {
      stopWorker();
      if (finished || signal?.aborted || offset >= tasks.length) return finish();
      let active;
      try { active = createWorker(); } catch { return finish(); }
      worker = active;
      const batchOffset = offset;
      // A separate startup deadline avoids charging worker creation to the first regex.
      timer = setTimeout(finish, startupTimeoutMs);
      active.onerror = () => finish();
      active.onmessage = ({ data }) => {
        if (finished || worker !== active || !data) return;
        if (data.type === 'ready') {
          clearTimeout(timer);
          timer = setTimeout(finish, startupTimeoutMs);
          active.postMessage({ text, tasks: tasks.slice(batchOffset), matched: [...matched] });
        } else if (data.type === 'testing') {
          if (!Number.isInteger(data.index) || data.index < 0 || batchOffset + data.index >= tasks.length) return finish();
          clearTimeout(timer);
          timer = setTimeout(() => {
            // Skip the offending pattern, keeping all later patterns and plugins usable.
            offset = batchOffset + data.index + 1;
            start();
          }, timeoutMs);
        } else if (data.type === 'match') {
          const task = tasks[batchOffset + data.index];
          if (!task) return finish();
          matched.add(task.key);
          onMatch?.(new Set(matched));
        } else if (data.type === 'done') {
          finish();
        }
      };
    }
    if (signal?.aborted || tasks.length === 0) return finish();
    signal?.addEventListener('abort', finish, { once: true });
    start();
  });
}
