//! 本地回环 HTTP 命令服务（插件下行通道）。
//! 端点：GET /ping、POST /click、POST /open、POST /refresh、POST /shutdown。

use crate::atspi::SharedAtspi;
use crate::proxy::{build_children_response, click_path, open_path, Shared};
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::io::Read;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Deserialize)]
struct PathBody {
    path: Vec<u32>,
}

#[derive(Clone)]
pub struct HttpServer {
    pub port: u16,
    shutdown: Arc<Mutex<Option<Sender<()>>>>,
}

pub fn spawn(
    shared: Arc<Mutex<Shared>>,
    atspi: SharedAtspi,
    refresh_tx: Sender<()>,
) -> Result<HttpServer> {
    // tiny_http 0.12：Server 非 Clone，shutdown/unblock 与请求循环共享 Arc。
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| anyhow!("http server: {e}"))?;
    let server = Arc::new(server);
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(0);
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
    let shutdown = Arc::new(Mutex::new(Some(shutdown_tx)));

    let server_for_sd = server.clone();
    thread::spawn(move || {
        let _ = shutdown_rx.recv();
        server_for_sd.unblock();
    });
    thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let shared = shared.clone();
            let atspi = atspi.clone();
            let refresh_tx = refresh_tx.clone();
            // 每请求一线程：/click 全树重解析耗时数秒，不能阻塞 /ping 心跳
            // 探测（否则插件误判桥死亡而重启）。shared/atspi/refresh_tx 均可 clone。
            thread::spawn(move || {
                let response = handle(&mut request, shared, atspi, refresh_tx);
                let _ = request.respond(response);
            });
        }
    });

    Ok(HttpServer { port, shutdown })
}

fn handle(
    request: &mut tiny_http::Request,
    shared: Arc<Mutex<Shared>>,
    atspi: SharedAtspi,
    refresh_tx: Sender<()>,
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let path = request.url().split('?').next().unwrap_or("/").to_string();
    let method = request.method().clone();
    let json = |code: u16, v: serde_json::Value| {
        tiny_http::Response::from_string(v.to_string())
            .with_status_code(code)
            .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
    };
    // body 读取上限：防御异常大请求（超出直接 413，不解析）
    const MAX_BODY_BYTES: u64 = 64 * 1024;
    let mut body = String::new();
    let _ = request.as_reader().take(MAX_BODY_BYTES + 1).read_to_string(&mut body);
    if body.len() as u64 > MAX_BODY_BYTES {
        return json(413, serde_json::json!({"ok": false, "error": "body too large"}));
    }

    let result: serde_json::Value = match (method, path.as_str()) {
        (_, "/ping") => serde_json::json!({"ok": true}),

        (_, "/shutdown") => {
            std::process::exit(0);
        }

        (_, "/click") => {
            let st = shared.lock().unwrap();
            let Some(focus) = st.focus.clone().or_else(|| st.session.as_ref().map(|s| s.focus.clone())) else {
                return json(200, serde_json::json!({"ok": false, "error": "no session"}));
            };
            match serde_json::from_str::<PathBody>(&body) {
                Ok(b) => match click_path(&atspi, &focus, &b.path) {
                    Ok((found, clicked)) => {
                        let ok = found && clicked;
                        if ok {
                            // 点击成功后自动重拉并补发 menu 事件（设计承诺的勾选/禁用状态同步）
                            let _ = refresh_tx.send(());
                        }
                        serde_json::json!({"ok": ok, "found": found, "clicked": clicked})
                    }
                    Err(e) => serde_json::json!({"ok": false, "error": format!("{e:#}")}),
                },
                Err(e) => serde_json::json!({"ok": false, "error": format!("bad body: {e}")}),
            }
        }

        (_, "/open") => {
            let st = shared.lock().unwrap();
            let Some(focus) = st.focus.clone().or_else(|| st.session.as_ref().map(|s| s.focus.clone())) else {
                return json(200, serde_json::json!({"ok": false, "error": "no session"}));
            };
            match serde_json::from_str::<PathBody>(&body) {
                Ok(b) => match open_path(&atspi, &focus, &b.path) {
                    Ok((found, items)) => {
                        if found {
                            serde_json::to_value(build_children_response(true, items)).unwrap()
                        } else {
                            serde_json::json!({"ok": false, "error": "path not found"})
                        }
                    }
                    Err(e) => serde_json::json!({"ok": false, "error": format!("{e:#}")}),
                },
                Err(e) => serde_json::json!({"ok": false, "error": format!("bad body: {e}")}),
            }
        }

        (_, "/refresh") => {
            let _ = refresh_tx.send(());
            serde_json::json!({"ok": true})
        }

        _ => return json(404, serde_json::json!({"ok": false, "error": "not found"})),
    };
    json(200, result)
}

impl HttpServer {
    /// 通知 HTTP 线程停止（主循环退出时调用）。
    pub fn stop(&self) {
        if let Some(tx) = self.shutdown.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}
