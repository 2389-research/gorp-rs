#[test]
fn test_config_loads_from_env() {
    std::env::set_var("MATRIX_HOME_SERVER", "https://test.com");
    std::env::set_var("MATRIX_USER_ID", "@bot:test.com");
    std::env::set_var("MATRIX_ROOM_ID", "!room:test.com");
    std::env::set_var("MATRIX_PASSWORD", "secret");
    std::env::set_var("ALLOWED_USERS", "@user1:test.com,@user2:test.com");

    let config = matrix_bridge::config::Config::from_env().unwrap();

    assert_eq!(config.matrix_home_server, "https://test.com");
    assert_eq!(config.matrix_user_id, "@bot:test.com");
    assert_eq!(config.matrix_room_id, "!room:test.com");
    assert_eq!(config.matrix_password, Some("secret".to_string()));
    assert_eq!(config.allowed_users.len(), 2);
    assert!(config.allowed_users.contains("@user1:test.com"));
}

#[test]
fn test_config_fails_on_missing_required_field() {
    std::env::remove_var("MATRIX_HOME_SERVER");
    std::env::remove_var("MATRIX_USER_ID");

    let result = matrix_bridge::config::Config::from_env();

    assert!(result.is_err());
}
