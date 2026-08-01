use serde::Serialize;

/// 统一菜单项模型（桥 → 插件）。
/// id 为桥按 DFS 序分配的会话内稳定序号；path 为相对菜单栏的 child-index 链，
/// 点击/展开时提交 path，桥重解析后按 path 定位（树变化时比序号健壮）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MenuItem {
    pub id: u32,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mnemonic: Option<String>,
    #[serde(rename = "type")]
    pub item_type: MenuItemType,
    pub enabled: bool,
    pub visible: bool,
    pub checked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<MenuItem>,
    /// child-index 链（相对 menubar 根），如 [3, 1, 2]。
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub path: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MenuItemType {
    Item,
    Submenu,
    Separator,
    Checkbox,
    Radio,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AppInfo {
    pub app_id: String,
    pub title: String,
    pub pid: u32,
}

/// 桥 → 插件（stdout 每行一个）的 NDJSON 事件。
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeEvent {
    Hello { port: u16, pid: u32 },
    Menu {
        app: AppInfo,
        #[serde(skip_serializing_if = "Option::is_none")]
        menu: Option<MenuItem>,
        source: &'static str,
    },
    Heartbeat { ts: u64 },
    Error { msg: String },
}

impl BridgeEvent {
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("BridgeEvent serialization cannot fail")
    }
}
