use std::{path::PathBuf, process::Command};

fn main() {
    let commit = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=ZHFD_GIT_COMMIT={commit}");
    for git_path in ["HEAD", "refs/heads"] {
        if let Some(path) = git_path_from_repo(git_path) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

fn git_path_from_repo(path: &str) -> Option<PathBuf> {
    Command::new("git")
        .args(["rev-parse", "--git-path", path])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
        .filter(|path| !path.as_os_str().is_empty())
}
