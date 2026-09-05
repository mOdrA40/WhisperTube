use std::process::{Child, Command, Stdio};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub fn hide_console(command: &mut Command) {
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);
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
            .args(["-TERM", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

pub fn terminate_child(child: &mut Child) {
    terminate_process_tree(child.id());
    let _ = child.kill();
}
