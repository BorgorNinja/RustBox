use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// Minimal QMP client: connect, negotiate, run one command, return its result.
pub fn execute(sock: &Path, cmd: &str, args: Option<Value>) -> Result<Value> {
    let stream = UnixStream::connect(sock).with_context(|| format!("connect {}", sock.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut w = stream;
    let mut line = String::new();
    reader.read_line(&mut line)?; // greeting
    send(&mut w, &json!({"execute": "qmp_capabilities"}))?;
    wait_reply(&mut reader)?;
    let mut req = json!({"execute": cmd});
    if let Some(a) = args {
        req["arguments"] = a;
    }
    send(&mut w, &req)?;
    wait_reply(&mut reader)
}

fn send(w: &mut UnixStream, v: &Value) -> Result<()> {
    w.write_all(v.to_string().as_bytes())?;
    w.write_all(b"\r\n")?;
    Ok(())
}

fn wait_reply(r: &mut BufReader<UnixStream>) -> Result<Value> {
    loop {
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            // QEMU closes the socket right after "quit"; treat as success.
            return Ok(Value::Null);
        }
        let v: Value = serde_json::from_str(line.trim())?;
        if let Some(ret) = v.get("return") {
            return Ok(ret.clone());
        }
        if let Some(err) = v.get("error") {
            bail!("QMP error: {err}");
        }
        // otherwise: an async event, keep reading
    }
}
