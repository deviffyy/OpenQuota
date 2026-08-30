#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    if let Some(exit_code) = openquota_lib::run_cli_if_requested() {
        return std::process::ExitCode::from(exit_code as u8);
    }
    openquota_lib::run();
    std::process::ExitCode::SUCCESS
}
