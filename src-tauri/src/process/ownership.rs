use std::{
    collections::BTreeMap,
    io,
    ops::{Deref, DerefMut},
    process::{Child, Command, ExitStatus},
    sync::{Mutex, OnceLock},
    time::Duration,
};

#[derive(Default)]
struct Registry {
    shutting_down: bool,
    children: BTreeMap<u32, Owner>,
}

#[derive(Clone)]
struct Owner {
    pid: u32,
    #[cfg(windows)]
    job: std::sync::Arc<windows::Job>,
}

impl Owner {
    fn terminate(&self) {
        #[cfg(windows)]
        self.job.terminate();
        #[cfg(unix)]
        if let Err(error) = unix_kill_remaining_group(self.pid) {
            eprintln!("WhisperTube: gagal menghentikan process group: {error}");
        }
    }
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

pub struct OwnedChild {
    child: Child,
    reaped: bool,
    owner: Owner,
}

impl OwnedChild {
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
        #[cfg(unix)]
        if !self.reaped {
            if !unix_leader_exited(self.owner.pid)? {
                return Ok(None);
            }
            // WNOWAIT reserves the leader PID/PGID until this group cleanup
            // finishes. Reaping first would allow an unrelated new process
            // to reuse the PID before the group signal is sent.
            unix_kill_remaining_group(self.owner.pid)?;
        }
        let status = self.child.try_wait()?;
        if status.is_some() && !self.reaped {
            #[cfg(windows)]
            // Job ownership also covers descendants with inherited handles
            // that are not their current stdout/stderr handles.
            self.owner.terminate();
            self.reaped = true;
            registry.children.remove(&self.owner.pid);
        }
        Ok(status)
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn terminate(&mut self) {
        #[cfg(windows)]
        self.owner.terminate();
        if !self.reaped {
            #[cfg(unix)]
            self.owner.terminate();
            let _ = self.child.kill();
        }
    }
}

#[cfg(unix)]
fn unix_leader_exited(pid: u32) -> io::Result<bool> {
    // SAFETY: siginfo_t accepts an all-zero initialization. waitid writes only
    // this live buffer; WNOWAIT observes this owned child without reaping it.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    loop {
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == 0 {
            return Ok(unsafe { info.si_pid() } != 0);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(unix)]
fn unix_kill_remaining_group(pid: u32) -> io::Result<()> {
    // SAFETY: the group belongs to a child whose leader has not been reaped.
    // No pipeline child is allowed to leave behind an independent daemon.
    if unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

pub fn terminate_registered(pid: u32) {
    // Reaping/removal and cancellation share the registry lock, so a stale
    // active_pid cannot kill an unrelated process after PID reuse.
    let registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(owner) = registry.children.get(&pid) {
        owner.terminate();
    }
}

impl Deref for OwnedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}
impl DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.reaped {
            self.terminate();
            if let Err(error) = self.wait() {
                eprintln!("WhisperTube: gagal menunggu cleanup process: {error}");
            }
        }
        // Closing the Windows job also removes descendants after normal exit.
        if !self.reaped {
            registry()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .children
                .remove(&self.owner.pid);
        }
    }
}

pub fn spawn(command: &mut Command) -> io::Result<OwnedChild> {
    // Shutdown and spawn share a lock: no child can appear after the shutdown
    // snapshot, including a helper whose cancellation flag was just reset.
    let mut registry = registry()
        .lock()
        .map_err(|_| io::Error::other("Tracker proses terkunci."))?;
    if registry.shutting_down {
        return Err(io::Error::other("Aplikasi sedang ditutup."));
    }
    super::hide_console(command);
    #[cfg(windows)]
    let job = {
        use std::os::windows::process::CommandExt;
        // Assign the suspended process before it can create any descendant.
        let job = std::sync::Arc::new(windows::Job::new()?);
        command.creation_flags(0x08000000 | 0x00000004);
        job
    };
    let result = command.spawn();
    super::hide_console(command);
    #[allow(unused_mut)]
    let mut child = result?;
    #[cfg(windows)]
    if let Err(error) = job.assign_and_resume(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let owner = Owner {
        pid: child.id(),
        #[cfg(windows)]
        job,
    };
    registry.children.insert(child.id(), owner.clone());
    Ok(OwnedChild {
        child,
        owner,
        reaped: false,
    })
}

pub fn shutdown() {
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    registry.shutting_down = true;
    for owner in registry.children.values() {
        owner.terminate();
    }
}

#[cfg(windows)]
mod windows {
    use std::{
        ffi::c_void,
        io,
        mem::{size_of, zeroed},
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::Child,
    };
    type Handle = *mut c_void;
    #[repr(C)]
    #[derive(Default)]
    struct BasicLimits {
        process_time: i64,
        job_time: i64,
        flags: u32,
        min_working_set: usize,
        max_working_set: usize,
        active_process_limit: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io_counters: [u64; 6],
        process_memory: usize,
        job_memory: usize,
        peak_process_memory: usize,
        peak_job_memory: usize,
    }
    #[repr(C)]
    struct ThreadEntry {
        size: u32,
        usage: u32,
        id: u32,
        process_id: u32,
        priority: i32,
        delta: i32,
        flags: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(
            job: Handle,
            class: i32,
            information: *const c_void,
            size: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn TerminateJobObject(job: Handle, exit_code: u32) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
        fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
        fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
        fn OpenThread(access: u32, inherit: i32, id: u32) -> Handle;
        fn ResumeThread(thread: Handle) -> u32;
    }
    pub struct Job(OwnedHandle);
    impl Job {
        pub fn new() -> io::Result<Self> {
            // SAFETY: null security/name parameters create an unnamed,
            // non-inheritable job. OwnedHandle closes the returned live handle.
            let raw = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self(unsafe { OwnedHandle::from_raw_handle(raw) });
            let mut limits = ExtendedLimits::default();
            // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            limits.basic.flags = 0x00002000;
            // SAFETY: class 9 takes a correctly sized/aligned extended limits struct.
            if unsafe {
                SetInformationJobObject(
                    job.0.as_raw_handle(),
                    9,
                    &limits as *const _ as *const c_void,
                    size_of::<ExtendedLimits>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
        pub fn assign_and_resume(&self, child: &Child) -> io::Result<()> {
            // SAFETY: both handles are owned and live; the child is suspended.
            if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), child.as_raw_handle()) }
                == 0
            {
                return Err(io::Error::last_os_error());
            }
            // The standard Child API does not expose the primary thread handle.
            // Enumerate the single thread before any child code can execute.
            let raw = unsafe { CreateToolhelp32Snapshot(4, 0) }; // TH32CS_SNAPTHREAD
            if raw == -1isize as Handle {
                return Err(io::Error::last_os_error());
            }
            let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
            // SAFETY: ThreadEntry contains only integers; initialize its size.
            let mut entry: ThreadEntry = unsafe { zeroed() };
            entry.size = size_of::<ThreadEntry>() as u32;
            let mut found = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
            while found != 0 {
                if entry.process_id == child.id() {
                    let raw = unsafe { OpenThread(0x0002, 0, entry.id) }; // THREAD_SUSPEND_RESUME
                    if raw.is_null() {
                        return Err(io::Error::last_os_error());
                    }
                    let thread = unsafe { OwnedHandle::from_raw_handle(raw) };
                    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                        return Err(io::Error::last_os_error());
                    }
                    return Ok(());
                }
                entry.size = size_of::<ThreadEntry>() as u32;
                found = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
            }
            Err(io::Error::other(
                "Thread utama child process tidak ditemukan.",
            ))
        }
        pub fn terminate(&self) {
            // SAFETY: the job handle remains live during termination.
            if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
                eprintln!(
                    "WhisperTube: gagal menghentikan job process: {}",
                    io::Error::last_os_error()
                );
            }
        }
    }
}
