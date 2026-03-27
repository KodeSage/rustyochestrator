use rustyochestrator::config::ConnectConfig;

#[test]
fn test_connect_config_json_round_trip() {
    let config = ConnectConfig {
        dashboard_url: "https://example.com".to_string(),
        token: "my-token-123".to_string(),
        user_login: "testuser".to_string(),
    };
    let json = serde_json::to_string(&config).unwrap();
    let restored: ConnectConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.dashboard_url, "https://example.com");
    assert_eq!(restored.token, "my-token-123");
    assert_eq!(restored.user_login, "testuser");
}

#[test]
fn test_connect_config_pretty_json_contains_expected_keys() {
    let config = ConnectConfig {
        dashboard_url: "https://dashhy.vercel.app".to_string(),
        token: "eyJhbGciOiJIUzI1NiJ9".to_string(),
        user_login: "alice".to_string(),
    };
    let json = serde_json::to_string_pretty(&config).unwrap();
    assert!(json.contains("dashboard_url"));
    assert!(json.contains("dashhy.vercel.app"));
    assert!(json.contains("user_login"));
    assert!(json.contains("alice"));
    assert!(json.contains("token"));
}

#[test]
fn test_connect_config_clone_is_equal() {
    let config = ConnectConfig {
        dashboard_url: "https://example.com".to_string(),
        token: "tok".to_string(),
        user_login: "user".to_string(),
    };
    let cloned = config.clone();
    assert_eq!(cloned.dashboard_url, config.dashboard_url);
    assert_eq!(cloned.token, config.token);
    assert_eq!(cloned.user_login, config.user_login);
}

#[test]
fn test_connect_config_debug_format_includes_struct_name() {
    let config = ConnectConfig {
        dashboard_url: "https://example.com".to_string(),
        token: "tok".to_string(),
        user_login: "user".to_string(),
    };
    let debug = format!("{:?}", config);
    assert!(debug.contains("ConnectConfig"));
}

#[test]
fn test_connect_config_from_json_string() {
    let json = r#"{
        "dashboard_url": "https://ci.example.com",
        "token": "super-secret-token",
        "user_login": "devbot"
    }"#;
    let config: ConnectConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.dashboard_url, "https://ci.example.com");
    assert_eq!(config.token, "super-secret-token");
    assert_eq!(config.user_login, "devbot");
}

#[test]
fn test_connect_config_url_with_trailing_slash() {
    let config = ConnectConfig {
        dashboard_url: "https://example.com/".to_string(),
        token: "tok".to_string(),
        user_login: "user".to_string(),
    };
    let json = serde_json::to_string(&config).unwrap();
    let restored: ConnectConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.dashboard_url, "https://example.com/");
}
