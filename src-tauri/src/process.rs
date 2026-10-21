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

// Small optional executable probe, without pipes that descendants could keep
// open. Register its PID so shutdown/cancellation owns it just like media work.
pub fn probe_optional_runtime(
    command: &mut Command,
    timeout: Duration,
    active_pid: &std::sync::Mutex<Option<u32>>,
    cancelled: &AtomicBool,
) -> Result<bool, String> {
    if cancelled.load(Ordering::SeqCst) {
        return Err("Proses dibatalkan.".into());
    }
    hide_console(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Runtime JavaScript tidak bisa diperiksa: {error}")),
    };
    let pid = child.id();
    match active_pid.lock() {
        Ok(mut guard) => *guard = Some(pid),
        Err(_) => {
            terminate_child(&mut child);
            let _ = child.wait();
            return Err("Tracker proses runtime tidak tersedia.".into());
        }
    }
    let started = Instant::now();
    let outcome = loop {
        if cancelled.load(Ordering::SeqCst) {
            terminate_child(&mut child);
            let _ = child.wait();
            break Err("Proses dibatalkan.".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status.success()),
            Ok(None) => {}
            Err(error) => {
                terminate_child(&mut child);
                let _ = child.wait();
                break Err(format!(
                    "Status runtime JavaScript tidak bisa dibaca: {error}"
                ));
            }
        }
        if started.elapsed() >= timeout {
            terminate_child(&mut child);
            let _ = child.wait();
            eprintln!("WhisperTube: probe runtime JavaScript melewati batas waktu; runtime dinonaktifkan.");
            break Ok(false);
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut guard = active_pid
        .lock()
        .map_err(|_| "Tracker proses runtime tidak tersedia.".to_string())?;
    if *guard == Some(pid) {
        *guard = None;
    }
    outcome
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn runtime_probe_child() {
        if std::env::var_os("WHISPERTUBE_RUNTIME_PROBE_CHILD").is_some() {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    fn sleeping_child() -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "process::tests::runtime_probe_child",
                "--nocapture",
            ])
            .env("WHISPERTUBE_RUNTIME_PROBE_CHILD", "1");
        command
    }

    #[test]
    fn runtime_timeout_reaps_child_and_clears_tracker() {
        let tracker = Mutex::new(None);
        let cancelled = AtomicBool::new(false);
        let started = Instant::now();
        assert!(!probe_optional_runtime(
            &mut sleeping_child(),
            Duration::from_millis(100),
            &tracker,
            &cancelled
        )
        .unwrap());
        assert_eq!(*tracker.lock().unwrap(), None);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn runtime_cancel_is_propagated_and_reaps_tracked_child() {
        let tracker = Arc::new(Mutex::new(None));
        let cancelled = Arc::new(AtomicBool::new(false));
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let deadline = Instant::now() + Duration::from_secs(5);
                while tracker.lock().unwrap().is_none() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(tracker.lock().unwrap().is_some());
                cancelled.store(true, Ordering::SeqCst);
            });
            let error = probe_optional_runtime(
                &mut sleeping_child(),
                Duration::from_secs(10),
                &tracker,
                &cancelled,
            )
            .unwrap_err();
            assert!(error.contains("dibatalkan"));
        });
        assert_eq!(*tracker.lock().unwrap(), None);
    }
}
