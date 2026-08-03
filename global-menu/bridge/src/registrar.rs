//! `com.canonical.AppMenu.Registrar` 服务端（会话总线）。
//!
//! Chromium/Electron（GlobalMenuBarX11，X11 模式）与 Qt 应用在启动时调用
//! `RegisterWindow(xid, menu_path)` 把菜单导出为 com.canonical.dbusmenu；
//! KDE 全局菜单即此通道。桥**自己成为 registrar**：拥有 well-known name、
//! 实现标准三方法、内部维护 xid → (caller, path, pid) 映射与 pid → 注册列表
//! 索引（焦点匹配用）。
//!
//! 线程模型：接口方法在 zbus blocking 连接的 ObjectServer 派发线程执行；
//! HTTP/proxy 线程只读查询。共享状态 Arc<Mutex<RegistrarState>>。
//!
//! 焦点匹配（resolve 时，见 find_for_focus）：
//! 1. pid 精确匹配（Wayland 原生应用，niri pid 即应用 pid）；
//! 2. X11 兜底：XWayland 窗口的 niri pid 是 xwayland-satellite 的 pid（实测
//!    Typora 报 2682=xwayland，注册 pid 却是 242071=Typora 本体）——用
//!    `xdotool search --class <app_id>` 枚举该 WM_CLASS 的 X 窗口，与已注册
//!    xid 求交（注册表里的 xid 本身就是 X 窗口，WM_CLASS 匹配是精确的）；
//! 3. comm 兜底：/proc/<pid>/comm == app_id（大小写不敏感，无外部依赖）。
//!
//! 名字被占（vala-panel-appmenu-daemon 等已持名）→ warn 非致命，与
//! status::own_status 同模式。

use anyhow::Result;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use zbus::blocking::Connection;
use zbus::interface;
use zbus::message::Header;

/// 一次注册记录：xid → (caller 唯一名, menu 对象路径, 调用者 pid)。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MenuRegistration {
    pub xid: u32,
    /// 调用方连接唯一名（如 ":1.29161"），fetch_layout/Event 的 destination。
    pub bus: String,
    /// 调用方导出的 com.canonical.dbusmenu 对象路径。
    pub path: String,
    /// 经 GetConnectionUnixProcessID 解析的调用者 pid。
    pub pid: u32,
}

/// 注册表核心（纯逻辑，可单测）。
#[derive(Default, Debug)]
pub struct RegistrarState {
    /// 规范映射：xid → 注册记录。
    by_xid: HashMap<u32, MenuRegistration>,
    /// 焦点匹配索引：pid → 注册记录列表（一进程可注册多窗口）。
    by_pid: HashMap<u32, Vec<MenuRegistration>>,
}

impl RegistrarState {
    /// 注册（覆盖同 xid 旧记录）。返回旧记录（如有）。
    /// 持久化文件（桥重启后恢复 Electron 等应用的注册——它们不监听
    /// Registrar 名字出现，桥重启 = 注册丢失，实测 Typora 需手动重启）。
    fn persist(&self) {
        let Some(path) = persist_path() else { return };
        let entries: Vec<&MenuRegistration> = self.by_xid.values().collect();
        if let Ok(json) = serde_json::to_string(&entries) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, json);
        }
    }

    /// 启动恢复：读文件 → 验证调用者连接存活 → 重建索引。
    pub fn restore(&mut self, conn: &zbus::blocking::Connection) {
        let Some(path) = persist_path() else { return };
        let Ok(raw) = std::fs::read_to_string(path) else { return };
        let Ok(entries): Result<Vec<MenuRegistration>, _> = serde_json::from_str(&raw) else { return };
        for reg in entries {
            // 连接存活 = GetConnectionUnixProcessID 成功（唯一名是进程级，进程没退就有效）
            let alive = conn
                .call_method(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    Some("org.freedesktop.DBus"),
                    "GetConnectionUnixProcessID",
                    &(reg.bus.as_str(),),
                )
                .is_ok();
            if alive {
                self.by_xid.insert(reg.xid, reg.clone());
                self.by_pid.entry(reg.pid).or_default().push(reg);
            }
        }
        if !self.by_xid.is_empty() {
            eprintln!("[global-menu-bridge] registrar restored {} entries", self.by_xid.len());
        }
    }

    pub fn register(&mut self, xid: u32, bus: String, path: String, pid: u32) -> Option<MenuRegistration> {
        let reg = MenuRegistration { xid, bus, path, pid };
        let old = self.by_xid.insert(xid, reg.clone());
        if let Some(old) = &old {
            // 同 xid 重注册：先移除旧 pid 槽位
            if let Some(list) = self.by_pid.get_mut(&old.pid) {
                list.retain(|r| r.xid != xid);
                if list.is_empty() {
                    self.by_pid.remove(&old.pid);
                }
            }
        }
        self.by_pid.entry(pid).or_default().push(reg);
        self.persist();
        old
    }

    /// 注销（连接退出/窗口销毁时应用主动调用；也信任 xid 即键，不复查调用者）。
    pub fn unregister(&mut self, xid: u32) -> Option<MenuRegistration> {
        let old = self.by_xid.remove(&xid)?;
        if let Some(list) = self.by_pid.get_mut(&old.pid) {
            list.retain(|r| r.xid != xid);
            if list.is_empty() {
                self.by_pid.remove(&old.pid);
            }
        }
        self.persist();
        Some(old)
    }

    pub fn lookup_xid(&self, xid: u32) -> Option<&MenuRegistration> {
        self.by_xid.get(&xid)
    }

    pub fn registrations_for_pid(&self, pid: u32) -> &[MenuRegistration] {
        self.by_pid.get(&pid).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn is_empty(&self) -> bool {
        self.by_xid.is_empty()
    }

    pub fn len(&self) -> usize {
        self.by_xid.len()
    }

    /// 焦点 → 注册记录。优先级：
    /// 1. pid 精确匹配（Wayland 原生应用）；
    /// 2. X11 兜底：xdotool search --class <app_id> 与注册 xid 求交（XWayland
    ///    应用：niri 报的 pid 是 xwayland 进程 pid，注册 pid 才是应用本体；
    ///    WM_CLASS 与 niri app_id 同源，命中即精确）；
    /// 3. comm 兜底：/proc/<pid>/comm == app_id（大小写不敏感，无外部依赖）。
    pub fn find_for_focus(&self, pid: u32, app_id: &str) -> Option<MenuRegistration> {
        // 1. pid
        if let Some(regs) = self.by_pid.get(&pid) {
            if let Some(r) = regs.first() {
                return Some(r.clone());
            }
        }
        // 2. X11 窗口类匹配（best effort，无 xdotool/DISPLAY 时静默跳过）
        if !app_id.is_empty() {
            if let Some(r) = self.find_via_x11(app_id) {
                return Some(r);
            }
        }
        // 3. /proc comm 匹配
        if !app_id.is_empty() {
            let needle = app_id.to_ascii_lowercase();
            for (proc_pid, regs) in &self.by_pid {
                let Ok(comm) = std::fs::read_to_string(format!("/proc/{proc_pid}/comm")) else {
                    continue;
                };
                if comm.trim().to_ascii_lowercase() == needle {
                    if let Some(r) = regs.first() {
                        return Some(r.clone());
                    }
                }
            }
        }
        None
    }

    /// 枚举 X11 上 WM_CLASS 匹配的窗口，与注册 xid 求交。
    fn find_via_x11(&self, app_id: &str) -> Option<MenuRegistration> {
        let display = x11_display()?;
        let out = std::process::Command::new("xdotool")
            .args(["search", "--class", app_id])
            .env("DISPLAY", display)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Ok(xid) = line.trim().parse::<u32>() {
                if let Some(reg) = self.by_xid.get(&xid) {
                    return Some(reg.clone());
                }
            }
        }
        None
    }
}

/// DISPLAY 探测：优先环境变量；否则从 xwayland-satellite 进程参数解析
/// （niri 环境 DISPLAY 可能未导出到桥）。
fn x11_display() -> Option<String> {
    if let Ok(d) = std::env::var("DISPLAY") {
        if !d.is_empty() {
            return Some(d);
        }
    }
    let entries = std::fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(cmdline) = std::fs::read_to_string(format!("/proc/{name}/cmdline")) else {
            continue;
        };
        let mut args = cmdline.split('\0');
        let exe = args.next().unwrap_or("");
        if exe.contains("xwayland-satellite") {
            if let Some(disp) = args.next() {
                if disp.starts_with(':') {
                    return Some(disp.to_string());
                }
            }
        }
    }
    None
}

pub type RegistrarHandle = Arc<Mutex<RegistrarState>>;

/// 接口线程 → worker 线程的工作项。
/// 接口方法**绝不能在 executor 线程上做阻塞总线调用**（zbus async-io 后端
/// 单线程 executor：block_on 等回复时唤醒丢失 → 永久死锁，实测 RegisterWindow
/// 挂死并瘫痪整个 ObjectServer）——所有总线交互（pid 查询）经 worker 线程。
enum RegWork {
    Register { xid: u32, bus: String, path: String },
    Unregister(u32),
}

/// 启动 registrar worker：负责 GetConnectionUnixProcessID 等阻塞总线调用。
/// 返回工作通道（AppMenuRegistrar 持有 clone，接口存活期间通道不关闭）。
fn spawn_worker(conn: Connection, state: RegistrarHandle) -> Sender<RegWork> {
    let (tx, rx) = std::sync::mpsc::channel::<RegWork>();
    std::thread::spawn(move || {
        for work in rx {
            match work {
                RegWork::Register { xid, bus, path } => {
                    // 会话总线取调用者 pid（a11y 总线是独立 daemon，不能用那个）
                    let pid = conn
                        .call_method(
                            Some("org.freedesktop.DBus"),
                            "/org/freedesktop/DBus",
                            Some("org.freedesktop.DBus"),
                            "GetConnectionUnixProcessID",
                            &(bus.as_str(),),
                        )
                        .and_then(|r| r.body().deserialize::<u32>())
                        .unwrap_or(0); // 失败 pid=0：不参与任何匹配，仅记录
                    state.lock().unwrap().register(xid, bus.clone(), path.clone(), pid);
                    eprintln!("[global-menu-bridge] RegisterWindow xid={xid} pid={pid} caller={bus} path={path}");
                }
                RegWork::Unregister(xid) => {
                    let removed = state.lock().unwrap().unregister(xid);
                    eprintln!(
                        "[global-menu-bridge] UnregisterWindow xid={xid} {}",
                        if removed.is_some() { "(removed)" } else { "(unknown xid)" }
                    );
                }
            }
        }
    });
    tx
}

/// 持久化文件位置：~/.cache/noctalia-global-menu-registrar.json
fn persist_path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/noctalia-global-menu-registrar.json"))
}

/// `com.canonical.AppMenu.Registrar` 接口实现。
/// 方法在 zbus ObjectServer 派发线程执行（内部是单线程 executor，阻塞 = 死锁），
/// 因此方法体只做状态写队列/读，pid 查询交给 worker 线程；
/// 状态经 Mutex 与 HTTP/proxy 线程共享。
pub struct AppMenuRegistrar {
    state: RegistrarHandle,
    work_tx: Sender<RegWork>,
}

#[interface(name = "com.canonical.AppMenu.Registrar")]
impl AppMenuRegistrar {
    /// 应用发布菜单。xid 为 X11 窗口 id（或 Wayland 合成值），
    /// menu_path 为调用连接上导出的 com.canonical.dbusmenu 对象路径。
    /// 立即返回；pid 解析由 worker 线程完成（接口线程不阻塞总线）。
    /// 注意 wire 签名是 `(uo)`：menu_path 是对象路径而非字符串（Chromium/Electron
    /// `g_variant_new("(uo)", ...)`；曾用 String 导致 dbus-broker 拒绝调用——
    /// 实测 Typora 注册失败，见 git log）。
    fn register_window(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        xid: u32,
        menu_path: zbus::zvariant::ObjectPath<'_>,
    ) -> zbus::fdo::Result<()> {
        let sender = hdr
            .sender()
            .ok_or_else(|| zbus::fdo::Error::Failed("RegisterWindow: no sender".into()))?;
        let _ = self.work_tx.send(RegWork::Register {
            xid,
            bus: sender.to_string(),
            path: menu_path.as_str().to_string(),
        });
        Ok(())
    }

    /// 应用退出/窗口销毁时注销。以 xid 为键，不复查调用者
    /// （连接可能已消失，pid 无法再查）。
    fn unregister_window(&self, xid: u32) -> zbus::fdo::Result<()> {
        let _ = self.work_tx.send(RegWork::Unregister(xid));
        Ok(())
    }

    /// 按 xid 查菜单地址。未注册 → Failed（规范行为，供 bar 类消费方探测）。
    fn get_menu_for_window(&self, xid: u32) -> zbus::fdo::Result<(String, zbus::zvariant::OwnedObjectPath)> {
        let reg = self
            .state
            .lock()
            .unwrap()
            .lookup_xid(xid)
            .cloned()
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("no menu registered for xid {xid}")))?;
        let path = zbus::zvariant::OwnedObjectPath::try_from(reg.path.as_str())
            .map_err(|e| zbus::fdo::Error::Failed(format!("bad menu path {}: {e}", reg.path)))?;
        Ok((reg.bus, path))
    }
}

/// 尝试注册 com.canonical.AppMenu.Registrar。失败（名字被占）仅 warn，不致命。
pub fn own_registrar(conn: &Connection, handle: &RegistrarHandle) -> Result<()> {
    use enumflags2::BitFlag;
    use zbus::blocking::fdo::DBusProxy;
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    use zbus::names::WellKnownName;
    let dbus = DBusProxy::new(conn)?;
    // ReplaceExisting：桥重启时旧实例还占着名字（非致命 warn 路径），
    // 本桥是唯一 Registrar 场景（无 KDE），直接接管避免"名字被占后不重试"的
    // 死局（实测：新桥启动失败后旧桥退出，名字空置，新桥再无 Registrar）。
    let name = WellKnownName::try_from("com.canonical.AppMenu.Registrar")?;
    let flags = RequestNameFlags::ReplaceExisting.into();
    match dbus.request_name(name, flags) {
        Ok(reply) if reply == RequestNameReply::PrimaryOwner => {
            let work_tx = spawn_worker(conn.clone(), handle.clone());
            let iface = AppMenuRegistrar { state: handle.clone(), work_tx };
            conn.object_server().at("/com/canonical/AppMenu/Registrar", iface)?;
            Ok(())
        }
        Ok(_) => Err(anyhow::anyhow!("com.canonical.AppMenu.Registrar already owned by another service")),
        Err(e) => Err(anyhow::anyhow!("request_name failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(xid: u32, pid: u32) -> MenuRegistration {
        MenuRegistration { xid, bus: format!(":1.{xid}"), path: format!("/com/canonical/menu/{xid}"), pid }
    }

    #[test]
    fn register_and_unregister_maintains_both_indexes() {
        let mut s = RegistrarState::default();
        s.register(100, ":1.1".into(), "/m/100".into(), 1000);
        s.register(200, ":1.1".into(), "/m/200".into(), 1000);
        s.register(300, ":1.2".into(), "/m/300".into(), 2000);
        assert_eq!(s.len(), 3);
        assert_eq!(s.registrations_for_pid(1000).len(), 2);
        assert_eq!(s.registrations_for_pid(2000).len(), 1);

        // 注销中间项：pid 索引同步收缩
        let old = s.unregister(200);
        assert!(old.is_some());
        assert_eq!(s.len(), 2);
        assert_eq!(s.registrations_for_pid(1000).len(), 1);
        assert_eq!(s.registrations_for_pid(1000)[0].xid, 100);
        assert!(s.lookup_xid(200).is_none());

        // 注销未知 xid → None
        assert!(s.unregister(999).is_none());
    }

    #[test]
    fn re_register_same_xid_replaces_old_pid_slot() {
        let mut s = RegistrarState::default();
        s.register(100, ":1.1".into(), "/m/100".into(), 1000);
        // 同 xid 换 pid 重注册（菜单重建场景）
        s.register(100, ":1.9".into(), "/m/100-new".into(), 9000);
        assert_eq!(s.len(), 1);
        assert_eq!(s.registrations_for_pid(1000).len(), 0);
        assert_eq!(s.registrations_for_pid(9000).len(), 1);
        assert_eq!(s.lookup_xid(100).unwrap().path, "/m/100-new");
    }

    #[test]
    fn find_for_focus_matches_pid_first() {
        let mut s = RegistrarState::default();
        s.register(100, ":1.1".into(), "/m/100".into(), 1000);
        // pid 命中（即使 app_id 完全不相关也不看 app_id）
        let found = s.find_for_focus(1000, "anything-else").unwrap();
        assert_eq!(found.xid, 100);
    }

    #[test]
    fn find_for_focus_falls_back_to_comm() {
        let mut s = RegistrarState::default();
        // 用当前测试进程注册：niri 式 pid 失配（传不存在的 pid），comm 兜底命中
        let pid = std::process::id();
        s.register(100, ":1.1".into(), "/m/100".into(), pid);
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap();
        let comm = comm.trim().to_string();
        let found = s.find_for_focus(0xdead_beef, &comm).unwrap();
        assert_eq!(found.xid, 100);
        // 大小写不敏感
        let found2 = s.find_for_focus(0xdead_beef, &comm.to_uppercase()).unwrap();
        assert_eq!(found2.xid, 100);
    }

    #[test]
    fn find_for_focus_returns_none_when_nothing_matches() {
        let s = RegistrarState::default();
        assert!(s.find_for_focus(12345, "nonexistent-app").is_none());
    }

    #[test]
    fn empty_app_id_skips_fallbacks() {
        let mut s = RegistrarState::default();
        s.register(100, ":1.1".into(), "/m/100".into(), std::process::id());
        // pid 失配 + app_id 空 → 不触发 comm/X11 兜底
        assert!(s.find_for_focus(0xdead_beef, "").is_none());
    }
}
