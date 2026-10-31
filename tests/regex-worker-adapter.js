import { Worker } from 'node:worker_threads';

// Runs the exact browser worker bootstrap in a real, terminable Node worker thread.
export class BrowserWorkerAdapter {
  constructor() {
    this.worker = new Worker(new URL('./regex-worker-harness.js', import.meta.url));
    this.worker.on('message', (data) => this.onmessage?.({ data }));
    this.worker.on('error', (error) => this.onerror?.(error));
  }
  postMessage(data) { this.worker.postMessage(data); }
  terminate() { return this.worker.terminate(); }
}
