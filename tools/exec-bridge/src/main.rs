//! ca-exec-bridge —— 鸿蒙版 Craft Anything 的外部进程执行后端。
//!
//! 鸿蒙应用沙箱无法 exec 任意 CLI agent，App 把 spawn 请求发到这里，本程序用
//! std::process::Command 真正运行 claude / codex / gemini 等，并流式回传输出。
//!
//! 协议：4 字节大端长度 + JSON 帧。请求 spawn / stdin / kill，响应
//! started / stdout / stderr / exit / error。stdin 数据用 hex 编码。
//!
//! 安全：默认只监听回环地址，配合 `hdc fport tcp:17890 tcp:17890` 使用；
//! 如需局域网直连，显式设置 CA_EXEC_BRIDGE_ADDR=0.0.0.0:17890 并自行保证网络隔离。

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};

const MAX_FRAME: usize = 64 * 1024 * 1024;

fn write_frame(stream: &mut TcpStream, value: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    let len = u32::try_from(bytes.len()).map_err(|_| std::io::Error::other("frame too large"))?;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(&bytes)?;
    stream.flush()
}

fn read_frame(stream: &mut TcpStream) -> std::io::Result<Value> {
    let mut head = [0u8; 4];
    stream.read_exact(&mut head)?;
    let len = u32::from_be_bytes(head) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::other("frame exceeds limit"));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(std::io::Error::other)
}

fn main() {
    let addr = std::env::var("CA_EXEC_BRIDGE_ADDR").unwrap_or_else(|_| "127.0.0.1:17890".to_string());
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    println!("ca-exec-bridge listening on {addr}");
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                std::thread::Builder::new()
                    .name("bridge-conn".into())
                    .spawn(move || handle(stream))
                    .ok();
            }
            Err(e) => eprintln!("accept failed: {e}"),
        }
    }
}

fn handle(mut stream: TcpStream) {
    let request = match read_frame(&mut stream) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("bad request frame: {e}");
            return;
        }
    };
    if request.get("t").and_then(|t| t.as_str()) != Some("spawn") {
        let _ = write_frame(&mut stream, &json!({"t":"error","message":"expected spawn"}));
        return;
    }
    let program = request.get("program").and_then(|v| v.as_str()).unwrap_or_default();
    let args: Vec<String> = serde_json::from_value(request.get("args").cloned().unwrap_or_default()).unwrap_or_default();
    let label = request.get("label").and_then(|v| v.as_str()).unwrap_or(program);

    let mut command = Command::new(program);
    command.args(&args);
    if request.get("env_clear").and_then(|v| v.as_bool()).unwrap_or(false) {
        command.env_clear();
    }
    if let Some(pairs) = request.get("env").and_then(|v| v.as_array()) {
        for pair in pairs {
            let key = pair.get(0).and_then(|v| v.as_str());
            if let Some(key) = key {
                match pair.get(1).and_then(|v| v.as_str()) {
                    Some(value) => {
                        command.env(key, value);
                    }
                    None => {
                        command.env_remove(key);
                    }
                }
            }
        }
    }
    if let Some(cwd) = request.get("cwd").and_then(|v| v.as_str()) {
        command.current_dir(cwd);
    }
    let stdin_mode = request.get("stdin").and_then(|v| v.as_str()).unwrap_or("closed");
    let stdin_bytes = hex::decode(request.get("stdin_data").and_then(|v| v.as_str()).unwrap_or("")).unwrap_or_default();
    command.stdin(if stdin_mode == "closed" { Stdio::null() } else { Stdio::piped() });
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = write_frame(
                &mut stream,
                &json!({"t":"error","message":format!("Cannot start {program}: {e}. Install it or set its path in Settings → AI.")}),
            );
            return;
        }
    };
    let pid = child.id();
    let child = Arc::new(Mutex::new(child));
    let mut stdin: Option<ChildStdin> = child.lock().ok().and_then(|mut c| c.stdin.take());
    let stdout = child.lock().ok().and_then(|mut c| c.stdout.take());
    let stderr = child.lock().ok().and_then(|mut c| c.stderr.take());

    let (tx, rx) = mpsc::channel::<Value>();
    if let Some(out) = stdout {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                match line {
                    Ok(line) => {
                        if tx.send(json!({"t":"stdout","line":line})).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }
    if let Some(err) = stderr {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines() {
                match line {
                    Ok(line) => {
                        if tx.send(json!({"t":"stderr","line":line})).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    // 一次性 stdin：写完即关闭，让等待输入的 CLI agent 看到 EOF。
    if stdin_mode == "bytes" {
        if let Some(pipe) = stdin.as_mut() {
            let _ = pipe.write_all(&stdin_bytes);
            let _ = pipe.flush();
        }
        stdin = None;
    }
    let shared_stdin = Arc::new(Mutex::new(stdin));

    // 读 socket 的后续控制帧：ACP 的双向 stdin、取消、强制结束。
    let mut control = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let control_child = child.clone();
    let control_stdin = shared_stdin.clone();
    std::thread::spawn(move || loop {
        match read_frame(&mut control) {
            Ok(frame) => match frame.get("t").and_then(|v| v.as_str()) {
                Some("stdin") => {
                    let data = frame.get("data").and_then(|v| v.as_str()).unwrap_or("");
                    if !data.is_empty() {
                        if let Ok(bytes) = hex::decode(data) {
                            if let Ok(mut guard) = control_stdin.lock() {
                                if let Some(pipe) = guard.as_mut() {
                                    if pipe.write_all(&bytes).is_err() || pipe.flush().is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    if frame.get("eof").and_then(|v| v.as_bool()).unwrap_or(false) {
                        if let Ok(mut guard) = control_stdin.lock() {
                            guard.take();
                        }
                    }
                }
                Some("kill") => {
                    if let Ok(mut c) = control_child.lock() {
                        let _ = c.kill();
                    }
                    break;
                }
                _ => {}
            },
            Err(_) => break, // 对端断开：让进程自然结束，或等 wait 线程返回。
        }
    });

    let wait_child = child.clone();
    let wait_tx = tx.clone();
    std::thread::spawn(move || {
        let code = wait_child.lock().ok().and_then(|mut c| c.wait().ok()).and_then(|s| s.code());
        let _ = wait_tx.send(json!({"t":"exit","code":code}));
    });

    let _ = write_frame(&mut stream, &json!({"t":"started","pid":pid,"label":label}));
    while let Ok(event) = rx.recv() {
        let terminal = event.get("t").and_then(|v| v.as_str()) == Some("exit");
        if write_frame(&mut stream, &event).is_err() {
            break;
        }
        if terminal {
            break;
        }
    }
    println!("bridge run finished: {label}");
}
