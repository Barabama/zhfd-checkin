use std::{path::PathBuf, process::Command};

fn main() {
    // Provenance must describe the tree the binary was actually built from. A
    // bare `rev-parse HEAD` is a lie whenever tracked files are modified, so
    // the dirty state is captured separately and reported as its own field
    // (FR-014). "unknown" is only used when git itself is unavailable -- never
    // as a stand-in for "clean".
    let commit = git(&["rev-parse", "--short=12", "HEAD"])
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=ZHFD_GIT_COMMIT={commit}");

    if commit != "unknown" {
        // Include untracked files too. A newly added file can be pulled into
        // the binary by `include_bytes!`/`#[path]`; calling that tree clean
        // would make the provenance claim false. Release checkouts are clean,
        // while local scratch files correctly make local builds `+dirty`.
        let status = git(&["--no-optional-locks", "status", "--porcelain"])
            .or_else(|| git(&["status", "--porcelain"]));
        if let Some(status) = status {
            let dirty = if status.trim().is_empty() { "0" } else { "1" };
            println!("cargo:rustc-env=ZHFD_GIT_DIRTY={dirty}");
        }
        // A failed `git status` leaves ZHFD_GIT_DIRTY unset, which the binary
        // reports as `git_dirty: null` rather than as a clean tree.
    }

    // `src` plus the repository-level inputs that are embedded into the
    // binaries. Declaring these explicitly keeps the dirty stamp fresh when an
    // included fixture/font changes (Cargo's default build-script trigger is
    // disabled once any rerun rule is present).
    for input in [
        "src",
        "assets",
        "Cargo.toml",
        "Cargo.lock",
        "../autojs6/fixtures",
    ] {
        println!("cargo:rerun-if-changed={input}");
    }
    for git_path in ["HEAD", "refs/heads", "index"] {
        if let Some(path) = git_path_from_repo(git_path) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_path_from_repo(path: &str) -> Option<PathBuf> {
    git(&["rev-parse", "--git-path", path])
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}
