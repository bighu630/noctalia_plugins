# 手动冒烟清单（真实桌面）

前置：在 niri 会话内（ssh 或终端模拟器），`export NIRI_SOCKET=/run/user/$(id -u)/niri.wayland-*.sock`（或用 niri 会话内 shell）。

1. 启动 GIMP（GTK3，无需任何配置）
2. `cd global-menu/bridge && cargo build`（或 release）
3. `./target/debug/noctalia-global-menu-bridge > /tmp/gm-out.log 2>/tmp/gm-err.log &`
4. 焦点切到 GIMP 窗口（`niri msg action focus-window --id <gimp窗口id>` 或鼠标点击）
5. `grep '"type":"menu"' /tmp/gm-out.log | tail -1 | head -c 600`
   预期：`"app":{"app_id":"gimp"...},"menu":{"label":"","type":"submenu","children":[{"label":"File"...`
6. 若 menu 为 null：检查 `cat /tmp/gm-err.log`（a11y 连接失败？gimp 未注册？）
7. 测试点击（手动）：`curl -s -X POST -d '{"path":[3]}' http://127.0.0.1:<port>/click`
   （port 从 hello 行取；path [3] = View 菜单——预期触发 GIMP 打开 View 相关动作或不报错）
8. 全链路插件验证：在 Noctalia 中启用插件（见 README），观察菜单栏条 + 点击 Fullscreen
