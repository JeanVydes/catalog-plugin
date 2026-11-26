//! Reads account quota through the installed Codex app server.

use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Usage {
    pub account_id: Option<String>,
    pub rate_limits: Limits,
}

#[derive(Deserialize)]
pub(super) struct Limits {
    pub primary: Option<Window>,
    pub secondary: Option<Window>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Window {
    pub used_percent: i32,
    #[serde(rename = "windowDurationMins")]
    pub duration_minutes: Option<i64>,
    pub resets_at: Option<i64>,
}

#[derive(Deserialize)]
struct Reply {
    id: u64,
    result: Option<Value>,
}

#[derive(Serialize)]
struct Request<T> {
    method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<T>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Initialize {
    client_info: ClientInfo,
}

#[derive(Serialize)]
struct ClientInfo {
    name: &'static str,
    title: &'static str,
    version: &'static str,
}

pub(super) fn read() -> Option<Usage> {
    let mut child = Command::new("codex")
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let result = child
        .stdin
        .take()
        .zip(child.stdout.take())
        .and_then(|(mut input, output)| {
            let (sender, receiver) = mpsc::channel();
            std::thread::Builder::new()
                .name("codex-usage-rpc".to_owned())
                .spawn(move || {
                    for line in BufReader::new(output).lines().map_while(Result::ok) {
                        if let Ok(reply) = serde_json::from_str::<Reply>(&line)
                            && sender.send(reply).is_err()
                        {
                            return;
                        }
                    }
                })
                .ok()?;
            query(&mut input, &receiver)
        });
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn query(input: &mut ChildStdin, replies: &Receiver<Reply>) -> Option<Usage> {
    let deadline = Instant::now() + TIMEOUT;
    send(
        input,
        &Request {
            method: "initialize",
            id: Some(1),
            params: Some(Initialize {
                client_info: ClientInfo {
                    name: "norte_code",
                    title: "Norte Code",
                    version: env!("CARGO_PKG_VERSION"),
                },
            }),
        },
    )?;
    receive(replies, 1, deadline)?;
    send(
        input,
        &Request::<()> {
            method: "initialized",
            id: None,
            params: None,
        },
    )?;
    send(
        input,
        &Request::<()> {
            method: "account/rateLimits/read",
            id: Some(2),
            params: None,
        },
    )?;
    serde_json::from_value(receive(replies, 2, deadline)?).ok()
}

fn send(input: &mut ChildStdin, request: &Request<impl Serialize>) -> Option<()> {
    serde_json::to_writer(&mut *input, request).ok()?;
    input.write_all(b"\n").ok()
}

fn receive(replies: &Receiver<Reply>, id: u64, deadline: Instant) -> Option<Value> {
    loop {
        let reply = replies
            .recv_timeout(deadline.checked_duration_since(Instant::now())?)
            .ok()?;
        if reply.id == id {
            return reply.result;
        }
    }
}
