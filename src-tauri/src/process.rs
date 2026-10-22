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
    if cancelled.is_some_and(|token| token.load(Ordering::SeqCst)) {
        return Err("Proses dibatalkan.".into());
    }
    hide_console(command);
    command.stdout(Stdio::null()).stdin(Stdio::null());
    #[cfg(unix)]
    let mut stderr = {
        use std::os::fd::OwnedFd;
        let (reader, writer) = std::os::unix::net::UnixStream::pair()
            .map_err(|e| format!("Stream stderr tidak bisa dibuat: {e}"))?;
        reader
            .set_nonblocking(true)
            .map_err(|e| format!("Stream stderr tidak bisa dikonfigurasi: {e}"))?;
        command.stderr(Stdio::from(OwnedFd::from(writer)));
        reader
    };
    #[cfg(windows)]
    command.stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Proses tidak bisa dimulai: {e}"))?;
    // Release the parent's copy of the Unix writer. Otherwise EOF would never
    // arrive while the Command remains alive in the caller.
    command.stderr(Stdio::null());
    #[cfg(windows)]
    let mut stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            terminate_child(&mut child);
            let _ = child.wait();
            return Err("stderr process tidak bisa dibaca.".into());
        }
    };
    let mut captured = Vec::with_capacity(stderr_limit.min(64 * 1024));
    let mut truncated = false;
    let mut eof = false;
    let started = Instant::now();
    loop {
        if !eof {
            match drain_stderr(&mut stderr, &mut captured, stderr_limit, &mut truncated) {
                Ok(closed) => eof = closed,
                Err(error) => {
                    terminate_child(&mut child);
                    let _ = child.wait();
                    return Err(format!("Gagal membaca stderr process: {error}"));
                }
            }
        }
        // Do not reap the group leader while descendants still own stderr.
        // Keeping its PID reserved also avoids signalling a recycled Unix PID.
        if eof {
            match child.try_wait() {
                Ok(Some(status)) => return Ok((status, captured_text(captured, truncated))),
                Err(error) => {
                    terminate_child(&mut child);
                    let _ = child.wait();
                    return Err(format!("Status proses tidak bisa dibaca: {error}"));
                }
                Ok(None) => {}
            }
        }
        if cancelled.is_some_and(|token| token.load(Ordering::SeqCst)) {
            terminate_child(&mut child);
            let _ = child.wait();
            return Err("Proses dibatalkan.".into());
        }
        if started.elapsed() >= timeout {
            terminate_child(&mut child);
            let _ = child.wait();
            let stderr = captured_text(captured, truncated);
            let detail = if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(" Detail: {}", stderr.trim())
            };
            return Err(format!("Proses melewati batas waktu.{detail}"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
type StderrReader = std::os::unix::net::UnixStream;
#[cfg(windows)]
type StderrReader = std::process::ChildStderr;

fn drain_stderr(
    reader: &mut StderrReader,
    captured: &mut Vec<u8>,
    limit: usize,
    truncated: &mut bool,
) -> std::io::Result<bool> {
    let mut buffer = [0u8; 8 * 1024];
    // Bound work per iteration even when a child continuously floods stderr,
    // so output capture cannot starve the timeout/cancellation checks.
    for _ in 0..8 {
        let read = match read_stderr(reader, &mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(error),
        };
        if read == 0 {
            return Ok(true);
        }
        let remaining = limit.saturating_sub(captured.len());
        let retained = remaining.min(read);
        captured.extend_from_slice(&buffer[..retained]);
        *truncated |= retained < read;
    }
    Ok(false)
}

#[cfg(unix)]
fn read_stderr(reader: &mut StderrReader, buffer: &mut [u8]) -> std::io::Result<usize> {
    reader.read(buffer)
}

#[cfg(windows)]
fn read_stderr(reader: &mut StderrReader, buffer: &mut [u8]) -> std::io::Result<usize> {
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn PeekNamedPipe(
            pipe: *mut std::ffi::c_void,
            buffer: *mut std::ffi::c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            remaining: *mut u32,
        ) -> i32;
    }
    let mut available = 0;
    // SAFETY: the owned ChildStderr handle is live; the only output pointer
    // refers to available, and null pointers request no peek buffer.
    let success = unsafe {
        PeekNamedPipe(
            reader.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(109) {
            // ERROR_BROKEN_PIPE: all writers closed.
            Ok(0)
        } else {
            Err(error)
        };
    }
    if available == 0 {
        return Err(std::io::ErrorKind::WouldBlock.into());
    }
    // This reader is the only consumer, so reading at most the bytes reported
    // by PeekNamedPipe cannot block waiting for a descendant's next write.
    let count = buffer.len().min(available as usize);
    reader.read(&mut buffer[..count])
}

fn captured_text(captured: Vec<u8>, truncated: bool) -> String {
    let mut output = String::from_utf8_lossy(&captured).into_owned();
    if truncated {
        output.push_str("\n[stderr dipotong karena terlalu panjang]");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    #[test]
    fn runtime_probe_child() {
        if std::env::var_os("WHISPERTUBE_RUNTIME_PROBE_CHILD").is_some() {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn captured_probe_child() {
        let Some(role) = std::env::var_os("WHISPERTUBE_CAPTURE_CHILD") else {
            return;
        };
        match role.to_str().unwrap() {
            "writer" => std::io::stderr()
                .write_all(&vec![b'x'; 2 * 1024 * 1024])
                .unwrap(),
            "sleeper" => std::thread::sleep(Duration::from_secs(30)),
            "parent" => {
                let mut command = capture_child("sleeper");
                command.stderr(Stdio::inherit()).stdout(Stdio::null());
                #[cfg(windows)]
                hide_console(&mut command);
                let child = command.spawn().unwrap();
                std::fs::write(
                    std::env::var_os("WHISPERTUBE_CAPTURE_PID_FILE").unwrap(),
                    child.id().to_string(),
                )
                .unwrap();
                // Intentionally leave stderr inherited after the parent exits;
                // the outer test owns cleanup of this synthetic descendant.
                drop(child);
            }
            _ => panic!("unknown capture fixture"),
        }
    }

    fn capture_child(role: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "process::tests::captured_probe_child",
                "--nocapture",
            ])
            .env("WHISPERTUBE_CAPTURE_CHILD", role);
        command
    }

    #[test]
    fn captured_output_drains_large_stderr_but_retains_only_the_limit() {
        let (status, output) =
            run_with_timeout_captured(&mut capture_child("writer"), Duration::from_secs(10), 1024)
                .unwrap();
        assert!(status.success());
        assert!(output.starts_with(&"x".repeat(1024)));
        assert!(output.contains("stderr dipotong"));
        assert!(output.len() < 1200);
    }

    #[test]
    fn captured_timeout_includes_descendants_holding_stderr_after_parent_exit() {
        let pid_file =
            std::env::temp_dir().join(format!("whispertube-capture-{}", uuid::Uuid::new_v4()));
        let mut command = capture_child("parent");
        command.env("WHISPERTUBE_CAPTURE_PID_FILE", &pid_file);
        let started = Instant::now();
        let result = run_with_timeout_captured(&mut command, Duration::from_millis(300), 1024);
        // Clean up even before assertions: Windows taskkill cannot discover a
        // process tree through a parent that has already exited.
        if let Ok(pid) = std::fs::read_to_string(&pid_file) {
            let pid = pid.parse::<u32>().unwrap();
            #[cfg(windows)]
            terminate_process_tree(pid);
            #[cfg(unix)]
            {
                let _ = Command::new("kill")
                    .args(["-KILL", "--", &pid.to_string()])
                    .status();
            }
            std::fs::remove_file(&pid_file).unwrap();
        }
        assert!(result.unwrap_err().contains("batas waktu"));
        assert!(started.elapsed() < Duration::from_secs(8));
    }

    #[test]
    fn captured_cancellation_does_not_wait_for_stderr_eof() {
        let cancelled = AtomicBool::new(false);
        let started = Instant::now();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                cancelled.store(true, Ordering::SeqCst);
            });
            let error = run_with_timeout_captured_cancelable(
                &mut capture_child("sleeper"),
                Duration::from_secs(10),
                1024,
                &cancelled,
            )
            .unwrap_err();
            assert!(error.contains("dibatalkan"));
        });
        assert!(started.elapsed() < Duration::from_secs(8));
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
