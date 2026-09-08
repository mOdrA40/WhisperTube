use std::{
    io::Read,
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn hide_console(command: &mut Command) {
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
    #[cfg(unix)]
    command.process_group(0);
}

pub fn terminate_process_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("taskkill");
        hide_console(&mut command);
        let _ = command
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut command = Command::new("kill");
        hide_console(&mut command);
        let _ = command
            .args(["-TERM", "--", &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let mut probe = Command::new("kill");
            hide_console(&mut probe);
            let exited = probe
                .args(["-0", "--", &format!("-{pid}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|status| !status.success())
                .unwrap_or(true);
            if exited {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let mut force = Command::new("kill");
        hide_console(&mut force);
        let _ = force
            .args(["-KILL", "--", &format!("-{pid}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

pub fn terminate_child(child: &mut Child) {
    terminate_process_tree(child.id());
    let _ = child.kill();
}

pub fn run_with_timeout_captured(
    command: &mut Command,
    timeout: Duration,
    stderr_limit: usize,
) -> Result<(ExitStatus, String), String> {
    run_with_timeout_captured_inner(command, timeout, stderr_limit, None)
}

pub fn run_with_timeout_captured_cancelable(
    command: &mut Command,
    timeout: Duration,
    stderr_limit: usize,
    cancelled: &AtomicBool,
) -> Result<(ExitStatus, String), String> {
    run_with_timeout_captured_inner(command, timeout, stderr_limit, Some(cancelled))
}

fn run_with_timeout_captured_inner(
    command: &mut Command,
    timeout: Duration,
    stderr_limit: usize,
    cancelled: Option<&AtomicBool>,
) -> Result<(ExitStatus, String), String> {
    command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Proses tidak bisa dimulai: {e}"))?;
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            terminate_child(&mut child);
            let _ = child.wait();
            return Err("stderr process tidak bisa dibaca.".into());
        }
    };
    let stderr_reader = std::thread::spawn(move || capture_bounded(stderr, stderr_limit));
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stderr = stderr_reader
                    .join()
                    .map_err(|_| "Reader stderr process gagal.".to_string())??;
                return Ok((status, stderr));
            }
            Err(error) => {
                terminate_child(&mut child);
                let _ = child.wait();
                let stderr = stderr_reader
                    .join()
                    .map_err(|_| "Reader stderr process gagal.".to_string())??;
                let detail = if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(" Detail: {}", stderr.trim())
                };
                return Err(format!("Status proses tidak bisa dibaca: {error}.{detail}"));
            }
            Ok(None) => {}
        }
        if cancelled.is_some_and(|token| token.load(Ordering::SeqCst)) {
            terminate_child(&mut child);
            let _ = child.wait();
            let _ = stderr_reader.join();
            return Err("Proses dibatalkan.".into());
        }
        if started.elapsed() >= timeout {
            terminate_child(&mut child);
            let _ = child.wait();
            let stderr = stderr_reader
                .join()
                .map_err(|_| "Reader stderr process gagal.".to_string())??;
            let detail = if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(" Detail: {}", stderr.trim())
            };
            return Err(format!("Proses melewati batas waktu.{detail}"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn capture_bounded(mut reader: impl Read, limit: usize) -> Result<String, String> {
    let mut captured = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0u8; 8 * 1024];
    let mut truncated = false;
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("Gagal membaca stderr process: {error}")),
        };
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(captured.len());
        let retained = remaining.min(read);
        captured.extend_from_slice(&buffer[..retained]);
        truncated |= retained < read;
    }
    let mut output = String::from_utf8_lossy(&captured).into_owned();
    if truncated {
        output.push_str("\n[stderr dipotong karena terlalu panjang]");
    }
    Ok(output)
}
