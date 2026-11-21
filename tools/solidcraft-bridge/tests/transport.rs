use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

#[test]
fn actual_worker_obeys_jsonrpc_framing_and_notifications_have_no_effects() {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-model-worker"))
        .arg("--allow-path")
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    writeln!(input, "not JSON").unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","method":"v1.load","params":{"path":"/notification-must-not-load.step"}})).unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":1,"method":"v1.capabilities"})).unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":2,"method":"v1.source_status"})).unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":3,"method":"v99.load","params":{}})).unwrap();
    writeln!(
        input,
        "{}",
        json!([{"jsonrpc":"2.0","id":4,"method":"v1.capabilities"},
        {"jsonrpc":"2.0","id":5,"method":"v1.capabilities"}])
    )
    .unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":"model-status","method":"v1.source_status","params":{}})).unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":1.25,"method":"v1.load","params":{"path":"/fractional-must-not-load.sc"}})).unwrap();
    drop(input);
    let responses: Vec<Value> = BufReader::new(output).lines().map(|line| serde_json::from_str(&line.unwrap()).unwrap()).collect();
    assert_eq!(responses.len(), 7, "notifications must not emit a reply");
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["id"], 1);
    assert_eq!(responses[1]["result"]["protocol"], "cadcraft-model-worker/1");
    assert_eq!(responses[2]["error"]["data"]["kind"], "no_model");
    assert_eq!(responses[3]["error"]["code"], -32601);
    assert_eq!(responses[4]["error"]["code"], -32600);
    assert_eq!(responses[5]["id"], "model-status");
    assert_eq!(responses[5]["error"]["data"]["kind"], "no_model");
    assert_eq!(responses[6]["error"]["code"], -32600);
    assert!(child.wait().unwrap().success());
}

#[test]
fn oversized_line_closes_the_real_worker_with_bounded_error_frame() {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-model-worker"))
        .arg("--allow-path")
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    input.write_all(&vec![b'x'; cadcraft_model_worker::MAX_MESSAGE_BYTES + 1]).unwrap();
    drop(input);
    let responses: Vec<Value> = BufReader::new(output).lines().map(|line| serde_json::from_str(&line.unwrap()).unwrap()).collect();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["code"], -32003);
    assert!(child.wait().unwrap().success());
}
