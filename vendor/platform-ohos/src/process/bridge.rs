//! OHOS 外部进程桥。
//!
//! 鸿蒙应用沙箱没有「按路径 exec 任意程序」的公开 API：官方 childProcessManager 只能启动
//! 应用自身声明的 Ark 入口，native child process 只能加载自带 .so。因此 CLI agent 的执行
//! 代理到 PC 上的 `ca-exec-bridge`：本模块把 [`Process`] 的请求序列化后经 TCP 发出，
//! 再把对端回传的 stdout / stderr 行喂给原有的回调与输出上限逻辑。
//!
//! 协议：4 字节大端长度 + UTF-8 JSON 帧。stdin 用 hex 编码（无需新增依赖）。

use super::{Capped, LineSink, Output, Process, StdinMode};
use crate::sync::Mutex;
use craft_anything_evidence::{ErrorCode, ToolError};
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

/// 桥地址，可由适配层在启动时设置（ArkTS 传入或默认值）。
static ADDR: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// 设置桥地址，例如 `127.0.0.1:17890`（配合 `hdc fport`）。只生效一次。
pub fn set_bridge_addr(addr: impl Into<String>) {
    let _ = ADDR.set(addr.into());
}

fn bridge_addr() -> String {
    ADDR.get()
        .cloned()
        .or_else(|| std::env::var("CRAFT_ANYTHING_EXEC_BRIDGE").ok())
        .unwrap_or_else(|| "127.0.0.1:17890".to_string())
}

const MAX_FRAME: usize = 64 * 1024 * 1024;

fn write_frame(stream: &mut TcpStream, value: &serde_json::Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    let len = u32::try_from(bytes.len()).map_err(|_| std::io::Error::other("bridge frame too large"))?;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()
}

fn read_frame(stream: &mut TcpStream) -> std::io::Result<serde_json::Value> {
    let mut head = [0u8; 4];
    stream.read_exact(&mut head)?;
    let len = u32::from_be_bytes(head) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::other("bridge frame exceeds limit"));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(std::io::Error::other)
}

fn env_pairs(p: &Process) -> serde_json::Value {
    serde_json::Value::Array(
        p.env
            .iter()
            .map(|(k, v)| serde_json::json!([k.to_string_lossy(), v.as_ref().map(|x| x.to_string_lossy().to_string())]))
            .collect(),
    )
}

/// 在 OHOS 上代替 [`Process::spawn`]：连接桥、发 spawn、在后台监督并返回 [`super::RunningProcess`]。
pub(super) fn spawn_bridged(p: Process) -> Result<super::RunningProcess, ToolError> {
    let name = p.display();
    let addr = bridge_addr();
    let mut stream = TcpStream::connect(&addr).map_err(|e| {
        ToolError::new(
            ErrorCode::ProgramFailed,
            format!("Cannot reach the exec bridge at {addr}: {e}"),
        )
        .with_remediation("Start ca-exec-bridge on the PC and forward the port with: hdc fport tcp:17890 tcp:17890")
    })?;
    let _ = stream.set_nodelay(true);

    let (stdin_mode, stdin_data) = match &p.stdin {
        StdinMode::Closed => ("closed", Vec::new()),
        StdinMode::Bytes(b) => ("bytes", b.clone()),
        StdinMode::Piped => ("piped", Vec::new()),
    };
    let req = serde_json::json!({
        "t": "spawn",
        "program": p.program.to_string_lossy(),
        "args": p.args.iter().map(|a| a.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "env": env_pairs(&p),
        "env_clear": p.env_clear,
        "cwd": p.cwd.as_ref().map(|d| d.to_string_lossy().to_string()),
        "stdin": stdin_mode,
        "stdin_data": hex::encode(stdin_data),
        "label": p.label,
    });
    write_frame(&mut stream, &req).map_err(|e| {
        ToolError::new(ErrorCode::ProgramFailed, format!("Could not send the run request for {name} to {addr}: {e}"))
    })?;

    // 一个线程只管读帧；监督线程用 channel 收事件，这样 read timeout 不会把半帧截断。
    let (event_tx, event_rx): (Sender<serde_json::Value>, Receiver<serde_json::Value>) = mpsc::channel();
    let mut reader = stream.try_clone().map_err(|e| ToolError::new(ErrorCode::ProgramFailed, format!("bridge socket: {e}")))?;
    let reader_name = name.clone();
    std::thread::Builder::new()
        .name("bridge-reader".into())
        .spawn(move || loop {
            match read_frame(&mut reader) {
                Ok(v) => {
                    let end = v.get("t").and_then(|t| t.as_str()) == Some("exit")
                        || v.get("t").and_then(|t| t.as_str()) == Some("error");
                    if event_tx.send(v).is_err() || end {
                        return;
                    }
                }
                Err(e) if e.kind() == ErrorKind::TimedOut || e.kind() == ErrorKind::WouldBlock => continue,
                Err(_) => return,
            }
        })
        .map_err(|e| ToolError::new(ErrorCode::ProgramFailed, format!("Could not start bridge reader for {reader_name}: {e}")))?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));

    let kill = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let sup = BridgeSupervisor {
        name: name.clone(),
        start: Instant::now(),
        wall: p.wall,
        idle: p.idle,
        cancel: p.cancel.clone(),
        kill: kill.clone(),
        on_stdout: p.on_stdout.clone(),
        on_stderr: p.on_stderr.clone(),
        out_buf: Arc::new(Mutex::new(Capped::new(p.stdout_cap))),
        err_buf: Arc::new(Mutex::new(Capped::new(p.stderr_cap))),
        activity: Arc::new(AtomicU64::new(0)),
        event_rx,
        socket: stream,
        pid: 0,
    };
    std::thread::Builder::new()
        .name(format!("bridge-{name}"))
        .spawn(move || {
            let result = sup.run();
            let _ = tx.send(result);
        })
        .map_err(|e| ToolError::new(ErrorCode::ProgramFailed, format!("Could not start bridge supervisor for {name}: {e}")))?;

    Ok(super::RunningProcess {
        pid: 0,
        name,
        stdin: None, // ACP 需要可写 ChildStdin，OHOS 上不可用；设置里关闭 ACP 走一次性 CLI。
        kill,
        rx,
        result: None,
        hands_off: false,
    })
}

struct BridgeSupervisor {
    name: String,
    start: Instant,
    wall: Duration,
    idle: Option<Duration>,
    cancel: Option<super::Cancel>,
    kill: Arc<AtomicBool>,
    on_stdout: Option<super::LineSink>,
    on_stderr: Option<super::LineSink>,
    out_buf: Arc<Mutex<Capped>>,
    err_buf: Arc<Mutex<Capped>>,
    activity: Arc<AtomicU64>,
    event_rx: Receiver<serde_json::Value>,
    socket: TcpStream,
    pid: u32,
}

enum Stop {
    Exited(Option<i32>),
    Wall,
    Idle,
    Cancelled,
    Lost(String),
}

impl BridgeSupervisor {
    fn run(mut self) -> Result<Output, ToolError> {
        let stop = self.watch();
        let duration = self.start.elapsed();
        let (stdout, stdout_tail, stdout_truncated) = super::take(&self.out_buf);
        let (stderr, stderr_tail, stderr_truncated) = super::take(&self.err_buf);
        let partial = || serde_json::json!({
            "stdout_tail": String::from_utf8_lossy(&stdout_tail),
            "stderr_tail": String::from_utf8_lossy(&stderr_tail),
        });
        match stop {
            Stop::Exited(code) => Ok(Output {
                success: code == Some(0),
                code,
                stdout,
                stderr,
                stdout_tail,
                stderr_tail,
                stdout_truncated,
                stderr_truncated,
                duration,
            }),
            Stop::Wall => Err(ToolError::new(ErrorCode::TimedOut, format!("{} took longer than {} s and was stopped.", self.name, self.wall.as_secs()))
                .with_remediation("Try a smaller job, or raise the time limit in Settings.")
                .with_details(serde_json::json!({"deadline": "wall", "partial": partial()}))),
            Stop::Idle => Err(ToolError::new(ErrorCode::TimedOut, format!("{} printed nothing for {} s and was stopped.", self.name, self.idle.map(|d| d.as_secs()).unwrap_or_default()))
                .with_remediation("It may be waiting for input or stuck; run it by hand to check.")
                .with_details(serde_json::json!({"deadline": "idle", "partial": partial()}))),
            Stop::Cancelled => Err(ToolError::cancelled().with_details(serde_json::json!({"partial": partial()}))),
            Stop::Lost(msg) => Err(ToolError::new(ErrorCode::ProgramFailed, format!("Lost the bridge process for {}: {msg}", self.name)).with_details(serde_json::json!({"partial": partial()}))),
        }
    }

    fn watch(&mut self) -> Stop {
        let mut tick = Duration::from_millis(2);
        let mut done = None;
        loop {
            if done.is_none() {
                match self.event_rx.try_recv() {
                    Ok(event) => {
                        if let Some(v) = self.handle(event) {
                            done = Some(v);
                        }
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) if self.start.elapsed() < Duration::from_millis(500) => {}
                    Err(TryRecvError::Disconnected) => return Stop::Lost("event channel closed".into()),
                }
            }
            if done.is_none() && (self.kill.load(Ordering::SeqCst) || self.cancel.as_ref().is_some_and(|c| c())) {
                let _ = write_frame(&mut self.socket, &serde_json::json!({"t": "kill"}));
                // 给对端一点时间回 exit；没有就按取消返回。
                let until = Instant::now() + Duration::from_millis(500);
                while Instant::now() < until {
                    match self.event_rx.recv_timeout(Duration::from_millis(20)) {
                        Ok(event) => {
                            if let Some(v) = self.handle(event) {
                                return v;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                return Stop::Cancelled;
            }
            if let Some(done) = done {
                return done;
            }
            let elapsed = self.start.elapsed();
            if elapsed >= self.wall {
                let _ = write_frame(&mut self.socket, &serde_json::json!({"t": "kill"}));
                return Stop::Wall;
            }
            if let Some(idle) = self.idle {
                let last = Duration::from_millis(self.activity.load(Ordering::Relaxed));
                if elapsed.saturating_sub(last) >= idle {
                    let _ = write_frame(&mut self.socket, &serde_json::json!({"t": "kill"}));
                    return Stop::Idle;
                }
            }
            std::thread::sleep(tick);
            tick = (tick * 2).min(Duration::from_millis(25));
        }
    }

    /// 返回 Some 表示该进程已终结。
    fn handle(&mut self, event: serde_json::Value) -> Option<Stop> {
        let kind = event.get("t").and_then(|v| v.as_str()).unwrap_or_default();
        match kind {
            "started" => {
                self.pid = event.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                None
            }
            "stdout" | "stderr" => {
                let line = event.get("line").and_then(|v| v.as_str()).unwrap_or_default();
                self.activity.store(super::millis_since(self.start), Ordering::Relaxed);
                let (buf, sink): (&Arc<Mutex<Capped>>, &Option<LineSink>) = if kind == "stdout" {
                    (&self.out_buf, &self.on_stdout)
                } else {
                    (&self.err_buf, &self.on_stderr)
                };
                let mut bytes = line.as_bytes().to_vec();
                bytes.push(b'\n');
                buf.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(&bytes);
                if let Some(sink) = sink {
                    sink(line);
                }
                None
            }
            "exit" => Some(Stop::Exited(event.get("code").and_then(|v| v.as_i64()).map(|c| c as i32))),
            "error" => Some(Stop::Lost(event.get("message").and_then(|v| v.as_str()).unwrap_or("unknown bridge error").to_string())),
            _ => None,
        }
    }
}
