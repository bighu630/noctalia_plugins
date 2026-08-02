use noctalia_global_menu_bridge::niri::{NiriEvent, NiriWindow, parse_event_line, focused_window_from_json};

#[test]
fn parses_externally_tagged_focus_event() {
    // niri 26.04 实测线格式
    let line = r#"{"WindowFocusChanged":{"id":30}}"#;
    match parse_event_line(line).unwrap() {
        NiriEvent::WindowFocusChanged { id } => assert_eq!(id, 30),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn unknown_event_falls_through_to_other() {
    // schema 漂移绝不崩溃（ADR-0016 教训）
    let line = r#"{"SomeFutureEvent":{"x":1}}"#;
    assert!(matches!(parse_event_line(line).unwrap(), NiriEvent::Other));
    let line2 = r#"{"WindowFocusChanged":{"id":30},"ExtraKey":1}"#;
    assert!(matches!(parse_event_line(line2).unwrap(), NiriEvent::Other));
    let line3 = r#"not json at all"#;
    assert!(parse_event_line(line3).is_err());
}

#[test]
fn parses_windows_changed_with_full_window_list() {
    let line = r#"{"WindowsChanged":{"windows":[
      {"id":30,"title":"pi-web-access","app_id":"google-chrome-canary","pid":1310790,"workspace_id":2,"is_focused":false,"is_floating":false,"is_urgent":false,"layout":{},"focus_timestamp":{"secs":1,"nanos":2}},
      {"id":34,"title":"PiPlus","app_id":"piplus","pid":1377900,"workspace_id":2,"is_focused":true,"is_floating":false,"is_urgent":false,"layout":{},"focus_timestamp":{"secs":1,"nanos":2}}
    ]}}"#;
    match parse_event_line(line).unwrap() {
        NiriEvent::WindowsChanged { windows } => {
            assert_eq!(windows.len(), 2);
            assert_eq!(windows[1].id, 34);
            assert_eq!(windows[1].pid, 1377900);
            assert!(windows[1].is_focused);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_workspace_active_window_event() {
    // M2 焦点兜底：层表面抢焦后焦点回到窗口时 niri 发 WorkspaceActiveWindowChanged
    let line = r#"{"WorkspaceActiveWindowChanged":{"workspace_id":2,"active_window_id":34}}"#;
    match parse_event_line(line).unwrap() {
        NiriEvent::WorkspaceActiveWindowChanged { workspace_id, active_window_id } => {
            assert_eq!(workspace_id, 2);
            assert_eq!(active_window_id, Some(34));
        }
        other => panic!("unexpected: {other:?}"),
    }
    // 焦点在层表面（launcher）上时 active_window_id 为 null
    let line2 = r#"{"WorkspaceActiveWindowChanged":{"workspace_id":2,"active_window_id":null}}"#;
    match parse_event_line(line2).unwrap() {
        NiriEvent::WorkspaceActiveWindowChanged { active_window_id, .. } => {
            assert_eq!(active_window_id, None);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn focused_window_extraction() {
    let windows = vec![
        NiriWindow { id: 1, title: "a".into(), app_id: "x".into(), pid: 10, workspace_id: Some(1), is_focused: false },
        NiriWindow { id: 2, title: "b".into(), app_id: "y".into(), pid: 20, workspace_id: Some(1), is_focused: true },
    ];
    assert_eq!(focused_window_from_json(&windows).unwrap().id, 2);
    assert!(focused_window_from_json(&vec![]).is_none());
}
