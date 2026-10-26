// 插件后端：从 stdin 逐行读取 JSON-RPC 请求，向 stdout 逐行写入响应。
// 协议与语言无关，任何能读写 stdin/stdout 的程序都可以做插件后端。
const readline = require('node:readline');
const crypto = require('node:crypto');

const ALGOS = ['md5', 'sha1', 'sha256', 'sha512'];
const methods = {
  hash({ text = '', algo = 'sha256' }) {
    if (!ALGOS.includes(algo)) throw new Error(`不支持的算法: ${algo}`);
    return crypto.createHash(algo).update(String(text)).digest('hex');
  },
};

const send = (obj) => process.stdout.write(JSON.stringify(obj) + '\n');

readline.createInterface({ input: process.stdin }).on('line', (line) => {
  let msg;
  try { msg = JSON.parse(line); } catch { return; }
  try {
    if (!Object.hasOwn(methods, msg.method)) throw new Error(`未知方法: ${msg.method}`);
    send({ jsonrpc: '2.0', id: msg.id, result: methods[msg.method](msg.params || {}) });
  } catch (e) {
    send({ jsonrpc: '2.0', id: msg.id, error: { code: -32000, message: e.message } });
  }
});
