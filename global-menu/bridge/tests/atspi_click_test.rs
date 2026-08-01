use noctalia_global_menu_bridge::atspi::{locate_by_path, RawNode, ROLE_MENU, ROLE_MENU_BAR, ROLE_MENU_ITEM};

fn node(role: u32, name: &str, children: Vec<RawNode>) -> RawNode {
    RawNode { role, name: name.into(), state: (0, 0), children, acc: None }
}

#[test]
fn locates_by_child_index_path() {
    // menubar [File[New, Open], Edit[Undo]]，path [1, 0] → Undo
    let root = node(ROLE_MENU_BAR, "", vec![
        node(ROLE_MENU, "File", vec![
            node(ROLE_MENU_ITEM, "New", vec![]),
            node(ROLE_MENU_ITEM, "Open", vec![]),
        ]),
        node(ROLE_MENU, "Edit", vec![
            node(ROLE_MENU_ITEM, "Undo", vec![]),
        ]),
    ]);
    let target = locate_by_path(&root, &[1, 0]).expect("found");
    assert_eq!(target.name, "Undo");
}

#[test]
fn missing_path_returns_none() {
    let root = node(ROLE_MENU_BAR, "", vec![node(ROLE_MENU, "A", vec![])]);
    assert!(locate_by_path(&root, &[0, 5]).is_none());
    assert!(locate_by_path(&root, &[3]).is_none());
    assert!(locate_by_path(&root, &[]).is_some()); // 根本身
}
