//! niubash#140 / #125 follow-up: launcher output paths must survive a reader
//! that closes stdout early (`niu --version | head -1`, `| true`).
//!
//! std `println!`/`print!` panic on any stdout write error; with
//! `panic = "abort"` that aborts the process. The shadowing writers route
//! through the engine's closed-pipe rule (BrokenPipe / os error 232 → exit 0),
//! matching the SIGPIPE termination GNU exhibits when its reader goes away.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn niu_binary() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_BIN_EXE_niu"));
    if p.exists() {
        return p;
    }
    let mut fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fallback.push("target");
    fallback.push("debug");
    fallback.push(if cfg!(windows) { "niu.exe" } else { "niubash" });
    fallback
}

/// Spawn `niu args...` with stdout piped, read at most `read_bytes` then
/// close the pipe and wait for the child. Assert: process exits on its own
/// (no hang), exit code 0, and stderr carries no panic/abort text.
fn assert_survives_closed_pipe(args: &[&str], read_bytes: usize) {
    let mut child = Command::new(niu_binary())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn niu");

    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut buf = vec![0u8; read_bytes.max(1)];
    let _ = stdout.read(&mut buf);
    drop(stdout); // reader closes — subsequent writes hit os error 232

    let start = Instant::now();
    let status = loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => break status,
            None => {
                assert!(
                    start.elapsed() < Duration::from_secs(30),
                    "niu {args:?} hung after reader closed stdout"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };

    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("piped stderr")
        .read_to_string(&mut stderr)
        .ok();

    assert!(
        !stderr.contains("panicked") && !stderr.contains("failed printing to stdout"),
        "niu {args:?} panicked on closed stdout: {stderr}"
    );
    assert!(
        status.code() == Some(0),
        "niu {args:?} exit {:?} after closed stdout; stderr: {stderr}",
        status.code()
    );
}

#[test]
fn version_survives_reader_closing_stdout() {
    // The original report: `niu --version | head -1` (one line read, then
    // the pipe dies while the launcher still has lines to write).
    assert_survives_closed_pipe(&["--version"], 32);
}

#[test]
fn version_survives_immediately_closed_stdout() {
    // `niu --version | true` — reader exits before the first write.
    assert_survives_closed_pipe(&["--version"], 0);
}

#[test]
fn help_survives_reader_closing_stdout() {
    // Same class for the longer usage text: `niu --help | head -3`.
    assert_survives_closed_pipe(&["--help"], 96);
}
