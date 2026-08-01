//! niri IPC 客户端：`niri msg --json event-stream` 事件解析 + windows 快照。
//!
//! niri 26.04 事件为**外部标签** JSON（`{"WindowFocusChanged":{"id":30}}`）。
//! 未知事件/未知字段一律落 `Other`，绝不崩溃（ADR-0016 教训）。

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum NiriEvent {
    WindowFocusChanged { id: u64 },
    WindowFocusTimestampChanged { id: u64 },
    WorkspaceActiveWindowChanged { workspace_id: u64, active_window_id: Option<u64> },
    WindowsChanged { windows: Vec<NiriWindow> },
    Other,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct NiriWindow {
    pub id: u64,
    pub title: String,
    #[serde(rename = "app_id")]
    pub app_id: String,
    pub pid: u32,
    #[serde(default)]
    pub workspace_id: Option<u64>,
    #[serde(rename = "is_focused", default)]
    pub is_focused: bool,
}

/// 解析 event-stream 的一行。任何无法精确匹配的输入 → Other，绝不 panic。
pub fn parse_event_line(line: &str) -> Result<NiriEvent> {
    let v: Value = serde_json::from_str(line)?;
    let map = v.as_object().ok_or_else(|| anyhow!("event line is not an object"))?;
    if map.len() != 1 {
        return Ok(NiriEvent::Other);
    }
    let (tag, payload) = map.iter().next().expect("len checked");
    let get = |key: &str| payload.get(key).and_then(Value::as_u64);
    Ok(match tag.as_str() {
        "WindowFocusChanged" => NiriEvent::WindowFocusChanged { id: get("id").unwrap_or(0) },
        "WindowFocusTimestampChanged" => NiriEvent::WindowFocusTimestampChanged { id: get("id").unwrap_or(0) },
        "WorkspaceActiveWindowChanged" => NiriEvent::WorkspaceActiveWindowChanged {
            workspace_id: get("workspace_id").unwrap_or(0),
            active_window_id: payload.get("active_window_id").and_then(Value::as_u64),
        },
        "WindowsChanged" => NiriEvent::WindowsChanged {
            windows: serde_json::from_value(payload.get("windows").cloned().unwrap_or(Value::Array(vec![])))
                .unwrap_or_default(),
        },
        _ => NiriEvent::Other,
    })
}

/// 从窗口列表找当前焦点窗口。
pub fn focused_window_from_json(windows: &[NiriWindow]) -> Option<&NiriWindow> {
    windows.iter().find(|w| w.is_focused)
}

/// 查询 `niri msg --json windows` 全量快照。
/// 失败（niri 不在运行/NIRI_SOCKET 未设）返回 Err，调用方降级处理。
pub fn query_windows() -> Result<Vec<NiriWindow>> {
    let out = Command::new("niri")
        .args(["msg", "--json", "windows"])
        .output()?;
    if !out.status.success() {
        return Err(anyhow!("niri msg failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let windows: Vec<NiriWindow> = serde_json::from_slice(&out.stdout)?;
    Ok(windows)
}
