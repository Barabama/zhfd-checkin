use std::{ffi::OsStr, process::Command};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;

pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

pub fn detached_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
    command
}

pub fn open_path(path: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        command("explorer.exe").arg(path).spawn().map_err(|error| {
            anyhow::anyhow!("无法在资源管理器中打开 {}: {error}", path.display())
        })?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        anyhow::bail!("打开文件/目录当前仅支持 Windows")
    }
}
