# Noctalia Global Menu

macOS 风格全局菜单：焦点应用（GTK3）的菜单栏显示在 Noctalia 顶栏，点击展开子菜单、点击项触发应用真实动作。

## 架构

- `bridge/` — Rust 桥接程序：niri IPC 焦点跟踪 + AT-SPI 菜单读取 + 本地 HTTP 命令服务（设计文档：`docs/superpowers/specs/2026-08-01-global-menu-bridge-design.md`）
- `service.luau` — 托管桥进程，NDJSON 事件 → `noctalia.state` 广播
- `widget.luau` — 菜单栏条
- `popup.luau` — 子菜单弹出面板

## 依赖

- niri（焦点事件来源）
- at-spi2-core（a11y 总线；GTK3 应用自动接入，无需配置）
- Rust ≥ 1.81（构建桥）
- GTK3 应用（如 GIMP）——Qt6/Chromium/Electron 本机暂不支持（见设计文档 §9）

## 构建与安装

```bash
bash global-menu/scripts/build.sh   # cargo build --release → global-menu/bridge-bin/
```

然后在 Noctalia 设置中启用 "Global Menu" 插件（或重启 Noctalia）。

## 使用

启动任意 GTK3 应用并聚焦 → 菜单栏出现在顶栏。点击顶层项展开子菜单，点击项执行。

## 故障排查

- 菜单不出现：`systemctl --user status at-spi-dbus-bus.service`（a11y 总线须运行）；
  确认应用是 GTK3（`ldd $(which gimp) | grep gtk-3`）
- 桥日志：插件日志（Noctalia 日志）或手动运行
  `NIRI_SOCKET=/run/user/1000/niri.wayland-*.sock ./bridge/target/release/noctalia-global-menu-bridge`
