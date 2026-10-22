use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::fs::{fchown, MetadataExt, PermissionsExt},
    path::Path,
};

// Configure an empty export before transcript bytes are copied. A failure must
// abort publication; mode bits alone cannot represent ACLs or group ownership.
pub fn preserve(target: &Path, output: &File) -> io::Result<()> {
    // No truncation: also require that the caller could write the old target.
    let target = OpenOptions::new().write(true).open(target)?;
    let metadata = target.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Target export bukan file biasa.",
        ));
    }
    let staging = output.metadata()?;
    // Keep every mode-based reader blocked while changing identity/ACL. An
    // early chmod to the target mode would let a denied user acquire a read fd
    // before their deny entry is installed; later ACL changes do not revoke it.
    output.set_permissions(std::fs::Permissions::from_mode(0o000))?;
    if staging.uid() != metadata.uid() || staging.gid() != metadata.gid() {
        fchown(output, Some(metadata.uid()), Some(metadata.gid()))?;
    }
    copy_acl(&target, output)?;
    // The complete ACL is now present (or inherited entries removed). Restoring
    // the original mode also restores the original POSIX mask, without ever
    // granting access beyond the target's policy.
    output.set_permissions(metadata.permissions())
}

#[cfg(target_os = "linux")]
pub fn create_private(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    linux::check_parent(path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Folder staging tidak valid.")
    })?)?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        ffi::{c_char, c_void, CStr, CString},
        fs::File,
        io,
        os::{fd::AsRawFd, unix::ffi::OsStrExt},
        path::Path,
    };

    unsafe extern "C" {
        fn getxattr(
            path: *const c_char,
            name: *const c_char,
            value: *mut c_void,
            size: usize,
        ) -> isize;
        fn fgetxattr(fd: i32, name: *const c_char, value: *mut c_void, size: usize) -> isize;
        fn fsetxattr(
            fd: i32,
            name: *const c_char,
            value: *const c_void,
            size: usize,
            flags: i32,
        ) -> i32;
        fn fremovexattr(fd: i32, name: *const c_char) -> i32;
    }

    pub(super) const ACCESS: &CStr = c"system.posix_acl_access";
    const NFS4: &CStr = c"system.nfs4_acl";

    pub(super) fn check_parent(path: &Path) -> io::Result<()> {
        let path = CString::new(path.as_os_str().as_bytes())?;
        // SAFETY: both strings are NUL-terminated; size zero only queries size.
        let size = unsafe { getxattr(path.as_ptr(), NFS4.as_ptr(), std::ptr::null_mut(), 0) };
        if size >= 0 {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "Export atomik pada direktori dengan ACL NFSv4 belum didukung. Pilih direktori lokal."));
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(61 | 95) => Ok(()),
            _ => Err(error),
        }
    }

    pub(super) fn read(file: &File, name: &CStr) -> io::Result<Option<Vec<u8>>> {
        // Linux limits each xattr value to 64 KiB. A single read avoids a
        // size-query/read race; ERANGE or any unexpected error aborts export.
        let mut value = vec![0u8; 64 * 1024];
        // SAFETY: live fd, NUL-terminated name and a writable buffer of size len.
        let count = unsafe {
            fgetxattr(
                file.as_raw_fd(),
                name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        };
        if count < 0 {
            let error = io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(61 | 95) => Ok(None), // ENODATA / EOPNOTSUPP: no POSIX access ACL.
                _ => Err(error),
            };
        }
        value.truncate(count as usize);
        Ok(Some(value))
    }

    pub(super) fn write(file: &File, name: &CStr, value: Option<&[u8]>) -> io::Result<()> {
        // SAFETY: live fd, NUL-terminated name and an initialized buffer with
        // its exact size. Neither syscall retains pointers after returning.
        let status = unsafe {
            match value {
                Some(value) => fsetxattr(
                    file.as_raw_fd(),
                    name.as_ptr(),
                    value.as_ptr().cast(),
                    value.len(),
                    0,
                ),
                None => fremovexattr(file.as_raw_fd(), name.as_ptr()),
            }
        };
        if status == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if value.is_none() && matches!(error.raw_os_error(), Some(61 | 95)) {
            Ok(())
        } else {
            Err(error)
        }
    }
}

#[cfg(target_os = "linux")]
fn copy_acl(target: &File, output: &File) -> io::Result<()> {
    if linux::read(target, c"system.nfs4_acl")?.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "ACL NFSv4 target export belum didukung.",
        ));
    }
    let acl = linux::read(target, linux::ACCESS)?;
    linux::write(output, linux::ACCESS, acl.as_deref())
}

#[cfg(target_os = "macos")]
pub fn create_private(path: &Path) -> io::Result<File> {
    use std::{
        ffi::{c_char, c_void, CString},
        os::{fd::FromRawFd, unix::ffi::OsStrExt},
    };
    unsafe extern "C" {
        fn acl_init(count: i32) -> *mut c_void;
        fn acl_free(acl: *mut c_void) -> i32;
        fn acl_get_flagset_np(acl: *mut c_void, flags: *mut *mut c_void) -> i32;
        fn acl_add_flag_np(flags: *mut c_void, flag: u32) -> i32;
        fn filesec_init() -> *mut c_void;
        fn filesec_free(security: *mut c_void);
        fn filesec_set_property(security: *mut c_void, property: i32, value: *const c_void) -> i32;
        fn openx_np(path: *const c_char, flags: i32, security: *mut c_void) -> i32;
    }
    let path = CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: native constructors return owned opaque security objects.
    let acl = unsafe { acl_init(0) };
    if acl.is_null() {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        // SAFETY: acl is live; flags points to a writable pointer-sized slot.
        let mut flags = std::ptr::null_mut();
        // ACL_FLAG_NO_INHERIT = 1 << 17.
        if unsafe { acl_get_flagset_np(acl, &mut flags) } != 0
            || unsafe { acl_add_flag_np(flags, 1 << 17) } != 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: constructor returns an owned filesec object.
        let security = unsafe { filesec_init() };
        if security.is_null() {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            // Darwin mode_t; FILESEC_MODE=4, FILESEC_ACL=5.
            let mode: u16 = 0o600;
            // SAFETY: live objects and correctly typed values, copied by filesec.
            if unsafe { filesec_set_property(security, 4, (&mode as *const u16).cast()) } != 0
                || unsafe { filesec_set_property(security, 5, (&acl as *const *mut c_void).cast()) }
                    != 0
            {
                return Err(io::Error::last_os_error());
            }
            // Unlike open(..., 0600), an explicit non-inheriting ACL excludes
            // inherited allow entries from the instant the file exists.
            // SAFETY: NUL-terminated path and live filesec; O_WRONLY | O_CREAT |
            // O_EXCL | O_CLOEXEC. A successful fd is transferred exactly once.
            let fd = unsafe { openx_np(path.as_ptr(), 1 | 0x200 | 0x800 | 0x01000000, security) };
            if fd < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(unsafe { File::from_raw_fd(fd) })
            }
        })();
        // SAFETY: free the owned filesec, which has copied its properties.
        unsafe { filesec_free(security) };
        result
    })();
    // SAFETY: release our owned ACL exactly once on every path.
    unsafe { acl_free(acl) };
    result
}

#[cfg(target_os = "macos")]
fn copy_acl(target: &File, output: &File) -> io::Result<()> {
    use std::{ffi::c_void, os::fd::AsRawFd};
    unsafe extern "C" {
        fn acl_get_fd(fd: i32) -> *mut c_void;
        fn acl_init(count: i32) -> *mut c_void;
        fn acl_set_fd(fd: i32, acl: *mut c_void) -> i32;
        fn acl_free(acl: *mut c_void) -> i32;
    }
    // SAFETY: a live fd; the returned extended ACL is owned by this function.
    let mut acl = unsafe { acl_get_fd(target.as_raw_fd()) };
    if acl.is_null() {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::NotFound {
            return Err(error);
        }
        // ENOENT on an existing fd means it has no extended ACL. An empty ACL
        // replaces any inherited entries on staging rather than retaining them.
        // SAFETY: zero entries requests a valid, owned empty ACL.
        acl = unsafe { acl_init(0) };
        if acl.is_null() {
            return Err(io::Error::last_os_error());
        }
    }
    // SAFETY: live fd and owned ACL; acl_set_fd does not consume it.
    let status = unsafe { acl_set_fd(output.as_raw_fd(), acl) };
    let error = (status != 0).then(io::Error::last_os_error);
    // SAFETY: release the owned ACL exactly once, including the failure path.
    unsafe { acl_free(acl) };
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "whispertube-export-acl-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn staging(&self) -> File {
            create_private(&self.0.join("staging")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[cfg(target_os = "linux")]
    fn acl(user_permissions: u16) -> Vec<u8> {
        let mut value = 2u32.to_le_bytes().to_vec(); // POSIX ACL xattr version.
        for (tag, permissions, id) in [
            (1u16, 6u16, u32::MAX),
            (2, user_permissions, 65534),
            (4, 4, u32::MAX),
            (16, 4, u32::MAX),
            (32, 4, u32::MAX),
        ] {
            value.extend(tag.to_le_bytes());
            value.extend(permissions.to_le_bytes());
            value.extend(id.to_le_bytes());
        }
        value
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn target_acl_and_identity_are_set_before_any_transcript_bytes() {
        let fixture = Fixture::new();
        let target = fixture.0.join("target.txt");
        fs::write(&target, b"old export").unwrap();
        let source = File::open(&target).unwrap();
        let expected = acl(0); // Named user denied even though others may read.
        linux::write(&source, linux::ACCESS, Some(&expected)).unwrap();
        let output = fixture.staging();
        preserve(&target, &output).unwrap();
        assert_eq!(output.metadata().unwrap().len(), 0);
        assert_eq!(linux::read(&output, linux::ACCESS).unwrap(), Some(expected));
        let before = source.metadata().unwrap();
        let after = output.metadata().unwrap();
        assert_eq!(
            (after.uid(), after.gid(), after.mode()),
            (before.uid(), before.gid(), before.mode())
        );
        assert_eq!(fs::read(target).unwrap(), b"old export");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn target_without_acl_does_not_keep_inherited_parent_grants() {
        let fixture = Fixture::new();
        let target = fixture.0.join("target.txt");
        fs::write(&target, b"old export").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        let parent = File::open(&fixture.0).unwrap();
        linux::write(&parent, c"system.posix_acl_default", Some(&acl(4))).unwrap();
        let output = fixture.staging();
        assert!(linux::read(&output, linux::ACCESS).unwrap().is_some());
        preserve(&target, &output).unwrap();
        assert_eq!(linux::read(&output, linux::ACCESS).unwrap(), None);
        assert_eq!(
            output.metadata().unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[cfg(target_os = "macos")]
    fn chmod(path: &Path, args: &[&str]) {
        let output = std::process::Command::new("/bin/chmod")
            .args(args)
            .arg(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(target_os = "macos")]
    fn acl_text(path: &Path) -> String {
        let output = std::process::Command::new("/bin/ls")
            .arg("-le")
            .arg(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn replacement_preserves_explicit_deny_before_writing() {
        let fixture = Fixture::new();
        let target = fixture.0.join("target.txt");
        fs::write(&target, b"old export").unwrap();
        chmod(&target, &["+a", "everyone deny read"]);
        let expected = acl_text(&target);
        assert!(expected.contains("deny read"));
        let output = fixture.staging();
        preserve(&target, &output).unwrap();
        assert_eq!(output.metadata().unwrap().len(), 0);
        assert_eq!(acl_text(&fixture.0.join("staging")), expected);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn replacement_removes_inherited_allow_when_target_has_no_acl() {
        let fixture = Fixture::new();
        let target = fixture.0.join("target.txt");
        fs::write(&target, b"old export").unwrap();
        chmod(&target, &["-N"]);
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        chmod(&fixture.0, &["+a", "everyone allow read,file_inherit"]);
        let output = fixture.staging();
        assert!(!acl_text(&fixture.0.join("staging")).contains("allow read"));
        preserve(&target, &output).unwrap();
        assert!(acl_text(&fixture.0.join("staging")).is_empty());
        assert_eq!(
            output.metadata().unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}
