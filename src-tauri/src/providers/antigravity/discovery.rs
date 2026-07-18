#[cfg(any(test, target_os = "linux"))]
use std::collections::HashSet;
use std::time::Duration;

use crate::child_process::{background_command, output_with_timeout};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct LanguageServer {
    pub csrf: String,
    pub ports: Vec<u16>,
    pub extension_port: Option<u16>,
}

#[derive(Clone, Copy)]
struct DiscoveryOptions {
    process_name: &'static str,
    markers: &'static [&'static str],
    csrf_flag: Option<&'static str>,
    port_flag: Option<&'static str>,
}

const DISCOVERY_OPTIONS: [DiscoveryOptions; 2] = [
    DiscoveryOptions {
        process_name: "language_server",
        markers: &["antigravity", "antigravity-ide"],
        csrf_flag: Some("--csrf_token"),
        port_flag: Some("--extension_server_port"),
    },
    DiscoveryOptions {
        process_name: "agy",
        markers: &[],
        csrf_flag: None,
        port_flag: None,
    },
];

#[cfg(any(test, not(target_os = "windows")))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessCandidate {
    pid: String,
    command: String,
    rank: u8,
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
    let script = r#"
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$netstat = @(& "$env:SystemRoot\System32\netstat.exe" -ano -p TCP 2>$null)
$items = Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'language_server|^agy(\.exe)?$' } | ForEach-Object {
    $pidValue = $_.ProcessId
    $ports = @(Get-NetTCPConnection -OwningProcess $pidValue -State Listen -ErrorAction SilentlyContinue | Select-Object -ExpandProperty LocalPort -Unique)
    if ($ports.Count -eq 0) {
        $ports = @($netstat | ForEach-Object {
            $fields = @($_ -split '\s+' | Where-Object { $_ })
            if ($fields.Count -ge 5 -and $fields[3] -eq 'LISTENING' -and $fields[4] -eq "$pidValue" -and $fields[1] -match ':(\d+)$') {
                [int]$Matches[1]
            }
        })
    }
    [pscustomobject]@{ command=$_.CommandLine; ports=@($ports) }
}
ConvertTo-Json -InputObject @($items) -Compress -Depth 4
"#;
    let output = output_with_timeout(
        background_command("powershell").args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ]),
        DISCOVERY_TIMEOUT,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let values = parse_windows_processes(&output.stdout)?;
    language_server_from_processes(values)
}

#[cfg(any(test, target_os = "windows"))]
fn parse_windows_processes(bytes: &[u8]) -> Option<Vec<serde_json::Value>> {
    match serde_json::from_slice(bytes).ok()? {
        serde_json::Value::Array(values) => Some(values),
        value @ serde_json::Value::Object(_) => Some(vec![value]),
        serde_json::Value::Null => Some(Vec::new()),
        _ => None,
    }
}

#[cfg(any(test, target_os = "windows"))]
fn language_server_from_processes(values: Vec<serde_json::Value>) -> Option<LanguageServer> {
    for options in DISCOVERY_OPTIONS {
        let mut candidates = values
            .iter()
            .filter_map(|value| {
                let command = value.get("command")?.as_str()?;
                let rank = marker_rank(command, options)?;
                let mut ports = value
                    .get("ports")?
                    .as_array()?
                    .iter()
                    .filter_map(|port| port.as_u64().and_then(|port| u16::try_from(port).ok()))
                    .filter(|port| *port > 0)
                    .collect::<Vec<_>>();
                ports.sort_unstable();
                ports.dedup();
                Some((rank, command, ports))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(rank, _, _)| *rank);
        for (_, command, ports) in candidates {
            if let Some(server) = server_from_command(command, ports, options) {
                return Some(server);
            }
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn discover_unix() -> Option<LanguageServer> {
    #[cfg(target_os = "macos")]
    let ps_program = "/bin/ps";
    #[cfg(not(target_os = "macos"))]
    let ps_program = "ps";
    let output = output_with_timeout(
        background_command(ps_program).args(["-ax", "-o", "pid=,command="]),
        DISCOVERY_TIMEOUT,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    for options in DISCOVERY_OPTIONS {
        for candidate in ranked_candidates(&text, options) {
            let ports = listening_ports(&candidate.pid);
            if let Some(server) = server_from_command(&candidate.command, ports, options) {
                return Some(server);
            }
        }
    }
    None
}

#[cfg(any(test, not(target_os = "windows")))]
fn ranked_candidates(output: &str, options: DiscoveryOptions) -> Vec<ProcessCandidate> {
    let mut candidates = output
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let split = trimmed.find(char::is_whitespace)?;
            let pid = trimmed[..split].parse::<u32>().ok()?.to_string();
            let command = trimmed[split..].trim();
            let rank = marker_rank(command, options)?;
            Some(ProcessCandidate {
                pid,
                command: command.to_owned(),
                rank,
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| candidate.rank);
    candidates
}

fn server_from_command(
    command: &str,
    mut ports: Vec<u16>,
    options: DiscoveryOptions,
) -> Option<LanguageServer> {
    let csrf = options
        .csrf_flag
        .map(|flag| extract_flag(command, flag))
        .unwrap_or_else(|| Some(String::new()))?;
    let extension_port = options
        .port_flag
        .and_then(|flag| extract_flag(command, flag))
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port > 0);
    ports.sort_unstable();
    ports.dedup();
    (!ports.is_empty() || extension_port.is_some()).then_some(LanguageServer {
        csrf,
        ports,
        extension_port,
    })
}

fn marker_rank(command: &str, options: DiscoveryOptions) -> Option<u8> {
    if !command_matches_process(command, options.process_name) {
        return None;
    }
    if options.markers.is_empty() {
        return Some(0);
    }
    let ide_name = extract_flag(command, "--ide_name").map(|value| value.to_ascii_lowercase());
    let override_ide_name =
        extract_flag(command, "--override_ide_name").map(|value| value.to_ascii_lowercase());
    let app_data = extract_flag(command, "--app_data_dir").map(|value| value.to_ascii_lowercase());
    if ide_name.is_some() || override_ide_name.is_some() || app_data.is_some() {
        return options
            .markers
            .iter()
            .any(|marker| {
                [
                    ide_name.as_deref(),
                    override_ide_name.as_deref(),
                    app_data.as_deref(),
                ]
                .into_iter()
                .flatten()
                .any(|value| value == *marker)
            })
            .then_some(0);
    }
    let normalized = command.replace('\\', "/").to_ascii_lowercase();
    options
        .markers
        .iter()
        .any(|marker| normalized.contains(&format!("/{marker}/")))
        .then_some(1)
}

fn argv0(command: &str) -> &str {
    let trimmed = command.trim_start();
    let Some(quote @ ('\'' | '"')) = trimmed.chars().next() else {
        return trimmed.split_whitespace().next().unwrap_or_default();
    };
    let rest = &trimmed[quote.len_utf8()..];
    rest.find(quote)
        .map(|end| &rest[..end])
        .unwrap_or_else(|| trimmed.split_whitespace().next().unwrap_or_default())
}

fn command_matches_process(command: &str, process_name: &str) -> bool {
    if process_name.is_empty() {
        return false;
    }
    let executable = argv0(command)
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let executable = executable.strip_suffix(".exe").unwrap_or(&executable);
    let process_name = process_name.to_ascii_lowercase();
    if executable == process_name {
        return true;
    }
    let command = command.to_ascii_lowercase();
    if process_name.len() >= 8 {
        executable.starts_with(&format!("{process_name}_")) || command.contains(&process_name)
    } else {
        let normalized = command.replace('\\', "/");
        normalized.ends_with(&format!("/{process_name}"))
            || normalized.contains(&format!("/{process_name} "))
            || normalized.contains(&format!("/{process_name}\t"))
    }
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

#[cfg(target_os = "linux")]
fn listening_ports(pid: &str) -> Vec<u16> {
    let ports = proc_listening_ports(pid);
    if ports.is_empty() {
        lsof_ports(pid)
    } else {
        ports
    }
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
fn listening_ports(pid: &str) -> Vec<u16> {
    lsof_ports(pid)
}

#[cfg(not(target_os = "windows"))]
fn lsof_ports(pid: &str) -> Vec<u16> {
    #[cfg(target_os = "macos")]
    let programs = ["/usr/sbin/lsof", "/usr/bin/lsof", "lsof"];
    #[cfg(not(target_os = "macos"))]
    let programs = ["/usr/bin/lsof", "/usr/sbin/lsof", "lsof"];
    for program in programs {
        let Some(output) = output_with_timeout(
            background_command(program).args(["-nP", "-iTCP", "-sTCP:LISTEN", "-a", "-p", pid]),
            DISCOVERY_TIMEOUT,
        )
        .ok()
        .filter(|output| output.status.success()) else {
            continue;
        };
        let Some(output) = String::from_utf8(output.stdout).ok() else {
            continue;
        };
        return parse_lsof_ports(&output);
    }
    Vec::new()
}

#[cfg(target_os = "linux")]
fn proc_listening_ports(pid: &str) -> Vec<u16> {
    if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return Vec::new();
    };
    let socket_ids = entries
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .filter_map(|target| {
            let target = target.to_string_lossy();
            target
                .strip_prefix("socket:[")
                .and_then(|value| value.strip_suffix(']'))
                .map(str::to_owned)
        })
        .collect::<HashSet<_>>();
    let mut ports = ["tcp", "tcp6"]
        .into_iter()
        .filter_map(|table| std::fs::read_to_string(format!("/proc/{pid}/net/{table}")).ok())
        .flat_map(|table| parse_proc_net_ports(&table, &socket_ids))
        .collect::<Vec<_>>();
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[cfg(any(test, target_os = "linux"))]
fn parse_proc_net_ports(output: &str, socket_ids: &HashSet<String>) -> Vec<u16> {
    let mut ports = output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.get(3).copied() != Some("0A")
                || !fields
                    .get(9)
                    .is_some_and(|inode| socket_ids.contains(*inode))
            {
                return None;
            }
            let port = fields.get(1)?.rsplit_once(':')?.1;
            u16::from_str_radix(port, 16).ok().filter(|port| *port > 0)
        })
        .collect::<Vec<_>>();
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[cfg(any(test, not(target_os = "windows")))]
fn parse_lsof_ports(output: &str) -> Vec<u16> {
    let mut ports = output
        .lines()
        .filter(|line| line.contains("LISTEN"))
        .filter_map(|line| {
            line.split_whitespace().rev().find_map(|part| {
                let value = part.rsplit_once(':')?.1.trim_end_matches("(LISTEN)");
                value.parse().ok().filter(|port| *port > 0)
            })
        })
        .collect::<Vec<_>>();
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        command_matches_process, extract_flag, language_server_from_processes, marker_rank,
        parse_lsof_ports, parse_proc_net_ports, parse_windows_processes, ranked_candidates,
        DiscoveryOptions, LanguageServer, DISCOVERY_OPTIONS,
    };

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

    #[test]
    fn windows_process_json_accepts_single_objects_and_arrays() {
        let single = br#"{
            "command": "C:\\Antigravity\\language_server.exe --override_ide_name antigravity --csrf_token token --extension_server_port=4567",
            "ports": [1234]
        }"#;
        let values = parse_windows_processes(single).unwrap();
        assert_eq!(values.len(), 1);
        assert_language_server(
            language_server_from_processes(values).unwrap(),
            "token",
            &[1234],
            Some(4567),
        );

        let multiple = br#"[
            {"command":"unrelated","ports":[]},
            {"command":"language_server --app_data_dir antigravity --csrf_token token","ports":[8765]}
        ]"#;
        let values = parse_windows_processes(multiple).unwrap();
        assert_eq!(values.len(), 2);
        assert_language_server(
            language_server_from_processes(values).unwrap(),
            "token",
            &[8765],
            None,
        );
    }

    #[test]
    fn agy_is_discovered_without_an_antigravity_marker() {
        let values = parse_windows_processes(
            br#"[{"command":"/Users/tester/.local/bin/agy","ports":[52168]}]"#,
        )
        .unwrap();

        assert_language_server(
            language_server_from_processes(values).unwrap(),
            "",
            &[52168],
            None,
        );
    }

    #[test]
    fn process_and_marker_matching_rejects_neighboring_products() {
        assert!(command_matches_process(
            "/Applications/Antigravity.app/bin/language_server --standalone",
            "language_server"
        ));
        assert!(command_matches_process(
            "/Users/tester/.local/bin/agy",
            "agy"
        ));
        assert!(!command_matches_process(
            "/Applications/Antigravity.app/bin/language_server",
            "agy"
        ));

        let language_server = DISCOVERY_OPTIONS[0];
        assert_eq!(
            marker_rank(
                "language_server --override_ide_name antigravity",
                language_server
            ),
            Some(0)
        );
        assert_eq!(
            marker_rank(
                "language_server --app_data_dir antigravity-next",
                language_server
            ),
            None
        );
        assert_eq!(
            marker_rank("/opt/antigravity/language_server", language_server),
            Some(1)
        );
    }

    #[test]
    fn unix_candidates_are_ranked_and_malformed_lines_do_not_abort_the_scan() {
        let output = r#"
            malformed
            10 /opt/antigravity/language_server --csrf_token path
            11 /opt/bin/language_server --override_ide_name antigravity --csrf_token exact
        "#;
        let candidates = ranked_candidates(output, DISCOVERY_OPTIONS[0]);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].pid, "11");
        assert_eq!(candidates[1].pid, "10");
    }

    #[test]
    fn parses_lsof_and_linux_proc_listening_ports() {
        let lsof = "language_ 4276 user 6u IPv4 0x0 0t0 TCP 127.0.0.1:52168 (LISTEN)\n\
                    language_ 4276 user 7u IPv6 0x0 0t0 TCP *:52169 (LISTEN)\n";
        assert_eq!(parse_lsof_ports(lsof), [52168, 52169]);

        let proc = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
                    0: 0100007F:CBD8 00000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 12345\n\
                    1: 0100007F:CBD9 00000000:0000 01 00000000:00000000 00:00000000 00000000 1000 0 12346\n\
                    2: 0100007F:CBDA 00000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 99999\n";
        assert_eq!(
            parse_proc_net_ports(proc, &HashSet::from(["12345".to_owned()])),
            [52184]
        );
    }

    #[test]
    fn empty_marker_strategy_accepts_only_the_requested_process() {
        let options = DiscoveryOptions {
            process_name: "agy",
            markers: &[],
            csrf_flag: None,
            port_flag: None,
        };
        assert_eq!(marker_rank("/usr/local/bin/agy", options), Some(0));
        assert_eq!(marker_rank("/usr/local/bin/agy-helper", options), None);
    }

    fn assert_language_server(
        server: LanguageServer,
        csrf: &str,
        ports: &[u16],
        extension_port: Option<u16>,
    ) {
        assert_eq!(server.csrf, csrf);
        assert_eq!(server.ports, ports);
        assert_eq!(server.extension_port, extension_port);
    }
}
