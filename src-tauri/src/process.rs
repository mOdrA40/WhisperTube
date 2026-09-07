use std::{
    process::{Child, Command, ExitStatus, Stdio},
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

pub fn run_with_timeout(command: &mut Command, timeout: Duration) -> Result<ExitStatus, String> {
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|e| format!("Proses tidak bisa dimulai: {e}"))?;
    let started = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("Status proses tidak bisa dibaca: {e}"))?
        {
            return Ok(status);
        }
        if started.elapsed() >= timeout {
            terminate_child(&mut child);
            let _ = child.wait();
            return Err("Proses melewati batas waktu.".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
