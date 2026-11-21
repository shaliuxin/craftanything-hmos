//! Minimal stand-in for `claude -p`, compiled to claude.exe for bridge verification.
//! Speaks the subset of Claude Code's JSON-lines protocol parsed by craft-anything's
//! ClaudeCode adapter. Not a real agent.

use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("claude 2.0.0 (mock bridge agent)");
        return;
    }
    let mut prompt = String::new();
    let _ = std::io::stdin().read_to_string(&mut prompt);
    let _ = prompt;

    println!(r#"{{"type":"system","subtype":"init","session_id":"mock-bridge-session","mcp_servers":[]}}"#);
    println!(r#"{{"type":"assistant","session_id":"mock-bridge-session","message":{{"content":[{{"type":"text","text":"外部进程桥已连通：PC 上的模拟 agent 成功执行。"}}]}}}}"#);
    println!(r#"{{"type":"result","subtype":"success","session_id":"mock-bridge-session","result":"mock agent finished","is_error":false,"total_cost_usd":0.0,"usage":{{"input_tokens":1,"output_tokens":1}}}}"#);
}
