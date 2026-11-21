//! Native, bounded JSON-RPC transport for the separately packaged exact model worker.
//! The host chooses an executable and an explicit local root; there is no in-process fallback.

use serde::Serialize;
use serde_json::Value;
use std::{
    io::{BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    thread::JoinHandle,
    time::Duration,
};

const REQUEST_BYTES: usize = 16 * 1024;
const RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_CALLS: u64 = 256;
const DEADLINE: Duration = Duration::from_secs(45);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("model worker transport: {0}")]
    Transport(String),
    #[error("model worker request: {0}")]
    Request(String),
    #[error("model worker RPC {code}: {message}")]
    Remote { code: i64, message: String, data: Option<Value> },
}

type Frame = std::result::Result<Vec<u8>, String>;

/// A single explicitly loaded model session. Transport failure closes it permanently.
/// Calls are serial; hosts may keep this object in their background model-job owner.
pub struct Client {
    process: Child,
    requests: Option<SyncSender<Vec<u8>>>,
    answers: Option<Receiver<Frame>>,
    writer: Option<JoinHandle<()>>,
    reader: Option<JoinHandle<()>>,
    next_id: u64,
}

#[derive(Serialize)]
struct Request<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: &'a Value,
}

impl Client {
    pub fn spawn(program: &Path, allowed_root: &Path) -> std::result::Result<Self, Error> {
        let allowed_root = allowed_root.canonicalize().map_err(|error| Error::Request(error.to_string()))?;
        if !allowed_root.is_dir() {
            return Err(Error::Request("allowed root must be a directory".into()));
        }
        let mut command = Command::new(program);
        command.arg("--allow-path").arg(allowed_root).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW, as with the DWG worker.
        }
        let process = command.spawn().map_err(|error| Error::Transport(format!("cannot start {}: {error}", program.display())))?;
        let mut client = Self { process, requests: None, answers: None, writer: None, reader: None, next_id: 1 };
        let mut input = client.process.stdin.take().ok_or_else(|| Error::Transport("missing input pipe".into()))?;
        let output = client.process.stdout.take().ok_or_else(|| Error::Transport("missing output pipe".into()))?;
        let (send_answer, answers) = mpsc::sync_channel(1);
        client.answers = Some(answers);
        let reader_answer = send_answer.clone();
        client.reader = Some(
            std::thread::Builder::new()
                .name("model-worker-read".into())
                .spawn(move || {
                    let mut output = BufReader::new(output);
                    loop {
                        let frame = craft_jsonrpc::read_line(&mut output, RESPONSE_BYTES)
                            .map_err(|error| error.to_string())
                            .and_then(|frame| frame.ok_or_else(|| "worker closed its output".into()));
                        let ended = frame.is_err();
                        if reader_answer.send(frame).is_err() || ended {
                            break;
                        }
                    }
                })
                .map_err(|error| Error::Transport(error.to_string()))?,
        );
        let (requests, receive_request) = mpsc::sync_channel::<Vec<u8>>(1);
        client.requests = Some(requests);
        client.writer = Some(
            std::thread::Builder::new()
                .name("model-worker-write".into())
                .spawn(move || {
                    while let Ok(frame) = receive_request.recv() {
                        if let Err(error) = input.write_all(&frame).and_then(|()| input.flush()) {
                            let _ = send_answer.send(Err(error.to_string()));
                            break;
                        }
                    }
                })
                .map_err(|error| Error::Transport(error.to_string()))?,
        );
        Ok(client)
    }

    pub fn call(&mut self, method: &str, params: &Value) -> std::result::Result<Value, Error> {
        if self.next_id > MAX_CALLS {
            return Err(Error::Request("session request limit reached; explicitly open a new session".into()));
        }
        let id = self.next_id;
        let mut bytes = serde_json::to_vec(&Request { jsonrpc: "2.0", id, method, params }).map_err(|error| Error::Request(error.to_string()))?;
        if bytes.len() > REQUEST_BYTES {
            return Err(Error::Request("request exceeds 16 KiB".into()));
        }
        bytes.push(b'\n');
        let result = (|| {
            let requests = self.requests.as_ref().ok_or_else(|| Error::Transport("session is closed".into()))?;
            requests.try_send(bytes).map_err(|error| Error::Transport(error.to_string()))?;
            self.next_id += 1;
            let answer = self
                .answers
                .as_ref()
                .ok_or_else(|| Error::Transport("session is closed".into()))?
                .recv_timeout(DEADLINE)
                .map_err(|error| Error::Transport(error.to_string()))?
                .map_err(Error::Transport)?;
            reply(&answer, id)
        })();
        if matches!(result, Err(Error::Transport(_))) {
            self.close();
        }
        result
    }

    fn close(&mut self) {
        // Release bounded channels before joining: an unsolicited response may have filled one.
        self.requests.take();
        self.answers.take();
        let _ = self.process.kill();
        let _ = self.process.wait();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.close();
    }
}

fn reply(bytes: &[u8], id: u64) -> std::result::Result<Value, Error> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|error| Error::Transport(error.to_string()))?;
    let object = value.as_object_mut().ok_or_else(|| Error::Transport("reply must be an object".into()))?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || object.get("id").and_then(Value::as_u64) != Some(id)
        || object.contains_key("result") == object.contains_key("error")
    {
        return Err(Error::Transport("invalid response envelope or request identity".into()));
    }
    if let Some(result) = object.remove("result") {
        return Ok(result);
    }
    let error = object.get_mut("error").and_then(Value::as_object_mut).ok_or_else(|| Error::Transport("invalid RPC error".into()))?;
    let code = error.get("code").and_then(Value::as_i64).ok_or_else(|| Error::Transport("invalid RPC error code".into()))?;
    let Some(Value::String(message)) = error.remove("message") else {
        return Err(Error::Transport("invalid RPC error message".into()));
    };
    Err(Error::Remote { code, message, data: error.remove("data") })
}

#[cfg(test)]
mod tests {
    use super::{Error, reply};

    #[test]
    fn successful_payload_is_not_confused_with_a_different_request_or_remote_failure() {
        assert_eq!(reply(br#"{"jsonrpc":"2.0","id":7,"result":{"volume":125}}"#, 7).unwrap()["volume"], 125);
        assert!(matches!(reply(br#"{"jsonrpc":"2.0","id":8,"result":{"volume":125}}"#, 7), Err(Error::Transport(_))));
        let failure = reply(br#"{"jsonrpc":"2.0","id":7,"error":{"code":-32007,"message":"changed source","data":{"kind":"stale_model"}}}"#, 7);
        let Err(Error::Remote { code, data: Some(data), .. }) = failure else { panic!("remote error must not become a successful model") };
        assert_eq!(code, -32007);
        assert_eq!(data["kind"], "stale_model");
        for invalid in [
            br#"{"jsonrpc":"2.0","id":7}"#.as_slice(),
            br#"{"jsonrpc":"2.0","id":7,"result":{},"error":{}}"#,
            br#"{"jsonrpc":"2.0","id":7,"error":{"code":1,"message":7}}"#,
        ] {
            assert!(matches!(reply(invalid, 7), Err(Error::Transport(_))));
        }
    }
}
