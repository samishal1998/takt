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
