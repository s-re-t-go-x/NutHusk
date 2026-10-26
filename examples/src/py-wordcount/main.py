# 插件后端（Python）：从 stdin 逐行读取 JSON-RPC 请求，向 stdout 逐行写出响应。
# 协议与语言无关：任何能读写 stdin/stdout 的程序都可以做插件后端（Go、Rust、C#、Java、Shell……）。
import json
import sys


def count(params):
    text = params.get("text", "")
    return {
        "chars": len(text),
        "words": len(text.split()),
        "lines": len(text.splitlines()) if text else 0,
        "bytes": len(text.encode("utf-8")),
    }


METHODS = {"count": count}

for line in sys.stdin:
    try:
        msg = json.loads(line)
    except ValueError:
        continue
    try:
        fn = METHODS.get(msg.get("method"))
        if fn is None:
            raise ValueError("未知方法: %s" % msg.get("method"))
        reply = {"jsonrpc": "2.0", "id": msg.get("id"), "result": fn(msg.get("params") or {})}
    except Exception as e:  # 出错时返回 error，而不是让进程退出
        reply = {"jsonrpc": "2.0", "id": msg.get("id"), "error": {"code": -32000, "message": str(e)}}
    sys.stdout.write(json.dumps(reply, ensure_ascii=False) + "\n")
    sys.stdout.flush()  # 一定要刷新，否则宿主收不到
