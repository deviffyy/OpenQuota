use crate::child_process::background_command;

#[derive(Debug, Clone)]
pub struct LanguageServer {
    pub csrf: String,
    pub ports: Vec<u16>,
    pub extension_port: Option<u16>,
}

pub fn discover() -> Option<LanguageServer> {
    #[cfg(target_os = "windows")]
    {
        discover_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        discover_unix()
    }
}

#[cfg(target_os = "windows")]
fn discover_windows() -> Option<LanguageServer> {
    let script = r#"$items=Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'language_server|agy' -and $_.CommandLine -match 'antigravity' } | ForEach-Object { [pscustomobject]@{ command=$_.CommandLine; ports=@(Get-NetTCPConnection -OwningProcess $_.ProcessId -State Listen -ErrorAction SilentlyContinue | Select-Object -ExpandProperty LocalPort -Unique) } }; @($items) | ConvertTo-Json -Compress -Depth 4"#;
    let output = background_command("powershell")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let values: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).ok()?;
    values.into_iter().find_map(|value| {
        let command = value.get("command")?.as_str()?;
        let csrf = extract_flag(command, "--csrf_token")?;
        let extension_port =
            extract_flag(command, "--extension_server_port").and_then(|value| value.parse().ok());
        let mut ports = value
            .get("ports")?
            .as_array()?
            .iter()
            .filter_map(|port| port.as_u64().and_then(|port| u16::try_from(port).ok()))
            .collect::<Vec<_>>();
        ports.sort_unstable();
        ports.dedup();
        (!ports.is_empty() || extension_port.is_some()).then_some(LanguageServer {
            csrf,
            ports,
            extension_port,
        })
    })
}

#[cfg(not(target_os = "windows"))]
fn discover_unix() -> Option<LanguageServer> {
    let output = background_command("ps")
        .args(["-ax", "-o", "pid=,command="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    for line in text.lines().filter(|line| {
        (line.contains("language_server") || line.contains("/agy"))
            && line.to_ascii_lowercase().contains("antigravity")
    }) {
        let trimmed = line.trim();
        let split = trimmed.find(char::is_whitespace)?;
        let pid = &trimmed[..split];
        let command = trimmed[split..].trim();
        let csrf = extract_flag(command, "--csrf_token").unwrap_or_default();
        let extension_port =
            extract_flag(command, "--extension_server_port").and_then(|value| value.parse().ok());
        let ports = background_command("lsof")
            .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-a", "-p", pid])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|output| parse_lsof_ports(&output))
            .unwrap_or_default();
        if !ports.is_empty() || extension_port.is_some() {
            return Some(LanguageServer {
                csrf,
                ports,
                extension_port,
            });
        }
    }
    None
}

fn extract_flag(command: &str, flag: &str) -> Option<String> {
    let parts = command.split_whitespace().collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        if *part == flag {
            return parts.get(index + 1).map(|value| (*value).to_owned());
        }
        if let Some(value) = part.strip_prefix(&format!("{flag}=")) {
            return Some(value.to_owned());
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn parse_lsof_ports(output: &str) -> Vec<u16> {
    let mut ports = output
        .lines()
        .filter(|line| line.contains("LISTEN"))
        .filter_map(|line| {
            line.split_whitespace().rev().find_map(|part| {
                let value = part.rsplit_once(':')?.1.trim_end_matches("(LISTEN)");
                value.parse().ok()
            })
        })
        .collect::<Vec<_>>();
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[cfg(test)]
mod tests {
    use super::extract_flag;

    #[test]
    fn extracts_both_flag_forms() {
        assert_eq!(
            extract_flag("server --csrf_token secret", "--csrf_token").as_deref(),
            Some("secret")
        );
        assert_eq!(
            extract_flag(
                "server --extension_server_port=1234",
                "--extension_server_port"
            )
            .as_deref(),
            Some("1234")
        );
    }
}
