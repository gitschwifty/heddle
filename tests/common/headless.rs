//! Headless subprocess driver — spawn the binary, send JSONL, collect stdout.

#![allow(dead_code)]

use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

pub struct Headless {
    proc: Child,
    lines: Arc<Mutex<Vec<String>>>,
    stdin: std::process::ChildStdin,
    _tempdir: TempDir,
}

impl Headless {
    pub fn spawn(extra_env: HashMap<String, String>) -> Self {
        let bin = env!("CARGO_BIN_EXE_heddle-headless");
        let td = tempfile::tempdir().expect("tempdir");
        let heddle_home: PathBuf = td.path().to_path_buf();

        let mut cmd = Command::new(bin);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env("HEDDLE_HOME", &heddle_home)
            .env("OPENROUTER_API_KEY", "test-key-headless")
            .env("HEDDLE_PROTOCOL_VERSION", "0.4.0");
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("spawn heddle-headless");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");

        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let lines_writer = lines.clone();
        thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            let mut reader = BufReader::new(stdout);
            let mut buf = String::new();
            loop {
                buf.clear();
                match reader.read_line(&mut buf) {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = buf
                            .trim_end_matches('\n')
                            .trim_end_matches('\r')
                            .to_string();
                        if !trimmed.is_empty() {
                            lines_writer.lock().unwrap().push(trimmed);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Headless {
            proc: child,
            lines,
            stdin,
            _tempdir: td,
        }
    }

    pub fn send_line(&mut self, line: &str) {
        let _ = writeln!(self.stdin, "{line}");
        let _ = self.stdin.flush();
    }

    pub fn wait_for_lines(&self, count: usize, timeout: Duration) -> Vec<String> {
        let start = Instant::now();
        loop {
            {
                let lines = self.lines.lock().unwrap();
                if lines.len() >= count {
                    return lines[..count].to_vec();
                }
            }
            if start.elapsed() > timeout {
                let lines = self.lines.lock().unwrap();
                panic!(
                    "timeout waiting for {count} lines, got {}: {:?}",
                    lines.len(),
                    *lines
                );
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn wait_for(
        &self,
        predicate: impl Fn(&[String]) -> bool,
        timeout: Duration,
    ) -> Vec<String> {
        let start = Instant::now();
        loop {
            {
                let lines = self.lines.lock().unwrap();
                if predicate(&lines) {
                    return lines.clone();
                }
            }
            if start.elapsed() > timeout {
                let lines = self.lines.lock().unwrap();
                panic!("timeout waiting for predicate; lines: {:?}", *lines);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn lines_snapshot(&self) -> Vec<String> {
        self.lines.lock().unwrap().clone()
    }

    pub fn line_count(&self) -> usize {
        self.lines.lock().unwrap().len()
    }

    pub fn heddle_home(&self) -> &Path {
        self._tempdir.path()
    }

    pub fn parse_line(line: &str) -> Value {
        serde_json::from_str(line).unwrap_or_else(|e| panic!("invalid JSON: {line:?} ({e})"))
    }

    pub fn close(&mut self) {
        let _ = self.stdin.flush();
        // Drop stdin: closing it signals EOF to the headless process.
        // We can't drop self.stdin (it's owned by self), so call shutdown via process kill.
        let _ = self.proc.kill();
        let _ = self.proc.wait();
    }

    pub fn wait_exit(&mut self, timeout: Duration) -> Option<i32> {
        let start = Instant::now();
        loop {
            match self.proc.try_wait() {
                Ok(Some(status)) => return status.code(),
                Ok(None) => {
                    if start.elapsed() > timeout {
                        return None;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return None,
            }
        }
    }
}

impl Drop for Headless {
    fn drop(&mut self) {
        let _ = self.proc.kill();
        let _ = self.proc.wait();
    }
}

pub fn init_msg() -> String {
    serde_json::json!({
        "type": "init",
        "id": "1",
        "protocol_version": "0.4.0",
        "config": {
            "model": "openrouter/auto",
            "system_prompt": "You are helpful.",
            "tools": ["read_file", "glob", "grep"],
            "max_iterations": 10
        }
    })
    .to_string()
}

pub fn parse_line(line: &str) -> Value {
    Headless::parse_line(line)
}
