mod ownership;
pub use ownership::shutdown;

use std::{
    io::Read,
    process::{Command, ExitStatus, Stdio},
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

pub fn terminate_child(child: &mut ownership::OwnedChild) {
    child.terminate();
}

pub struct ActiveProcessGuard<'a> {
    tracker: &'a std::sync::Mutex<Option<u32>>,
    pid: u32,
}

impl<'a> ActiveProcessGuard<'a> {
    pub fn new(tracker: &'a std::sync::Mutex<Option<u32>>, pid: u32) -> Result<Self, String> {
        *tracker
            .lock()
            .map_err(|_| "Tracker proses tidak tersedia.")? = Some(pid);
        Ok(Self { tracker, pid })
    }
}

impl Drop for ActiveProcessGuard<'_> {
    fn drop(&mut self) {
        let mut active = self.tracker.lock().unwrap_or_else(|error| {
            eprintln!("WhisperTube: tracker proses poisoned saat cleanup.");
            error.into_inner()
        });
        if *active == Some(self.pid) {
            *active = None;
        }
    }
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
    let mut child = match ownership::spawn(command) {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Runtime JavaScript tidak bisa diperiksa: {error}")),
    };
    let _active = ActiveProcessGuard::new(active_pid, child.id())?;
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

pub enum OutputEvent<'a> {
    Data(OutputStream, &'a [u8]),
    Tick,
}

// Every stream is polled in this loop, including after the leader has exited.
// No blocking reader thread may outlive cancellation or the caller's watchdog.
pub fn run_piped(
    command: &mut Command,
    timeout: Option<Duration>,
    cancelled: Option<&AtomicBool>,
    on_spawn: impl FnOnce(u32) -> Result<(), String>,
    mut on_event: impl FnMut(OutputEvent<'_>) -> Result<(), String>,
) -> Result<ExitStatus, String> {
    let started = Instant::now();
    let check = || -> Result<(), String> {
        if cancelled.is_some_and(|token| token.load(Ordering::SeqCst)) {
            return Err("Proses dibatalkan.".into());
        }
        if timeout.is_some_and(|timeout| started.elapsed() >= timeout) {
            return Err("Proses melewati batas waktu.".into());
        }
        Ok(())
    };
    check()?;
    hide_console(command);
    command.stdin(Stdio::null());
    #[cfg(unix)]
    let (mut stdout, mut stderr) = {
        use std::os::fd::OwnedFd;
        fn stream() -> std::io::Result<(PipeReader, Stdio)> {
            let (reader, writer) = std::os::unix::net::UnixStream::pair()?;
            reader.set_nonblocking(true)?;
            Ok((reader, Stdio::from(OwnedFd::from(writer))))
        }
        let (stdout, out_writer) = stream().map_err(|e| format!("Stream stdout: {e}"))?;
        let (stderr, err_writer) = stream().map_err(|e| format!("Stream stderr: {e}"))?;
        command.stdout(out_writer).stderr(err_writer);
        (stdout, stderr)
    };
    #[cfg(windows)]
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child =
        ownership::spawn(command).map_err(|e| format!("Proses tidak bisa dimulai: {e}"))?;
    // Drop the Command's copies of Unix writers, otherwise EOF never arrives.
    command.stdout(Stdio::null()).stderr(Stdio::null());
    on_spawn(child.id())?;
    #[cfg(windows)]
    let (mut stdout, mut stderr) = (
        PipeReader::Stdout(child.stdout.take().expect("piped stdout")),
        PipeReader::Stderr(child.stderr.take().expect("piped stderr")),
    );
    let outcome = (|| {
        let mut stdout_eof = false;
        let mut stderr_eof = false;
        let mut buffer = [0u8; 8 * 1024];
        loop {
            check()?;
            for (stream, reader, eof) in [
                (OutputStream::Stdout, &mut stdout, &mut stdout_eof),
                (OutputStream::Stderr, &mut stderr, &mut stderr_eof),
            ] {
                if *eof {
                    continue;
                }
                // A flood must not starve the other stream or watchdog.
                for _ in 0..8 {
                    let count = match read_pipe(reader, &mut buffer) {
                        Ok(0) => {
                            *eof = true;
                            break;
                        }
                        Ok(count) => count,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) => return Err(format!("Gagal membaca output process: {e}")),
                    };
                    on_event(OutputEvent::Data(stream, &buffer[..count]))?;
                }
            }
            on_event(OutputEvent::Tick)?;
            check()?;
            // Reserve the leader PID until all inherited streams have closed.
            if stdout_eof && stderr_eof {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|e| format!("Status proses tidak bisa dibaca: {e}"))?
                {
                    return Ok(status);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if outcome.is_err() {
        terminate_child(&mut child);
        let _ = child.wait();
    }
    outcome
}

fn run_with_timeout_captured_inner(
    command: &mut Command,
    timeout: Duration,
    stderr_limit: usize,
    cancelled: Option<&AtomicBool>,
) -> Result<(ExitStatus, String), String> {
    let mut captured = Vec::with_capacity(stderr_limit.min(64 * 1024));
    let mut truncated = false;
    let result = run_piped(
        command,
        Some(timeout),
        cancelled,
        |_| Ok(()),
        |event| {
            if let OutputEvent::Data(OutputStream::Stderr, bytes) = event {
                let retained = bytes.len().min(stderr_limit.saturating_sub(captured.len()));
                captured.extend_from_slice(&bytes[..retained]);
                truncated |= retained < bytes.len();
            }
            Ok(())
        },
    );
    let text = captured_text(captured, truncated);
    match result {
        Ok(status) => Ok((status, text)),
        Err(error) if error.contains("batas waktu") && !text.trim().is_empty() => {
            Err(format!("{error} Detail: {}", text.trim()))
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
type PipeReader = std::os::unix::net::UnixStream;
#[cfg(windows)]
enum PipeReader {
    Stdout(std::process::ChildStdout),
    Stderr(std::process::ChildStderr),
}

#[cfg(unix)]
fn read_pipe(reader: &mut PipeReader, buffer: &mut [u8]) -> std::io::Result<usize> {
    reader.read(buffer)
}

#[cfg(windows)]
fn read_pipe(reader: &mut PipeReader, buffer: &mut [u8]) -> std::io::Result<usize> {
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
    let handle = match reader {
        PipeReader::Stdout(reader) => reader.as_raw_handle(),
        PipeReader::Stderr(reader) => reader.as_raw_handle(),
    };
    let mut available = 0;
    // SAFETY: the reader owns the live handle; available is the only output.
    let success = unsafe {
        PeekNamedPipe(
            handle,
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
            Ok(0)
        } else {
            Err(error)
        };
    }
    if available == 0 {
        return Err(std::io::ErrorKind::WouldBlock.into());
    }
    let count = buffer.len().min(available as usize);
    // The sole reader consumes no more than PeekNamedPipe reported available.
    match reader {
        PipeReader::Stdout(reader) => reader.read(&mut buffer[..count]),
        PipeReader::Stderr(reader) => reader.read(&mut buffer[..count]),
    }
}

// Incremental, bounded line decoding for progress output. A line without a
// newline cannot monopolize the watchdog or allocate an unbounded buffer.
#[derive(Default)]
pub struct OutputLines {
    bytes: Vec<u8>,
    truncated: bool,
}

impl OutputLines {
    pub fn push(
        &mut self,
        bytes: &[u8],
        mut emit: impl FnMut(String) -> Result<(), String>,
    ) -> Result<(), String> {
        const LIMIT: usize = 128 * 1024;
        for part in bytes.split_inclusive(|byte| *byte == b'\n') {
            let retained = part.len().min(LIMIT.saturating_sub(self.bytes.len()));
            self.bytes.extend_from_slice(&part[..retained]);
            self.truncated |= retained < part.len();
            if part.last() == Some(&b'\n') {
                if let Some(line) = self.finish() {
                    emit(line)?;
                }
            }
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Option<String> {
        if self.bytes.is_empty() && !self.truncated {
            return None;
        }
        while matches!(self.bytes.last(), Some(b'\n' | b'\r')) {
            self.bytes.pop();
        }
        let mut line = String::from_utf8_lossy(&self.bytes).into_owned();
        self.bytes.clear();
        if std::mem::take(&mut self.truncated) {
            line.push_str("\n[output line dipotong karena terlalu panjang]");
        }
        Some(line)
    }
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
            "closed-streams" => {
                #[cfg(windows)]
                {
                    #[link(name = "kernel32")]
                    unsafe extern "system" {
                        fn GetStdHandle(kind: u32) -> *mut std::ffi::c_void;
                        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
                    }
                    // This isolated fixture exits without returning to libtest.
                    unsafe {
                        CloseHandle(GetStdHandle(-11i32 as u32));
                        CloseHandle(GetStdHandle(-12i32 as u32));
                    }
                }
                #[cfg(unix)]
                {
                    unsafe extern "C" {
                        fn close(fd: i32) -> i32;
                    }
                    unsafe {
                        close(1);
                        close(2);
                    }
                }
                std::thread::sleep(Duration::from_millis(250));
                std::process::exit(0);
            }
            "large-stdout" => std::io::stdout()
                .write_all(&vec![b'x'; 17 * 1024 * 1024])
                .unwrap(),
            "flood" => loop {
                std::io::stderr().write_all(&[b'x'; 8192]).unwrap();
            },
            "shutdown-parent" => {
                let file = std::path::PathBuf::from(
                    std::env::var_os("WHISPERTUBE_CAPTURE_PID_FILE").unwrap(),
                );
                let writer = file.clone();
                std::thread::spawn(move || {
                    let _ = run_piped(
                        &mut capture_child("sleeper"),
                        Some(Duration::from_secs(30)),
                        None,
                        |pid| {
                            std::fs::write(writer, pid.to_string()).unwrap();
                            Ok(())
                        },
                        |_| Ok(()),
                    );
                });
                let deadline = Instant::now() + Duration::from_secs(5);
                while !file.exists() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(file.exists());
                shutdown();
                assert!(run_with_timeout_captured(
                    &mut capture_child("sleeper"),
                    Duration::from_secs(5),
                    100
                )
                .unwrap_err()
                .contains("sedang ditutup"));
                // Bypass all Rust destructors: shutdown must have terminated the
                // registered helper already, without waiting for its worker.
                std::process::exit(0);
            }
            "parent" => {
                let mut command = capture_child("sleeper");
                command.stderr(Stdio::inherit()).stdout(Stdio::inherit());
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
        if pid_file.exists() {
            #[cfg(windows)]
            assert_process_exited(std::fs::read_to_string(&pid_file).unwrap().parse().unwrap());
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

    #[test]
    fn closed_streams_do_not_imply_process_completion() {
        let started = Instant::now();
        let (status, _) = run_with_timeout_captured(
            &mut capture_child("closed-streams"),
            Duration::from_secs(5),
            1024,
        )
        .unwrap();
        assert!(status.success());
        assert!(started.elapsed() >= Duration::from_millis(200));
    }

    #[test]
    fn cancelled_before_spawn_never_starts_a_child() {
        let cancelled = AtomicBool::new(true);
        let error = run_piped(
            &mut capture_child("sleeper"),
            Some(Duration::from_secs(5)),
            Some(&cancelled),
            |_| panic!("cancelled command spawned"),
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("dibatalkan"));
    }

    #[test]
    fn callback_failure_reaps_child_and_tracker_unwinds_after_panic() {
        let tracker = Mutex::new(None);
        let mut guard = None;
        let result = run_piped(
            &mut capture_child("sleeper"),
            Some(Duration::from_secs(5)),
            None,
            |pid| {
                guard = Some(ActiveProcessGuard::new(&tracker, pid)?);
                Ok(())
            },
            |_| Err("callback failure".into()),
        );
        assert_eq!(result.unwrap_err(), "callback failure");
        #[cfg(windows)]
        assert_process_exited(tracker.lock().unwrap().unwrap());
        drop(guard);
        assert_eq!(*tracker.lock().unwrap(), None);
        let panic = std::panic::catch_unwind(|| {
            let _guard = ActiveProcessGuard::new(&tracker, 123).unwrap();
            panic!("synthetic observer failure");
        });
        assert!(panic.is_err());
        assert_eq!(*tracker.lock().unwrap(), None);
    }

    #[test]
    fn output_lines_preserve_split_utf8_crlf_and_bound_unterminated_floods() {
        let mut lines = OutputLines::default();
        let mut output = Vec::new();
        let bytes = "你好\r\n\nlast".as_bytes();
        for byte in bytes {
            lines
                .push(&[*byte], |line| {
                    output.push(line);
                    Ok(())
                })
                .unwrap();
        }
        assert_eq!(output, ["你好", ""]);
        assert_eq!(lines.finish().as_deref(), Some("last"));
        assert!(lines.finish().is_none());
        lines
            .push(&vec![b'x'; 2 * 1024 * 1024], |_| {
                panic!("unterminated line emitted")
            })
            .unwrap();
        assert!(lines.bytes.len() <= 128 * 1024);
        let line = lines.finish().unwrap();
        assert!(line.starts_with(&"x".repeat(128 * 1024)));
        assert!(line.ends_with("[output line dipotong karena terlalu panjang]"));
    }

    #[test]
    fn output_flood_cannot_starve_cancellation_or_watchdog() {
        let cancelled = AtomicBool::new(false);
        let started = Instant::now();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(150));
                cancelled.store(true, Ordering::SeqCst);
            });
            let error = run_with_timeout_captured_cancelable(
                &mut capture_child("flood"),
                Duration::from_secs(5),
                1024,
                &cancelled,
            )
            .unwrap_err();
            assert!(error.contains("dibatalkan"));
        });
        assert!(started.elapsed() < Duration::from_secs(8));
        let error = run_with_timeout_captured(
            &mut capture_child("flood"),
            Duration::from_millis(150),
            1024,
        )
        .unwrap_err();
        assert!(error.contains("batas waktu"));
    }

    #[test]
    fn cancellation_after_leader_exit_closes_inherited_pipes_and_kills_descendant() {
        let pid_file =
            std::env::temp_dir().join(format!("whispertube-cancel-{}", uuid::Uuid::new_v4()));
        let cancelled = AtomicBool::new(false);
        let mut command = capture_child("parent");
        command.env("WHISPERTUBE_CAPTURE_PID_FILE", &pid_file);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let deadline = Instant::now() + Duration::from_secs(5);
                while !pid_file.exists() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                std::thread::sleep(Duration::from_millis(150));
                cancelled.store(true, Ordering::SeqCst);
            });
            assert!(run_with_timeout_captured_cancelable(
                &mut command,
                Duration::from_secs(8),
                1024,
                &cancelled
            )
            .unwrap_err()
            .contains("dibatalkan"));
        });
        #[cfg(windows)]
        assert_process_exited(std::fs::read_to_string(&pid_file).unwrap().parse().unwrap());
        std::fs::remove_file(pid_file).unwrap();
    }

    #[cfg(windows)]
    fn assert_process_exited(pid: u32) {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
            fn WaitForSingleObject(handle: *mut std::ffi::c_void, timeout: u32) -> u32;
        }
        // SYNCHRONIZE only; never read or modify the test process's memory.
        let raw = unsafe { OpenProcess(0x00100000, 0, pid) };
        if raw.is_null() {
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(87),
                "cannot determine helper exit"
            );
            return;
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        assert_eq!(
            unsafe { WaitForSingleObject(handle.as_raw_handle(), 5000) },
            0,
            "helper remains running: {pid}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn shutdown_kills_untracked_helpers_before_parent_exits_and_rejects_late_spawn() {
        let file =
            std::env::temp_dir().join(format!("whispertube-shutdown-{}", uuid::Uuid::new_v4()));
        let mut command = capture_child("shutdown-parent");
        command
            .env("WHISPERTUBE_CAPTURE_PID_FILE", &file)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut parent = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        let status = loop {
            if let Some(status) = parent.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                terminate_process_tree(parent.id());
                panic!("shutdown fixture hung");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success());
        assert_process_exited(std::fs::read_to_string(&file).unwrap().parse().unwrap());
        std::fs::remove_file(file).unwrap();
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
