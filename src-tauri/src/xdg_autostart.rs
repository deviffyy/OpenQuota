use std::{fs, path::PathBuf};

const DESKTOP_FILE: &str = "io.github.deviffyy.openquota.desktop";

#[cfg(target_os = "linux")]
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let path = autostart_path(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )?;
    let executable =
        std::env::current_exe().map_err(|_| "OpenQuota executable path could not be resolved.")?;
    set_enabled_at(&path, &executable, enabled)
}

fn set_enabled_at(
    path: &std::path::Path,
    executable: &std::path::Path,
    enabled: bool,
) -> Result<(), String> {
    if !enabled {
        if path.exists() {
            fs::remove_file(path).map_err(|_| "XDG autostart entry could not be removed.")?;
        }
        return Ok(());
    }

    let parent = path
        .parent()
        .ok_or("XDG autostart directory could not be resolved.")?;
    fs::create_dir_all(parent).map_err(|_| "XDG autostart directory could not be created.")?;
    let temporary = path.with_extension("desktop.tmp");
    fs::write(&temporary, desktop_entry(executable))
        .map_err(|_| "XDG autostart entry could not be written.")?;
    fs::rename(temporary, path)
        .map_err(|_| "XDG autostart entry could not be installed.".to_owned())
}

#[cfg(target_os = "linux")]
pub fn is_enabled() -> Result<bool, String> {
    Ok(autostart_path(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )?
    .is_file())
}

fn autostart_path(
    xdg_config_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let config = xdg_config_home
        .filter(|path| path.is_absolute() || path.to_string_lossy().starts_with('/'))
        .or_else(|| home.map(|path| path.join(".config")))
        .ok_or("XDG configuration directory could not be resolved.")?;
    Ok(config.join("autostart").join(DESKTOP_FILE))
}

fn desktop_entry(executable: &std::path::Path) -> String {
    format!(
        "[Desktop Entry]\n\
Type=Application\n\
Version=1.0\n\
Name=OpenQuota\n\
Comment=Track local AI provider quotas\n\
Exec={}\n\
Icon=io.github.deviffyy.openquota\n\
Terminal=false\n\
StartupNotify=false\n\
X-GNOME-Autostart-enabled=true\n",
        quote_exec(executable.to_string_lossy().as_ref())
    )
}

fn quote_exec(value: &str) -> String {
    let escaped = value
        .chars()
        .flat_map(|character| {
            if matches!(character, '\\' | '"' | '`' | '$') {
                vec!['\\', character]
            } else {
                vec![character]
            }
        })
        .collect::<String>();
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use super::{autostart_path, desktop_entry, set_enabled_at};

    #[test]
    fn honors_absolute_xdg_config_home_and_falls_back_to_home() {
        assert_eq!(
            autostart_path(Some(PathBuf::from("/custom/config")), None).unwrap(),
            PathBuf::from("/custom/config/autostart/io.github.deviffyy.openquota.desktop")
        );
        assert_eq!(
            autostart_path(
                Some(PathBuf::from("relative")),
                Some(PathBuf::from("/home/user"))
            )
            .unwrap(),
            PathBuf::from("/home/user/.config/autostart/io.github.deviffyy.openquota.desktop")
        );
    }

    #[test]
    fn desktop_entry_quotes_reserved_exec_characters() {
        let entry = desktop_entry(Path::new("/opt/Open Quota/$test`bin"));
        assert!(entry.contains("Exec=\"/opt/Open Quota/\\$test\\`bin\""));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
    }

    #[test]
    fn installs_and_removes_an_xdg_desktop_entry() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("autostart/openquota.desktop");
        set_enabled_at(&path, Path::new("/opt/Open Quota/openquota"), true).unwrap();
        let entry = std::fs::read_to_string(&path).unwrap();
        assert!(entry.contains("Exec=\"/opt/Open Quota/openquota\""));
        set_enabled_at(&path, Path::new("/unused"), false).unwrap();
        assert!(!path.exists());
    }
}
