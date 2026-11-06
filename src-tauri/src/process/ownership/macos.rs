//! Darwin killpg skips zombies and can return EPERM for a dead-only group.
//! Keep the owned leader unreaped while verifying that no live member remains.
use std::{
    io,
    mem::{size_of, MaybeUninit},
};

fn members(group: u32) -> io::Result<Vec<libc::pid_t>> {
    // Normal media tools have few children. Bound exceptional inventories
    // rather than allocating or retrying indefinitely while holding ownership.
    const MAX_MEMBERS: usize = 65_536;
    let mut pids = vec![0; 64];
    loop {
        // SAFETY: errno is thread-local; pids owns the full writable buffer.
        // libproc returns a PID count here (unlike proc_listpids's byte count).
        let (count, error) = unsafe {
            *libc::__error() = 0;
            let count = libc::proc_listpgrppids(
                group as libc::pid_t,
                pids.as_mut_ptr().cast(),
                (pids.len() * size_of::<libc::pid_t>()) as libc::c_int,
            );
            (count, io::Error::last_os_error())
        };
        if count < 0 || (count == 0 && error.raw_os_error() != Some(0)) {
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if count as usize >= pids.len() {
            if pids.len() == MAX_MEMBERS {
                return Err(io::Error::other(
                    "Inventaris process group macOS terlalu besar.",
                ));
            }
            pids.resize(pids.len() * 2, 0);
            continue;
        }
        pids.truncate(count as usize);
        if pids.iter().any(|pid| *pid <= 0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PID process group macOS tidak valid.",
            ));
        }
        pids.sort_unstable();
        pids.dedup();
        return Ok(pids);
    }
}

fn member_is_live(pid: libc::pid_t, group: u32) -> io::Result<bool> {
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::uninit();
    loop {
        // SAFETY: the buffer has the exact ABI size; only assume initialization
        // when libproc confirms that it wrote the entire struct.
        let count = unsafe {
            *libc::__error() = 0;
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size_of::<libc::proc_bsdinfo>() as libc::c_int,
            )
        };
        if count == size_of::<libc::proc_bsdinfo>() as libc::c_int {
            // SAFETY: the full struct was written by proc_pidinfo above.
            let info = unsafe { info.assume_init() };
            if info.pbi_pid != pid as u32 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Identitas proses macOS tidak cocok.",
                ));
            }
            // A PID that exited or moved out of this group no longer prevents
            // cleanup. A live member of this group must never be overlooked.
            return Ok(info.pbi_pgid == group && info.pbi_status != libc::SZOMB);
        }
        if count != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Metadata proses macOS tidak lengkap.",
            ));
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::ESRCH) => return Ok(false),
            Some(0) => return Err(io::Error::other("Metadata proses macOS tidak tersedia.")),
            _ => return Err(error),
        }
    }
}

pub(super) fn group_is_inactive(group: u32) -> io::Result<bool> {
    let before = members(group)?;
    for pid in &before {
        if member_is_live(*pid, group)? {
            return Ok(false);
        }
    }
    // A member may have forked just before it exited. If the inventory changed,
    // we cannot prove quiescence and must preserve the original signal error.
    Ok(before == members(group)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        process::Command,
        time::{Duration, Instant},
    };

    #[test]
    fn zombie_only_group_is_reaped_with_its_original_exit_status() {
        let mut child = super::super::spawn(Command::new("sh").args(["-c", "exit 7"])).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !super::super::unix_leader_exited(child.id()).unwrap() {
            assert!(Instant::now() < deadline, "fixture did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(group_is_inactive(child.id()).unwrap());
        assert_eq!(child.wait().unwrap().code(), Some(7));
        assert_eq!(child.try_wait().unwrap().unwrap().code(), Some(7));
    }

    #[test]
    fn live_group_is_not_mistaken_for_zombies_and_can_still_be_cancelled() {
        let mut child = super::super::spawn(Command::new("sleep").arg("30")).unwrap();
        assert!(!group_is_inactive(child.id()).unwrap());
        assert!(child.try_wait().unwrap().is_none());
        child.terminate();
        assert!(!child.wait().unwrap().success());
    }
}
