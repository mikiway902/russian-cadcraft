#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use cadcraft_model_worker::{Error, MAX_MESSAGE_BYTES, MAX_REQUESTS, MAX_RESPONSE_BYTES, Worker};
use craft_jsonrpc::{Limits, RpcError};
use serde_json::{Value, json};
use std::{
    io::{self, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !std::env::args_os().any(|arg| arg == "--internal-session") {
        return supervise().map_err(Into::into);
    }
    // Match upstream CLI's larger kernel stack, not the platform's small default stack.
    let result = std::thread::Builder::new().name("cadcraft-model-worker".into()).stack_size(solidcraft_kernel::KERNEL_STACK).spawn(run)?.join();
    match result {
        Ok(result) => result.map_err(Into::into),
        Err(_) => Err("worker thread failed".into()),
    }
}

fn run() -> Result<(), Error> {
    let mut roots = Vec::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--internal-session" {
            continue;
        }
        if arg != "--allow-path" {
            return Err(Error::Parameters("usage: cadcraft-model-worker --allow-path DIRECTORY [--allow-path DIRECTORY]".into()));
        }
        let root = args.next().ok_or_else(|| Error::Parameters("--allow-path needs a directory".into()))?;
        roots.push(PathBuf::from(root));
        if roots.len() > 8 {
            return Err(Error::Limit("allowed directory count"));
        }
    }
    let mut worker = Worker::new(roots)?;
    let limits = Limits { max_message_bytes: MAX_MESSAGE_BYTES, max_batch_items: 1, integer_ids_only: true, allow_null_ids: true };
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    for _ in 0..MAX_REQUESTS {
        let bytes = match craft_jsonrpc::read_line(&mut input, MAX_MESSAGE_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(()),
            Err(error) => {
                send(&mut output, &craft_jsonrpc::error_response(Value::Null, RpcError::new(-32003, error.to_string())))?;
                // An oversized line is not drained: bounded work, then close the session.
                return Ok(());
            }
        };
        let parsed = match craft_jsonrpc::parse_json(&bytes, limits) {
            Ok(parsed) => parsed,
            Err(_) => {
                send(&mut output, &craft_jsonrpc::parse_error_response())?;
                continue;
            }
        };
        let reply = craft_jsonrpc::process(
            &parsed,
            limits,
            |request| worker.call(request.method, request.params.cloned().unwrap_or_else(|| json!({}))).map_err(Error::rpc),
            // Only requests have effects. Notifications are deliberately not an alternate API.
            |_| {},
        )
        .unwrap_or_else(|e| Some(craft_jsonrpc::error_response(Value::Null, RpcError::new(-32600, e.to_string()))));
        if let Some(reply) = reply {
            send(&mut output, &reply)?;
        }
    }
    send(&mut output, &craft_jsonrpc::error_response(Value::Null, RpcError::new(-32003, "session frame limit reached; start a new worker")))?;
    Ok(())
}

// A separate process is necessary: cancellation cannot interrupt every numerical kernel call.
// A timed-out or failed child is killed and the session closes, never silently restarted.
struct Session(Child);
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn supervise() -> Result<(), Error> {
    let mut child = Session(
        Command::new(std::env::current_exe()?)
            .args(std::env::args_os().skip(1))
            .arg("--internal-session")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let mut child_input = child.0.stdin.take().ok_or(Error::Internal)?;
    let child_output = child.0.stdout.take().ok_or(Error::Internal)?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = std::thread::Builder::new().name("model-worker-response".into()).spawn(move || {
        let mut output = BufReader::new(child_output);
        loop {
            let line = craft_jsonrpc::read_line(&mut output, MAX_RESPONSE_BYTES);
            let done = !matches!(&line, Ok(Some(_)));
            if sender.send(line).is_err() || done {
                break;
            }
        }
    })?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let limits = Limits { max_message_bytes: MAX_MESSAGE_BYTES, max_batch_items: 1, integer_ids_only: true, allow_null_ids: true };
    for _ in 0..MAX_REQUESTS {
        let bytes = match craft_jsonrpc::read_line(&mut input, MAX_MESSAGE_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(()),
            Err(e) => {
                send(&mut output, &craft_jsonrpc::error_response(Value::Null, RpcError::new(-32003, e.to_string())))?;
                return Ok(());
            }
        };
        let parsed = match craft_jsonrpc::parse_json(&bytes, limits) {
            Ok(value) => value,
            Err(_) => {
                send(&mut output, &craft_jsonrpc::parse_error_response())?;
                continue;
            }
        };
        // Reuse the exact envelope/notification semantics when deciding whether a reply is owed.
        match craft_jsonrpc::process(&parsed, limits, |_| Ok(Value::Null), |_| {}) {
            Ok(None) => continue,
            Err(e) => {
                send(&mut output, &craft_jsonrpc::error_response(Value::Null, RpcError::new(-32600, e.to_string())))?;
                continue;
            }
            Ok(Some(_)) => {}
        }
        child_input.write_all(&bytes)?;
        child_input.write_all(b"\n")?;
        child_input.flush()?;
        match receiver.recv_timeout(Duration::from_secs(40)) {
            Ok(Ok(Some(reply))) => {
                output.write_all(&reply)?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
            result => {
                let timed_out = matches!(result, Err(mpsc::RecvTimeoutError::Timeout));
                let mut error = RpcError::new(
                    if timed_out { -32008 } else { -32603 },
                    if timed_out {
                        "geometry worker exceeded hard 40 second deadline"
                    } else {
                        "geometry worker terminated without a valid bounded reply"
                    },
                );
                error.data = Some(
                    json!({"kind": if timed_out { "hard_deadline" } else { "worker_failure" }, "model_discarded": true, "session_closed": true}),
                );
                send(&mut output, &craft_jsonrpc::error_response(parsed.get("id").cloned().unwrap_or(Value::Null), error))?;
                // Drop closes the pipes and terminates outstanding work. The response reader
                // then sees EOF; detaching its JoinHandle cannot keep the process alive.
                drop(child);
                drop(reader);
                return Ok(());
            }
        }
    }
    send(&mut output, &craft_jsonrpc::error_response(Value::Null, RpcError::new(-32003, "session frame limit reached; start a new worker")))?;
    Ok(())
}

struct Frame(Vec<u8>);
impl Write for Frame {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_RESPONSE_BYTES {
            return Err(io::Error::other("response exceeds frame byte limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn send(output: &mut impl Write, response: &Value) -> Result<(), Error> {
    let mut frame = Frame(Vec::new());
    if serde_json::to_writer(&mut frame, response).is_err() {
        frame.0.clear();
        serde_json::to_writer(
            &mut frame,
            &craft_jsonrpc::error_response(response.get("id").cloned().unwrap_or(Value::Null), Error::Limit("response frame bytes").rpc()),
        )?;
    }
    output.write_all(&frame.0)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
