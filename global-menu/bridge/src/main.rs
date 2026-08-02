//! Global Menu bridge 主程序。
//!
//! 启动顺序：会话总线 → org.a11y.Status（尽力）→ a11y 连接（失败退出，
//! 插件 ping 失败会重启桥）→ HTTP → hello → niri 订阅 → 事件循环。
//!
//! 线程：main=事件循环；niri 线程（event-stream 读取）；心跳由事件循环
//! recv_timeout 驱动；HTTP 线程（tiny_http 自管）。

mod atspi;
mod http;
mod niri;
mod protocol;
mod proxy;
mod stdout_io;
mod status;

use crate::atspi::{AtspiClient, SharedAtspi};
use crate::niri::{parse_event_line, NiriEvent, NiriWindow};
use crate::protocol::BridgeEvent;
use crate::proxy::{resolve_focus, FocusInfo, Shared};
use anyhow::Result;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEBOUNCE_MS: u64 = 150;
const HEARTBEAT_SECS: u64 = 5;

enum Ctrl {
    FocusChanged(u64),
    WorkspaceActive(Option<u64>),
    Windows(Vec<NiriWindow>),
    Refresh,
    Quit,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("[global-menu-bridge] fatal: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    // 1. 会话总线 + a11y 总线
    let session = zbus::blocking::Connection::session()?;
    match status::own_status(&session) {
        Ok(()) => eprintln!("[global-menu-bridge] org.a11y.Status owned"),
        Err(e) => eprintln!("[global-menu-bridge] org.a11y.Status unavailable (non-fatal): {e:#}"),
    }
    let atspi: SharedAtspi = Arc::new(AtspiClient::connect().map_err(|e| anyhow::anyhow!("AT-SPI unavailable: {e:#}"))?);

    // 2. 共享状态 + HTTP
    let (refresh_tx, refresh_rx) = mpsc::channel::<()>();
    let (focus_tx, focus_rx) = mpsc::channel::<Ctrl>();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let http_server = http::spawn(shared.clone(), atspi.clone(), refresh_tx)?;
    let sink = stdout_io::StdoutSink::new();

    // /refresh 通道转发到主循环（std mpsc 无 select，转发线程桥接）
    let focus_tx3 = focus_tx.clone();
    thread::spawn(move || {
        while refresh_rx.recv().is_ok() {
            let _ = focus_tx3.send(Ctrl::Refresh);
        }
    });

    // 3. hello
    sink.emit(&BridgeEvent::Hello { port: http_server.port, pid: std::process::id() });

    // 4. niri 订阅线程（spawn/读流失败 → error 事件上报，插件可见而非静默死亡）
    let focus_tx2 = focus_tx.clone();
    let sink_niri = sink.clone();
    thread::spawn(move || {
        if let Err(e) = niri_event_loop(focus_tx2) {
            sink_niri.emit(&BridgeEvent::Error { msg: format!("niri event stream failed: {e:#}") });
        }
    });

    // 5. 初始 windows 快照（缓存由事件流维护，这里兜底）
    if let Ok(windows) = niri::query_windows() {
        let _ = focus_tx.send(Ctrl::Windows(windows));
    }

    // 6. 主循环：去抖 + 心跳 + refresh
    let mut pending_focus: Option<u64> = None;
    let mut window_cache: Vec<NiriWindow> = Vec::new();
    let mut debounce_deadline: Option<std::time::Instant> = None;

    loop {
        let wait = match debounce_deadline {
            Some(d) => d.saturating_duration_since(std::time::Instant::now()),
            None => Duration::from_secs(HEARTBEAT_SECS),
        };
        let msg = focus_rx.recv_timeout(wait.max(Duration::from_millis(20)));
        match msg {
            Ok(Ctrl::FocusChanged(id)) => {
                pending_focus = Some(id);
                debounce_deadline = Some(std::time::Instant::now() + Duration::from_millis(DEBOUNCE_MS));
            }
            Ok(Ctrl::WorkspaceActive(id)) => {
                // 焦点来源兜底：层表面（launcher 等）抢焦时不发 WindowFocusChanged，
                // is_focused 全 false 导致菜单停摆；焦点回到窗口时靠
                // WorkspaceActiveWindowChanged 恢复解析（与 FocusChanged 同一去抖路径）。
                if let Some(id) = id {
                    // R1 门控：WorkspaceActiveWindowChanged 是**任意工作区**的活动窗口变化，
                    // 后台工作区窗口开/关不应抢当前菜单。仅当与当前 session 窗口同工作区
                    // （或无 session 的初始恢复）时才接受。
                    let same_workspace = {
                        let st = shared.lock().unwrap();
                        match st.session.as_ref().map(|s| s.focus.win_id) {
                            Some(cur) => {
                                let cur_ws = window_cache.iter().find(|w| w.id == cur).and_then(|w| w.workspace_id);
                                let new_ws = window_cache.iter().find(|w| w.id == id).and_then(|w| w.workspace_id);
                                cur_ws.is_none() || new_ws.is_none() || cur_ws == new_ws
                            }
                            None => true, // 无 session：初始恢复场景，接受
                        }
                    };
                    let stale = shared
                        .lock()
                        .unwrap()
                        .session
                        .as_ref()
                        .map(|s| s.focus.win_id != id)
                        .unwrap_or(true);
                    if same_workspace && stale {
                        pending_focus = Some(id);
                        debounce_deadline = Some(std::time::Instant::now() + Duration::from_millis(DEBOUNCE_MS));
                    }
                }
            }
            Ok(Ctrl::Windows(w)) => {
                window_cache = w;
                // 初始快照/窗口列表刷新：若当前焦点窗口与已解析会话不一致，立即解析。
                // （修复启动竞态：焦点切换事件先于窗口快照到达时会 miss，靠这里兜底）
                if let Some(f) = current_focus(&window_cache) {
                    let stale = shared
                        .lock()
                        .unwrap()
                        .session
                        .as_ref()
                        .map(|s| s.focus.win_id != f.win_id)
                        .unwrap_or(true);
                    if stale {
                        handle_focus(&atspi, &sink, &shared, f)?;
                    }
                }
            }
            Ok(Ctrl::Refresh) => {
                // 立即按当前焦点重解析
                if let Some(focus) = current_focus(&window_cache) {
                    handle_focus(&atspi, &sink, &shared, focus)?;
                }
            }
            Ok(Ctrl::Quit) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // 心跳
                sink.emit(&BridgeEvent::Heartbeat { ts: unix_now() });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // 去抖到期 → 处理焦点
        if let Some(d) = debounce_deadline {
            if std::time::Instant::now() >= d {
                if let Some(id) = pending_focus.take() {
                    if let Some(focus) = focus_from_cache(&window_cache, id) {
                        handle_focus(&atspi, &sink, &shared, focus)?;
                    } else {
                        // 缓存 miss（启动竞态）：现场查询全量快照再解析
                        if let Ok(windows) = niri::query_windows() {
                            window_cache = windows;
                            if let Some(focus) = focus_from_cache(&window_cache, id) {
                                handle_focus(&atspi, &sink, &shared, focus)?;
                            }
                        }
                    }
                }
                debounce_deadline = None;
            }
        }
    }

    http_server.stop();
    Ok(())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn current_focus(cache: &[NiriWindow]) -> Option<FocusInfo> {
    niri::focused_window_from_json(cache).map(focus_of)
}

fn focus_from_cache(cache: &[NiriWindow], id: u64) -> Option<FocusInfo> {
    cache.iter().find(|w| w.id == id).map(focus_of)
}

fn focus_of(w: &NiriWindow) -> FocusInfo {
    FocusInfo { win_id: w.id, app_id: w.app_id.clone(), title: w.title.clone(), pid: w.pid }
}

fn handle_focus(
    atspi: &AtspiClient,
    sink: &stdout_io::StdoutSink,
    shared: &Arc<Mutex<Shared>>,
    focus: FocusInfo,
) -> Result<()> {
    // 先更新当前焦点（解析全树耗时数秒，HTTP /click /open 必须尽早拿到新焦点，
    // 否则解析期间请求会命中过期 session——实测竞态）。
    {
        let mut st = shared.lock().unwrap();
        st.focus = Some(focus.clone());
    }
    let menu = match resolve_focus(atspi, &focus) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[global-menu-bridge] resolve failed: {e:#}");
            None
        }
    };
    {
        let mut st = shared.lock().unwrap();
        st.session = Some(proxy::Session { focus: focus.clone(), menu: menu.clone() });
    }
    let source = if menu.is_some() { "atspi" } else { "none" };
    sink.emit(&proxy::make_menu_event(&focus, menu, source));
    Ok(())
}

/// 订阅 niri event-stream；本线程内任何错误（进程退出/schema 变更）记录后结束。
fn niri_event_loop(tx: Sender<Ctrl>) -> Result<()> {
    let mut child = Command::new("niri")
        .args(["msg", "--json", "event-stream"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let line = line?;
        match parse_event_line(&line) {
            Ok(NiriEvent::WindowFocusChanged { id }) => {
                let _ = tx.send(Ctrl::FocusChanged(id));
            }
            Ok(NiriEvent::WorkspaceActiveWindowChanged { active_window_id, .. }) => {
                // 层表面抢焦时焦点事件缺失的兜底来源（主循环按 stale 判定去抖）
                let _ = tx.send(Ctrl::WorkspaceActive(active_window_id));
            }
            Ok(NiriEvent::WindowsChanged { windows }) => {
                let _ = tx.send(Ctrl::Windows(windows));
            }
            Ok(_) => {}
            Err(e) => eprintln!("[global-menu-bridge] niri line parse: {e:#}"),
        }
    }
    // 进程退出（niri 重启/socket 断）→ 让主循环退出
    let _ = child.wait();
    let _ = tx.send(Ctrl::Quit);
    Ok(())
}
