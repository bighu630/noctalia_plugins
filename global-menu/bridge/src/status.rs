//! 拥有会话总线 org.a11y.Status，置 IsEnabled=true。
//! GTK3 不需要它（自动启用），Qt 需要——为 Qt 预留（设计文档 §2）。
//!
//! zbus 4 的 #[interface] 宏生成接口实现；若宏 API 与 4.4 有出入，
//! 以 zbus 4 docs 为准调整，保持 XML 语义 org.a11y.Status / IsEnabled(b) 不变。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use zbus::blocking::Connection;
use zbus::interface;

pub struct A11yStatus {
    enabled: Arc<AtomicBool>,
}

impl A11yStatus {
    pub fn new() -> Self {
        Self { enabled: Arc::new(AtomicBool::new(true)) }
    }
}

#[interface(name = "org.a11y.Status")]
impl A11yStatus {
    #[zbus(property)]
    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

/// 把 at-spi-bus-launcher 的 org.a11y.Status.IsEnabled 置 true。
/// Chromium/Electron/Qt 检测 a11y 是否启用读的是 **org.a11y.Bus 服务**上的
/// IsEnabled 属性（readwrite，由外部显式 Set；GNOME 由 settings-daemon 管，
/// niri 无人管 → 默认 false → Chromium/Qt 不注册 a11y）。实测置 true 后
/// Chrome/Dolphin/KDE 应用全部注册（2026-08-03 实证）。
pub fn enable_launcher_a11y(conn: &Connection) -> anyhow::Result<()> {
    use zvariant::Value;
    let _ = conn.call_method(
        Some("org.a11y.Bus"),
        "/org/a11y/bus",
        Some("org.freedesktop.DBus.Properties"),
        "Set",
        &("org.a11y.Status", "IsEnabled", Value::Bool(true)),
    )?;
    Ok(())
}

/// 尝试注册 org.a11y.Status。失败（名字已被占）仅 warn，不致命。
pub fn own_status(conn: &Connection) -> anyhow::Result<()> {
    // zbus 4.4：blocking::fdo 内的 RequestNameFlags/Reply 为私有 re-export，
    // 公共路径在 zbus::fdo；empty() 由 enumflags2::BitFlag trait 提供。
    use enumflags2::BitFlag;
    use zbus::blocking::fdo::DBusProxy;
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    use zbus::names::WellKnownName;
    let dbus = DBusProxy::new(conn)?;
    // 请求名字：REPLACE_EXISTING 不启用（不抢别人的）；失败静默。
    let name = WellKnownName::try_from("org.a11y.Status")?;
    let flags = RequestNameFlags::empty();
    match dbus.request_name(name, flags) {
        Ok(reply) if reply == RequestNameReply::PrimaryOwner => {
            let iface = A11yStatus::new();
            let conn2 = conn.clone();
            // zbus 4 blocking ObjectServer 注册（blocking::Connection::object_server）
            conn2.object_server().at("/org/a11y/bus", iface)?;
            Ok(())
        }
        Ok(_) => Err(anyhow::anyhow!("org.a11y.Status already owned by another service")),
        Err(e) => Err(anyhow::anyhow!("request_name failed: {e}")),
    }
}
