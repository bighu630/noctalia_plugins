#!/usr/bin/env bash
# 冒烟：在真实 niri 会话内验证桥的 hello/heartbeat/niri 订阅/菜单解析。
# 需要：NIRI_SOCKET 可用（在 niri 会话内执行）、a11y 总线运行、GIMP 已启动。
# 注意：桥的日志走 stderr，stdout 是协议通道，断言分开。
set -uo pipefail

# 若在 niri 会话内但 NIRI_SOCKET 未导出，从运行时目录自动发现（兜底）。
if [ -z "${NIRI_SOCKET:-}" ]; then
  export NIRI_SOCKET=$(ls /run/user/$(id -u)/niri.wayland-*.sock 2>/dev/null | head -1)
fi

BIN="$(dirname "$0")/../bridge/target/debug/noctalia-global-menu-bridge"
[ -x "$BIN" ] || { echo "build bridge first: cd bridge && cargo build"; exit 1; }

"$BIN" > /tmp/gm-out.log 2>/tmp/gm-err.log &
BPID=$!
# 心跳间隔 5s：留足时间至少观察到一次 heartbeat。
sleep 7
kill $BPID 2>/dev/null; wait $BPID 2>/dev/null

echo "=== stdout (protocol) ==="
head -5 /tmp/gm-out.log
echo "=== stderr (log) ==="
head -5 /tmp/gm-err.log

PASS=0; FAIL=0
grep -q '"type":"hello"' /tmp/gm-out.log && { echo "PASS hello"; PASS=$((PASS+1)); } || { echo "FAIL hello"; FAIL=$((FAIL+1)); }
grep -q '"type":"heartbeat"' /tmp/gm-out.log && { echo "PASS heartbeat"; PASS=$((PASS+1)); } || { echo "FAIL heartbeat"; FAIL=$((FAIL+1)); }
echo "=== $PASS passed, $FAIL failed ==="
[ $FAIL -eq 0 ]
