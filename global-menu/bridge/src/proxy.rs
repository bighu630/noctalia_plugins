//! 会话管理：焦点信息 → 菜单树解析 → 事件组装。
//!
//! 桥无状态：每次焦点变化/点击/展开都从 niri + AT-SPI 重新解析。
//! id 语义 = DFS 解析序；path 语义 = 相对 menubar 的 child-index 链（点击/展开提交 path）。

use crate::atspi::{build_menu_tree, AtspiClient};
use crate::protocol::{AppInfo, BridgeEvent, MenuItem, MenuItemType};
use anyhow::Result;

#[derive(Debug, Clone, PartialEq)]
pub struct FocusInfo {
    pub win_id: u64,
    pub app_id: String,
    pub title: String,
    pub pid: u32,
}

/// 焦点变化处理：解析菜单并产出事件。menu=None 表示无菜单（回退占位）。
pub fn resolve_focus(atspi: &AtspiClient, focus: &FocusInfo) -> Result<Option<MenuItem>> {
    let Some(raw) = atspi.fetch_menubar(focus.pid, &focus.title)? else {
        return Ok(None);
    };
    let mut ids = 0u32;
    Ok(Some(build_menu_tree(&raw, &mut ids)))
}

pub fn make_menu_event(focus: &FocusInfo, menu: Option<MenuItem>, source: &'static str) -> BridgeEvent {
    BridgeEvent::Menu {
        app: AppInfo {
            app_id: focus.app_id.clone(),
            title: focus.title.clone(),
            pid: focus.pid,
        },
        menu,
        source,
    }
}

/// /open 响应体。
#[derive(serde::Serialize)]
pub struct ChildrenResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<MenuItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn build_children_response(ok: bool, children: Vec<MenuItem>) -> ChildrenResponse {
    ChildrenResponse { ok, children, error: None }
}

/// /click 响应体。
#[derive(serde::Serialize)]
pub struct ClickResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 点击：完整重解析 + path 定位 + DoAction（返回是否找到/执行）。
pub fn click_path(atspi: &AtspiClient, focus: &FocusInfo, path: &[u32]) -> Result<(bool, bool)> {
    atspi.click_path(focus.pid, &focus.title, path)
}

/// 展开兜底：重解析 + path 定位 + 读子项，转统一模型（id 空间与一次全新解析一致）。
/// 子项 path = 父 path + 原始 child-index（点击/再展开可直接提交）。
pub fn open_path(atspi: &AtspiClient, focus: &FocusInfo, path: &[u32]) -> Result<(bool, Vec<MenuItem>)> {
    let (found, raws) = atspi.read_children_by_path(focus.pid, &focus.title, path)?;
    if !found {
        return Ok((false, vec![]));
    }
    let mut ids = 0u32;
    let mut items = Vec::new();
    for raw in raws {
        // 每个子项与主树 children 同形：build_item 而非 build_menu_tree（后者是容器）。
        // 传父 path：子项 path = path + [原始索引]，保证 /click、/open 可直接提交。
        items.push(crate::atspi::build_item_pub(&raw, path, &mut ids));
    }
    Ok((true, items))
}

use std::sync::{Arc, Mutex};

/// 跨线程共享状态（proxy 主循环写，HTTP 线程读）。
#[derive(Default)]
pub struct Shared {
    pub session: Option<Session>,
}

#[derive(Clone)]
pub struct Session {
    pub focus: FocusInfo,
    pub menu: Option<MenuItem>,
}
