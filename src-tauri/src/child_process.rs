use std::process::Command;

/// Creates a child process intended for background discovery work.
///
/// A GUI-subsystem application does not own a console on Windows. Starting a
/// console-subsystem executable from it without `CREATE_NO_WINDOW` briefly
/// creates a visible console window, so every background child process must go
/// through this helper.
pub fn background_command(program: &str) -> Command {
    let mut command = Command::new(program);
    hide_console_window(&mut command);
    command
}

#[cfg(target_os = "windows")]
fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_console_window(_command: &mut Command) {}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::background_command;

    #[test]
    fn background_console_process_has_no_console_window() {
        let script = r#"Add-Type -Name NativeMethods -Namespace OpenQuota -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();'; [OpenQuota.NativeMethods]::GetConsoleWindow().ToInt64()"#;
        let output = background_command("powershell")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                script,
            ])
            .output()
            .expect("PowerShell should be available on supported Windows installations");

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
    }
}
