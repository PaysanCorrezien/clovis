#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn main() -> io::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let exe = std::env::current_exe()?;
    let install_dir = exe
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    if args.is_empty() || args.first().is_some_and(|arg| arg == "gui") {
        let mut command = Command::new(install_dir.join("clovis-cli.exe"));
        command
            .arg("gui")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_windows_console(&mut command);
        command.spawn()?;
        return Ok(());
    }

    let mut command = Command::new(install_dir.join("clovis-cli.exe"));
    command.args(args);
    command.spawn()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn hide_windows_console(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_windows_console(_command: &mut Command) {}
