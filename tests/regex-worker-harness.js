import { parentPort } from 'node:worker_threads';
import { regexWorkerMain } from '../src/js/regex-worker.js';
globalThis.self = { postMessage: (data) => parentPort.postMessage(data) };
parentPort.on('message', (data) => self.onmessage({ data }));
regexWorkerMain();
