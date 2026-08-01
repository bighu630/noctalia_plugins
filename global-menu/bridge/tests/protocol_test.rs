use noctalia_global_menu_bridge::protocol::*;

#[test]
fn menu_item_serializes_with_expected_shape() {
    let item = MenuItem {
        id: 3,
        label: "Export…".into(),
        mnemonic: Some("E".into()),
        item_type: MenuItemType::Item,
        enabled: true,
        visible: true,
        checked: false,
        icon: None,
        children: vec![],
        path: vec![],
    };
    let v: serde_json::Value = serde_json::to_value(&item).unwrap();
    assert_eq!(v["id"], 3);
    assert_eq!(v["label"], "Export…");
    assert_eq!(v["type"], "item");
    assert_eq!(v["checked"], false);
    assert!(v.get("mnemonic").is_some());
    assert!(v.get("icon").is_none()); // None 字段 skip
}

#[test]
fn bridge_event_menu_uses_internal_tag() {
    let ev = BridgeEvent::Menu {
        app: AppInfo { app_id: "gimp".into(), title: "GIMP".into(), pid: 1234 },
        menu: None,
        source: "none",
    };
    let v: serde_json::Value = serde_json::to_value(&ev).unwrap();
    assert_eq!(v["type"], "menu");
    assert_eq!(v["app"]["app_id"], "gimp");
    assert_eq!(v["menu"], serde_json::Value::Null);
    assert_eq!(v["source"], "none");
}

#[test]
fn checkbox_and_separator_types() {
    assert_eq!(serde_json::to_string(&MenuItemType::Checkbox).unwrap(), "\"checkbox\"");
    assert_eq!(serde_json::to_string(&MenuItemType::Separator).unwrap(), "\"separator\"");
    assert_eq!(serde_json::to_string(&MenuItemType::Submenu).unwrap(), "\"submenu\"");
}
