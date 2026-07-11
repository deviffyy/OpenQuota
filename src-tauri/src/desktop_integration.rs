#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxSessionType {
    X11,
    Wayland,
    Unknown,
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxDesktop {
    Gnome,
    Kde,
    Other,
}

#[derive(Debug, Clone)]
pub struct DesktopIntegration {
    pub standalone_window: bool,
    platform_summary: Option<String>,
}

impl DesktopIntegration {
    pub fn detect() -> Self {
        #[cfg(target_os = "linux")]
        {
            let session = parse_session_type(std::env::var("XDG_SESSION_TYPE").ok().as_deref());
            let desktop = parse_desktop(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref());
            let tray_available = status_notifier_host_available();
            linux_integration(session, desktop, tray_available)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self {
                standalone_window: false,
                platform_summary: None,
            }
        }
    }

    pub fn platform_summary(&self) -> Option<String> {
        self.platform_summary.clone()
    }
}

#[cfg(any(target_os = "linux", test))]
fn linux_integration(
    session: LinuxSessionType,
    desktop: LinuxDesktop,
    tray_available: bool,
) -> DesktopIntegration {
    let desktop = match desktop {
        LinuxDesktop::Gnome => "GNOME",
        LinuxDesktop::Kde => "KDE Plasma",
        LinuxDesktop::Other => "Linux desktop",
    };
    let session = match session {
        LinuxSessionType::X11 => "X11",
        LinuxSessionType::Wayland => "Wayland",
        LinuxSessionType::Unknown => "unknown session",
    };
    let mode = if tray_available {
        "StatusNotifier tray"
    } else {
        "standalone window"
    };
    DesktopIntegration {
        standalone_window: !tray_available,
        platform_summary: Some(format!("{desktop} · {session} · {mode}")),
    }
}

#[cfg(any(target_os = "linux", test))]
pub fn parse_desktop(value: Option<&str>) -> LinuxDesktop {
    let names = value
        .unwrap_or_default()
        .split(':')
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if names.iter().any(|name| name.contains("gnome")) {
        LinuxDesktop::Gnome
    } else if names
        .iter()
        .any(|name| name.contains("kde") || name.contains("plasma"))
    {
        LinuxDesktop::Kde
    } else {
        LinuxDesktop::Other
    }
}

#[cfg(any(target_os = "linux", test))]
pub fn parse_session_type(value: Option<&str>) -> LinuxSessionType {
    match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("x11") => LinuxSessionType::X11,
        Some("wayland") => LinuxSessionType::Wayland,
        _ => LinuxSessionType::Unknown,
    }
}

#[cfg(target_os = "linux")]
fn status_notifier_host_available() -> bool {
    match std::env::var("OPENQUOTA_LINUX_TRAY_HOST").as_deref() {
        Ok("available") => return true,
        Ok("unavailable") => return false,
        _ => {}
    }
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return false;
    };
    let Ok(proxy) = zbus::blocking::fdo::DBusProxy::new(&connection) else {
        return false;
    };
    proxy.list_names().is_ok_and(|names| {
        names
            .iter()
            .any(|name| name.as_str() == "org.kde.StatusNotifierWatcher")
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_desktop, parse_session_type, LinuxDesktop, LinuxSessionType};

    #[test]
    fn recognizes_x11_and_wayland_sessions_case_insensitively() {
        assert_eq!(parse_session_type(Some("x11")), LinuxSessionType::X11);
        assert_eq!(
            parse_session_type(Some(" Wayland ")),
            LinuxSessionType::Wayland
        );
        assert_eq!(parse_session_type(Some("tty")), LinuxSessionType::Unknown);
        assert_eq!(parse_session_type(None), LinuxSessionType::Unknown);
    }

    #[test]
    fn recognizes_gnome_and_kde_desktop_name_lists() {
        assert_eq!(parse_desktop(Some("ubuntu:GNOME")), LinuxDesktop::Gnome);
        assert_eq!(parse_desktop(Some("KDE")), LinuxDesktop::Kde);
        assert_eq!(parse_desktop(Some("plasma:wayland")), LinuxDesktop::Kde);
        assert_eq!(parse_desktop(Some("sway")), LinuxDesktop::Other);
    }

    #[test]
    fn platform_summary_explains_the_linux_fallback_mode() {
        let integration =
            super::linux_integration(LinuxSessionType::Wayland, LinuxDesktop::Gnome, false);
        assert_eq!(
            integration.platform_summary().as_deref(),
            Some("GNOME · Wayland · standalone window")
        );
    }
}
