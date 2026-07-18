use super::*;

fn directories() -> PlatformDirectories {
    PlatformDirectories {
        home: Some(PathBuf::from("/home/tester")),
        xdg_data_home: Some(PathBuf::from("/xdg/data")),
        xdg_config_home: Some(PathBuf::from("/xdg/config")),
        local_app_data: Some(PathBuf::from("C:/Users/tester/AppData/Local")),
        roaming_app_data: Some(PathBuf::from("C:/Users/tester/AppData/Roaming")),
    }
}

#[test]
fn parser_accepts_supported_string_forms_and_inline_comments() {
    let text = r#"
        ignored line
        windsurf_api_key = "devin-session-token$cli#part" # account
        api_server_url = 'https://server.codeium.test/'
    "#;
    assert_eq!(
        read_toml_string(text, "windsurf_api_key").as_deref(),
        Some("devin-session-token$cli#part")
    );
    assert_eq!(
        clean_api_server_url(read_toml_string(text, "api_server_url")).as_deref(),
        Some("https://server.codeium.test")
    );
    assert_eq!(
        read_toml_string(
            "windsurf_api_key = bare-value # comment",
            "windsurf_api_key"
        )
        .as_deref(),
        Some("bare-value")
    );
}

#[test]
fn parser_rejects_empty_unterminated_and_trailing_quoted_values() {
    for text in [
        "windsurf_api_key = \"\"",
        "windsurf_api_key = '   '",
        "windsurf_api_key = \"unterminated",
        "windsurf_api_key = \"valid\" trailing",
    ] {
        assert_eq!(read_toml_string(text, "windsurf_api_key"), None);
    }
    assert_eq!(
        clean_api_server_url(Some("http://server.codeium.test".into())),
        None
    );
}

#[test]
fn candidate_paths_cover_each_platform_in_deterministic_order() {
    let directories = directories();
    assert_eq!(
        credential_paths(HostPlatform::Macos, &directories),
        [
            PathBuf::from("/home/tester/.local/share/devin/credentials.toml"),
            PathBuf::from("/xdg/data/devin/credentials.toml"),
        ]
    );
    assert_eq!(
        credential_paths(HostPlatform::Windows, &directories),
        [
            PathBuf::from("C:/Users/tester/AppData/Local/devin/credentials.toml"),
            PathBuf::from("/home/tester/.local/share/devin/credentials.toml"),
        ]
    );
    assert_eq!(
        credential_paths(HostPlatform::Linux, &directories),
        [
            PathBuf::from("/xdg/data/devin/credentials.toml"),
            PathBuf::from("/home/tester/.local/share/devin/credentials.toml"),
        ]
    );

    assert_eq!(
        state_db_paths(HostPlatform::Macos, &directories),
        [PathBuf::from(
            "/home/tester/Library/Application Support/Devin/User/globalStorage/state.vscdb"
        )]
    );
    assert_eq!(
        state_db_paths(HostPlatform::Windows, &directories),
        [
            PathBuf::from("C:/Users/tester/AppData/Roaming/Devin/User/globalStorage/state.vscdb"),
            PathBuf::from("C:/Users/tester/AppData/Local/Devin/User/globalStorage/state.vscdb"),
        ]
    );
    assert_eq!(
        state_db_paths(HostPlatform::Linux, &directories),
        [
            PathBuf::from("/xdg/config/Devin/User/globalStorage/state.vscdb"),
            PathBuf::from("/home/tester/.config/Devin/User/globalStorage/state.vscdb"),
        ]
    );
}
