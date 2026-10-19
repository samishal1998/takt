//! Development-only process fixture discovery. No fixture code ships in Takt.
use std::path::PathBuf;

pub fn executable() -> PathBuf {
    if let Some(path) = std::env::var_os("TAKT_TEST_FIXTURE") {
        return PathBuf::from(path);
    }
    let test = std::env::current_exe().expect("locate integration executable");
    let directory = test.parent().expect("test executable directory");
    let directory = if directory.file_name().is_some_and(|name| name == "deps") {
        directory.parent().expect("target profile directory")
    } else {
        directory
    };
    let path = directory.join("takt-test-fixture");
    assert!(
        path.is_file(),
        "fixture missing at {}; run cargo build -p takt-test-fixture first, or set TAKT_TEST_FIXTURE",
        path.display()
    );
    path
}

/// Check process liveness independently of Takt's process enumeration. An exited
/// zombie has already released its memory and cannot perform further work.
pub fn assert_process_stopped(pid: u32) {
    #[cfg(target_os = "linux")]
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => {
            let state = stat
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .next()
                .unwrap();
            assert!(
                matches!(state, "Z" | "X"),
                "owned descendant {pid} still alive: {stat}"
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("read process {pid}: {error}"),
    }
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: the output buffer has the requested size and is read only after
        // libproc confirms that it initialized a complete proc_bsdinfo.
        let read = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if read == 0 {
            let error = std::io::Error::last_os_error();
            assert_eq!(
                error.raw_os_error(),
                Some(libc::ESRCH),
                "read process {pid}: {error}"
            );
        } else {
            assert_eq!(read, size, "incomplete process information for {pid}");
            // SAFETY: the successful call above initialized the complete value.
            let info = unsafe { info.assume_init() };
            assert_eq!(
                info.pbi_status,
                libc::SZOMB,
                "owned descendant {pid} still alive"
            );
        }
    }
}
