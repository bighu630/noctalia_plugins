//! AT-SPI 客户端：a11y 总线发现、应用定位、菜单树读取。
//!
//! 角色/状态 wire 常量来自 at-spi2-core `atspi-constants.h`（2026-05 实测核对）。
//! 参考实现：noctalia-appmenu bridge/src/atspi.rs（ADR-0024）。

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::sync::Arc;
use zbus::blocking::Connection;
use zvariant::OwnedObjectPath;

pub const ROLE_CHECK_MENU_ITEM: u32 = 8;
pub const ROLE_FRAME: u32 = 28;
pub const ROLE_MENU: u32 = 33;
pub const ROLE_MENU_BAR: u32 = 34;
pub const ROLE_MENU_ITEM: u32 = 35;
pub const ROLE_RADIO_MENU_ITEM: u32 = 45;
pub const ROLE_SEPARATOR: u32 = 50;
pub const ROLE_WINDOW: u32 = 15;

pub const STATE_ENABLED: u32 = 8;
pub const STATE_SENSITIVE: u32 = 24;
pub const STATE_VISIBLE: u32 = 31;
pub const STATE_CHECKED: u32 = 4;
pub const STATE_SHOWING: u32 = 25;

const IFACE_ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const IFACE_REGISTRY: &str = "org.a11y.atspi.Registry";
const A11Y_SERVICE: &str = "org.a11y.Bus";
const A11Y_PATH: &str = "/org/a11y/bus";
const MAX_DEPTH: usize = 8;

/// a11y 总线上一个 accessible 的坐标。
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibleRef {
    pub bus: String,
    pub path: OwnedObjectPath,
}

/// walker 的中间表示（与 zbus 调用解耦，便于纯函数测试）。
#[derive(Debug, Clone)]
pub struct RawNode {
    pub role: u32,
    pub name: String,
    pub state: (u32, u32),
    pub children: Vec<RawNode>,
    pub acc: Option<AccessibleRef>,
}

pub struct AtspiClient {
    conn: Connection,
}

impl AtspiClient {
    /// 通过会话总线的 org.a11y.Bus.GetAddress 发现并连接 a11y 总线。
    pub fn connect() -> Result<Self> {
        let session = Connection::session().context("session bus")?;
        let addr: String = session
            .call_method(Some(A11Y_SERVICE), A11Y_PATH, Some("org.a11y.Bus"), "GetAddress", &())
            .context("org.a11y.Bus.GetAddress")?
            .body()
            .deserialize()?;
        let conn = zbus::blocking::connection::Builder::address(addr.as_str())
            .context("a11y bus address")?
            .build()
            .context("a11y bus connect")?;
        Ok(Self { conn })
    }

    // ── 单节点原语 ──────────────────────────────────────────────

    fn get_role(&self, acc: &AccessibleRef) -> Result<u32> {
        Ok(self
            .conn
            .call_method(Some(acc.bus.as_str()), acc.path.as_str(), Some(IFACE_ACCESSIBLE), "GetRole", &())?
            .body()
            .deserialize()?)
    }

    fn get_name(&self, acc: &AccessibleRef) -> Result<String> {
        // 兼容两种 ATK 桥：标准实现提供 GetName 方法；
        // 实测（GIMP 3.2 环境）GetName 缺失但 Name 属性可用。
        if let Ok(r) = self
            .conn
            .call_method(Some(acc.bus.as_str()), acc.path.as_str(), Some(IFACE_ACCESSIBLE), "GetName", &())
        {
            if let Ok(name) = r.body().deserialize::<String>() {
                return Ok(name);
            }
        }
        let v = self
            .conn
            .call_method(
                Some(acc.bus.as_str()),
                acc.path.as_str(),
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.a11y.atspi.Accessible", "Name"),
            )?
            .body()
            .deserialize::<zvariant::OwnedValue>()?;
        let name: String = String::try_from(v).map_err(|_| anyhow!("Name is not string"))?;
        Ok(name)
    }

    /// GetState 的 wire 类型是 `au`（两个 u32 位词，位索引 = AtspiStateType 枚举值）。
    /// 注意：不能直接反序列化成 (u32, u32)——数组长度前缀会被静默读进第一个元素。
    fn get_state(&self, acc: &AccessibleRef) -> Result<(u32, u32)> {
        let states: Vec<u32> = self
            .conn
            .call_method(Some(acc.bus.as_str()), acc.path.as_str(), Some(IFACE_ACCESSIBLE), "GetState", &())?
            .body()
            .deserialize()?;
        Ok((states.first().copied().unwrap_or(0), states.get(1).copied().unwrap_or(0)))
    }

    fn child_count(&self, acc: &AccessibleRef) -> Result<i32> {
        // 实测：部分 at-spi2-registryd / ATK 实现不提供 GetChildCount 方法，
        // 但 ChildCount 属性通用存在（参考实现 noctalia-appmenu 同款路径）。
        let v = self
            .conn
            .call_method(
                Some(acc.bus.as_str()),
                acc.path.as_str(),
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &("org.a11y.atspi.Accessible", "ChildCount"),
            )?
            .body()
            .deserialize::<zvariant::OwnedValue>()?;
        let v: i32 = i32::try_from(v).map_err(|_| anyhow!("ChildCount is not int32"))?;
        Ok(v)
    }

    fn child_at(&self, acc: &AccessibleRef, i: i32) -> Result<Option<AccessibleRef>> {
        #[derive(Deserialize)]
        struct Child((String, OwnedObjectPath));
        let (bus, path): (String, OwnedObjectPath) = self
            .conn
            .call_method(
                Some(acc.bus.as_str()),
                acc.path.as_str(),
                Some(IFACE_ACCESSIBLE),
                "GetChildAtIndex",
                &(i,),
            )?
            .body()
            .deserialize()?;
        if bus.is_empty() {
            Ok(None)
        } else {
            Ok(Some(AccessibleRef { bus, path }))
        }
    }

    fn children(&self, acc: &AccessibleRef) -> Vec<AccessibleRef> {
        let mut out = Vec::new();
        if let Ok(n) = self.child_count(acc) {
            for i in 0..n {
                match self.child_at(acc, i) {
                    Ok(Some(c)) => out.push(c),
                    _ => break,
                }
            }
        }
        out
    }

    /// 递归读节点为 RawNode（深度受限，单节点失败跳过该分支）。
    fn read_node(&self, acc: &AccessibleRef, depth: usize) -> Option<RawNode> {
        if depth > MAX_DEPTH {
            return None;
        }
        let role = self.get_role(acc).ok()?;
        let name = self.get_name(acc).unwrap_or_default();
        let state = self.get_state(acc).unwrap_or((0, 0));
        let children = self
            .children(acc)
            .into_iter()
            .filter_map(|c| self.read_node(&c, depth + 1))
            .collect();
        Some(RawNode { role, name, state, children, acc: Some(acc.clone()) })
    }

    // ── 应用定位 ────────────────────────────────────────────────

    /// a11y 总线上所有注册应用（bus name, root accessible path）。
    /// 经 registry root accessible（/org/a11y/atspi/accessible/root）的 children
    /// 枚举；GetRegisteredApplications 在部分 registryd 缺失，root children
    /// 是通用路径（参考实现 noctalia-appmenu 同款）。
    /// null 哨兵（org.a11y.atspi.Registry + /org/a11y/atspi/null）过滤掉。
    fn registered_applications(&self) -> Result<Vec<(String, OwnedObjectPath)>> {
        let root = AccessibleRef {
            bus: IFACE_REGISTRY.to_string(),
            path: OwnedObjectPath::try_from("/org/a11y/atspi/accessible/root")?,
        };
        Ok(self
            .children(&root)
            .into_iter()
            .filter(|c| c.path.as_str() != "/org/a11y/atspi/null")
            .map(|c| (c.bus, c.path))
            .collect())
    }

    /// a11y 总线连接 → PID（与会话总线的 GetConnectionUnixProcessID 不同，a11y 总线是独立 daemon）。
    fn pid_of(&self, bus_name: &str) -> Result<u32> {
        // 显式 destination：实测 dbus-broker 拒绝无 destination 头的 method call
        // （zbus destination=None 不设头，规范允许但 dbus-broker 实现报 Invalid method call）。
        Ok(self
            .conn
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "GetConnectionUnixProcessID",
                &(bus_name,),
            )?
            .body()
            .deserialize()?)
    }

    /// 按 PID 找应用根 accessible。
    pub fn find_app_for_pid(&self, pid: u32) -> Result<Option<AccessibleRef>> {
        for (bus, path) in self.registered_applications()? {
            if self.pid_of(&bus).ok() == Some(pid) {
                return Ok(Some(AccessibleRef { bus, path }));
            }
        }
        Ok(None)
    }

    /// 多窗口同 PID：用 niri 焦点窗口 title 精确匹配 frame（ADR-0030：绝不猜）。
    /// 返回 Some(frame) = 从该 frame 找菜单；None = 从 app 根找；Err 仅网络层错误。
    pub fn choose_frame(&self, app: &AccessibleRef, title: &str) -> Result<Option<AccessibleRef>> {
        let frames = self.children(app);
        if frames.len() <= 1 {
            return Ok(None); // 单窗口：app 根
        }
        for f in &frames {
            let role = self.get_role(f).unwrap_or(0);
            if (role == ROLE_FRAME || role == ROLE_WINDOW) && self.get_name(f).ok().as_deref() == Some(title) {
                return Ok(Some(f.clone()));
            }
        }
        // 多窗口但匹配不到 → None 且上层判定"无法识别"，回退占位
        Ok(None)
    }

    /// 从起点 DFS 找 MENU_BAR（深度受限）。
    pub fn find_menubar(&self, root: &AccessibleRef) -> Result<Option<AccessibleRef>> {
        let mut stack = vec![root.clone()];
        for _ in 0..(1 << (MAX_DEPTH + 2)) {
            let Some(acc) = stack.pop() else { break };
            if self.get_role(&acc).unwrap_or(0) == ROLE_MENU_BAR {
                return Ok(Some(acc));
            }
            stack.extend(self.children(&acc).into_iter().rev());
        }
        Ok(None)
    }

    /// 完整链路：pid → app →（title 匹配 frame）→ menubar → RawNode 树。
    pub fn fetch_menubar(&self, pid: u32, title: &str) -> Result<Option<RawNode>> {
        let Some(app) = self.find_app_for_pid(pid)? else { return Ok(None) };
        let scope = match self.choose_frame(&app, title)? {
            Some(frame) => frame,
            None => app,
        };
        let Some(menubar) = self.find_menubar(&scope)? else { return Ok(None) };
        Ok(self.read_node(&menubar, 0))
    }
}

// ── 纯逻辑：RawNode → 统一菜单树 ───────────────────────────────

/// 可见性判据：MENU/MENU_BAR 是结构容器，总是保留（关闭的子菜单
/// VISIBLE/SHOWING 位为 0 也要出现在树里）；叶子项须带 VISIBLE 或
/// SHOWING 位，否则过滤（不占 id）。
fn is_visible(node: &RawNode) -> bool {
    node.role == ROLE_MENU
        || node.role == ROLE_MENU_BAR
        || node.state.0 & (1 << STATE_VISIBLE) != 0
        || node.state.0 & (1 << STATE_SHOWING) != 0
}

/// 由 RawNode 构建 MenuItem 树。ids 为 DFS 分配器（会话内连续）。
/// 规则：
/// - role==MENU_BAR/MENU 且 name 非空 → submenu（menubar 根 label 取空）
/// - MENU_ITEM → item；子节点为无名 MENU 时扁平化（Qt wrapper）
/// - CHECK/RADIO_MENU_ITEM → checkbox/radio（checked 取 state 位）
/// - SEPARATOR → separator
/// - 无 STATE_VISIBLE 的项过滤（不占 id）
/// - path = 相对根的 child-index 链（**原始** children 索引，含不可见项；与 locate_by_path 一致）
/// build_item 的公开包装（proxy::open_path 需要与主树同形的子项）。
pub fn build_item_pub(node: &RawNode, path: &[u32], ids: &mut u32) -> crate::protocol::MenuItem {
    build_item(node, path.to_vec(), ids)
}

pub fn build_menu_tree(root: &RawNode, ids: &mut u32) -> crate::protocol::MenuItem {
    use crate::protocol::{MenuItem, MenuItemType};
    let id = { *ids += 1; *ids };
    let path = vec![];
    let mut children = Vec::new();
    for (i, child) in root.children.iter().enumerate() {
        if !is_visible(child) {
            continue;
        }
        let child_path = {
            let mut p = path.clone();
            p.push(i as u32);
            p
        };
        let item = build_item(child, child_path, ids);
        children.push(item);
    }
    MenuItem {
        id,
        label: root.name.clone(),
        mnemonic: None,
        item_type: MenuItemType::Submenu,
        enabled: true,
        visible: true,
        checked: false,
        icon: None,
        children,
        path,
    }
}

fn build_item(node: &RawNode, path: Vec<u32>, ids: &mut u32) -> crate::protocol::MenuItem {
    use crate::protocol::{MenuItem, MenuItemType};
    let id = { *ids += 1; *ids };
    let enabled = node.state.0 & (1 << STATE_ENABLED) != 0
        && node.state.0 & (1 << STATE_SENSITIVE) != 0;
    let checked = node.state.0 & (1 << STATE_CHECKED) != 0;

    let is_submenu_like = node.role == ROLE_MENU_BAR || node.role == ROLE_MENU;
    let mut children = Vec::new();
    if is_submenu_like {
        for (i, child) in node.children.iter().enumerate() {
            if !is_visible(child) {
                continue;
            }
            let mut child_path = path.clone();
            child_path.push(i as u32);
            children.push(build_item(child, child_path, ids));
        }
    } else {
        // item 的 children：Qt 包装的无名 MENU → 扁平化为 submenu
        let mut popup_children: Vec<&RawNode> = Vec::new();
        for child in &node.children {
            if child.role == ROLE_MENU && child.name.is_empty() {
                popup_children.extend(child.children.iter());
            } else {
                popup_children.push(child);
            }
        }
        for (i, child) in popup_children.iter().enumerate() {
            if !is_visible(child) {
                continue;
            }
            let mut child_path = path.clone();
            child_path.push(i as u32);
            children.push(build_item(child, child_path, ids));
        }
    }

    // "有 children" 是 submenu 的规范判据（Qt 顶层项是带 popup MENU 的
    // MENU_ITEM，role 会把它们误标为普通 item）——与生产参考实现一致。
    let item_type = if node.role == ROLE_SEPARATOR {
        MenuItemType::Separator
    } else if is_submenu_like || !children.is_empty() {
        MenuItemType::Submenu
    } else if node.role == ROLE_CHECK_MENU_ITEM {
        MenuItemType::Checkbox
    } else if node.role == ROLE_RADIO_MENU_ITEM {
        MenuItemType::Radio
    } else {
        MenuItemType::Item
    };

    MenuItem {
        id,
        label: node.name.clone(),
        mnemonic: None,
        item_type,
        enabled,
        visible: true,
        checked,
        icon: None,
        children,
        path,
    }
}

pub type SharedAtspi = Arc<AtspiClient>;

// ── 点击侧 ─────────────────────────────────────────────────────

/// 按 child-index 链定位节点（相对 menubar 根；索引为**全部子节点**索引，
/// 与 build_menu_tree 的 path 语义一致）。
pub fn locate_by_path<'a>(root: &'a RawNode, path: &[u32]) -> Option<&'a RawNode> {
    let mut cur = root;
    for &i in path {
        cur = cur.children.get(i as usize)?;
    }
    Some(cur)
}

impl AtspiClient {
    /// 重新执行完整解析（pid → app → frame → menubar → RawNode），按 path 定位，
    /// 再对目标做 DoAction。返回 (是否找到, 是否执行成功)。
    pub fn click_path(&self, pid: u32, title: &str, path: &[u32]) -> Result<(bool, bool)> {
        let Some(app) = self.find_app_for_pid(pid)? else { return Ok((false, false)) };
        let scope = match self.choose_frame(&app, title)? {
            Some(frame) => frame,
            None => app,
        };
        let Some(menubar) = self.find_menubar(&scope)? else { return Ok((false, false)) };
        let Some(root) = self.read_node(&menubar, 0) else { return Ok((false, false)) };
        let Some(target) = locate_by_path(&root, path) else { return Ok((false, false)) };
        let Some(acc) = &target.acc else { return Ok((false, false)) };
        let ok = self.do_action(acc)?;
        Ok((true, ok))
    }

    /// 重新解析并按 path 读取节点直接子项（懒构建兜底，/open 用）。
    /// 返回 (找到, 子树 RawNode 的 children)。
    pub fn read_children_by_path(
        &self,
        pid: u32,
        title: &str,
        path: &[u32],
    ) -> Result<(bool, Vec<RawNode>)> {
        let Some(app) = self.find_app_for_pid(pid)? else { return Ok((false, vec![])) };
        let scope = match self.choose_frame(&app, title)? {
            Some(frame) => frame,
            None => app,
        };
        let Some(menubar) = self.find_menubar(&scope)? else { return Ok((false, vec![])) };
        let Some(root) = self.read_node(&menubar, 0) else { return Ok((false, vec![])) };
        let Some(target) = locate_by_path(&root, path) else { return Ok((false, vec![])) };
        let Some(acc) = &target.acc else { return Ok((false, vec![])) };
        // 子项按 RawNode 读取（与主树 children 同形）；深度 = 目标深度 + 1。
        let depth = path.len() + 1;
        let children = self
            .children(acc)
            .into_iter()
            .filter_map(|c| self.read_node(&c, depth))
            .collect();
        Ok((true, children))
    }

    /// AT-SPI Action 接口点击。qatspi 约定索引 0 = "click"；
    /// 兜底：枚举动作名匹配 "click"，否则尝试 0。返回执行结果。
    fn do_action(&self, acc: &AccessibleRef) -> Result<bool> {
        // 兼容两种 ATK 桥：标准 at-spi2 提供 GetActionCount/GetActionName 方法；
        // 实测（GIMP 3.2 环境）二者缺失，但 NActions 属性 + GetName(i)（qspi 风格）可用。
        let count: i32 = self
            .conn
            .call_method(Some(acc.bus.as_str()), acc.path.as_str(), Some("org.a11y.atspi.Action"), "GetActionCount", &())
            .ok()
            .and_then(|r| r.body().deserialize::<i32>().ok())
            .unwrap_or(0);
        let count = if count > 0 {
            count
        } else {
            let v = self
                .conn
                .call_method(
                    Some(acc.bus.as_str()),
                    acc.path.as_str(),
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &("org.a11y.atspi.Action", "NActions"),
                )?
                .body()
                .deserialize::<zvariant::OwnedValue>()?;
            i32::try_from(v).unwrap_or(0)
        };
        if count <= 0 {
            return Ok(false);
        }
        let mut idx = -1i32;
        for i in 0..count {
            let name: String = self
                .conn
                .call_method(Some(acc.bus.as_str()), acc.path.as_str(), Some("org.a11y.atspi.Action"), "GetActionName", &(i,))
                .ok()
                .and_then(|r| r.body().deserialize::<String>().ok())
                .or_else(|| {
                    self.conn
                        .call_method(Some(acc.bus.as_str()), acc.path.as_str(), Some("org.a11y.atspi.Action"), "GetName", &(i,))
                        .ok()
                        .and_then(|r| r.body().deserialize::<String>().ok())
                })
                .unwrap_or_default();
            if name.to_ascii_lowercase().contains("click") {
                idx = i;
                break;
            }
        }
        if idx < 0 {
            idx = 0;
        }
        let ok: bool = self
            .conn
            .call_method(Some(acc.bus.as_str()), acc.path.as_str(), Some("org.a11y.atspi.Action"), "DoAction", &(idx,))?
            .body()
            .deserialize()?;
        Ok(ok)
    }
}
