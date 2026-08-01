use noctalia_global_menu_bridge::proxy::{FocusInfo, make_menu_event, build_children_response};
use noctalia_global_menu_bridge::protocol::{AppInfo, MenuItem, MenuItemType};

fn item(id: u32, label: &str, item_type: MenuItemType, children: Vec<MenuItem>) -> MenuItem {
    MenuItem { id, label: label.into(), mnemonic: None, item_type, enabled: true, visible: true, checked: false, icon: None, children, path: vec![] }
}

#[test]
fn menu_event_carries_app_and_tree() {
    let focus = FocusInfo { win_id: 5, app_id: "gimp".into(), title: "GIMP".into(), pid: 42 };
    let menu = Some(item(1, "", MenuItemType::Submenu, vec![item(2, "File", MenuItemType::Submenu, vec![item(3, "New", MenuItemType::Item, vec![])])]));
    let ev = make_menu_event(&focus, menu, "atspi");
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "menu");
    assert_eq!(v["app"]["pid"], 42);
    assert_eq!(v["app"]["app_id"], "gimp");
    assert_eq!(v["source"], "atspi");
    assert_eq!(v["menu"]["children"][0]["label"], "File");
}

#[test]
fn no_menu_yields_null_menu_with_none_source() {
    let focus = FocusInfo { win_id: 5, app_id: "chrome".into(), title: "Chrome".into(), pid: 7 };
    let ev = make_menu_event(&focus, None, "none");
    let v = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["menu"], serde_json::Value::Null);
    assert_eq!(v["source"], "none");
}

#[test]
fn children_response_uses_same_id_space() {
    // /open 响应：children 数组 + ok 标记（id 从解析后的树取）
    let children = vec![item(3, "Zoom", MenuItemType::Item, vec![])];
    let resp = build_children_response(true, children);
    let v = serde_json::to_value(&resp).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["children"][0]["id"], 3);
}
