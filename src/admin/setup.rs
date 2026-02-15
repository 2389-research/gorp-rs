// ABOUTME: Setup wizard routes for first-run configuration
// ABOUTME: 3-step flow: create admin account, display API token, show platform status

use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use serde::Deserialize;

use super::auth::AuthConfig;
use super::routes::AdminState;
use super::templates::{LoginTemplate, SetupStep1Template, SetupStep2Template, SetupStep3Template};

/// Build the setup wizard router mounted at /setup
pub fn setup_router() -> Router<AdminState> {
    Router::new()
        .route("/", get(setup_step1_view))
        .route("/step1", post(setup_step1_submit))
        .route("/step2", get(setup_step2_view))
        .route("/step3", get(setup_step3_view))
        .route("/finish", post(setup_finish))
}

/// Build the login router for /login
pub fn login_router() -> Router<AdminState> {
    Router::new()
        .route("/", get(login_view).post(login_submit))
}

// =============================================================================
// Setup Step 1: Create Admin Account
// =============================================================================

async fn setup_step1_view(State(state): State<AdminState>) -> Response {
    // If setup is already complete, redirect to admin
    if let Some(ref auth) = state.auth_config {
        if auth.setup_complete {
            return Redirect::temporary("/admin").into_response();
        }
    }

    SetupStep1Template {
        error_message: None,
        prefill_username: String::new(),
    }
    .into_response()
}

#[derive(Deserialize)]
struct SetupStep1Form {
    username: String,
    password: String,
    confirm_password: String,
}

async fn setup_step1_submit(
    State(state): State<AdminState>,
    Form(form): Form<SetupStep1Form>,
) -> Response {
    // If setup is already complete, redirect to admin
    if let Some(ref auth) = state.auth_config {
        if auth.setup_complete {
            return Redirect::temporary("/admin").into_response();
        }
    }

    let username = form.username.trim();
    let password = form.password.trim();
    let confirm = form.confirm_password.trim();

    // Validate username
    if username.len() < 3 || username.len() > 32 {
        return SetupStep1Template {
            error_message: Some("Username must be 3-32 characters".to_string()),
            prefill_username: username.to_string(),
        }
        .into_response();
    }

    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return SetupStep1Template {
            error_message: Some(
                "Username must contain only letters, numbers, hyphens, and underscores".to_string(),
            ),
            prefill_username: username.to_string(),
        }
        .into_response();
    }

    // Validate password
    if password.len() < 8 {
        return SetupStep1Template {
            error_message: Some("Password must be at least 8 characters".to_string()),
            prefill_username: username.to_string(),
        }
        .into_response();
    }

    if password != confirm {
        return SetupStep1Template {
            error_message: Some("Passwords do not match".to_string()),
            prefill_username: username.to_string(),
        }
        .into_response();
    }

    // Create auth config
    let auth_config = match AuthConfig::create(username, password) {
        Ok(config) => config,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create auth config during setup");
            return SetupStep1Template {
                error_message: Some("Internal error creating account. Check logs.".to_string()),
                prefill_username: username.to_string(),
            }
            .into_response();
        }
    };

    // Save to disk
    let data_dir = crate::paths::data_dir();
    let data_dir_str = data_dir.to_string_lossy();
    if let Err(e) = auth_config.save(&data_dir_str) {
        tracing::error!(error = %e, "Failed to save auth config during setup");
        return SetupStep1Template {
            error_message: Some("Failed to save configuration. Check logs.".to_string()),
            prefill_username: username.to_string(),
        }
        .into_response();
    }

    tracing::info!(username = username, "Admin account created during setup");

    // Redirect to step 2 to show the API token
    Redirect::to("/setup/step2").into_response()
}

// =============================================================================
// Setup Step 2: Display API Token
// =============================================================================

async fn setup_step2_view(State(_state): State<AdminState>) -> Response {
    // Load the auth config we just saved to get the token
    let data_dir = crate::paths::data_dir();
    let data_dir_str = data_dir.to_string_lossy();

    match AuthConfig::load(&data_dir_str) {
        Ok(Some(config)) => SetupStep2Template {
            api_token: config.api_token,
        }
        .into_response(),
        _ => {
            // Shouldn't happen — redirect back to step 1
            Redirect::temporary("/setup").into_response()
        }
    }
}

// =============================================================================
// Setup Step 3: Platform Status
// =============================================================================

async fn setup_step3_view(State(state): State<AdminState>) -> Response {
    let matrix_configured = state.config.matrix.is_some();
    let telegram_configured = state.config.telegram.is_some();

    SetupStep3Template {
        matrix_configured,
        telegram_configured,
    }
    .into_response()
}

// =============================================================================
// Setup Finish: Mark Complete
// =============================================================================

async fn setup_finish(State(_state): State<AdminState>) -> Response {
    let data_dir = crate::paths::data_dir();
    let data_dir_str = data_dir.to_string_lossy();

    match AuthConfig::load(&data_dir_str) {
        Ok(Some(mut config)) => {
            config.setup_complete = true;
            if let Err(e) = config.save(&data_dir_str) {
                tracing::error!(error = %e, "Failed to save auth config on setup finish");
                return SetupStep1Template {
                    error_message: Some("Failed to finalize setup. Check logs.".to_string()),
                    prefill_username: String::new(),
                }
                .into_response();
            }
            tracing::info!("Setup wizard completed");
            Redirect::to("/login").into_response()
        }
        _ => Redirect::temporary("/setup").into_response(),
    }
}

// =============================================================================
// Login
// =============================================================================

async fn login_view() -> LoginTemplate {
    LoginTemplate {
        error_message: None,
    }
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

async fn login_submit(
    State(_state): State<AdminState>,
    session: tower_sessions::Session,
    Form(form): Form<LoginForm>,
) -> Response {
    let data_dir = crate::paths::data_dir();
    let data_dir_str = data_dir.to_string_lossy();

    let auth_config = match AuthConfig::load(&data_dir_str) {
        Ok(Some(config)) => config,
        _ => {
            return LoginTemplate {
                error_message: Some("Auth not configured. Run setup first.".to_string()),
            }
            .into_response();
        }
    };

    let username = form.username.trim();
    let password = form.password.trim();

    if username != auth_config.username || !auth_config.verify_password(password) {
        return LoginTemplate {
            error_message: Some("Invalid username or password".to_string()),
        }
        .into_response();
    }

    // Set session
    if let Err(e) = session.insert("user", username.to_string()).await {
        tracing::error!(error = %e, "Failed to set session data");
        return LoginTemplate {
            error_message: Some("Session error. Try again.".to_string()),
        }
        .into_response();
    }

    tracing::info!(username = username, "User logged in");
    Redirect::to("/admin").into_response()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;

    #[test]
    fn test_setup_step1_template_renders() {
        let template = SetupStep1Template {
            error_message: None,
            prefill_username: String::new(),
        };
        let rendered = template.render().expect("Step 1 template should render");
        assert!(rendered.contains("Create your admin account"));
        assert!(rendered.contains("step 1/3"));
    }

    #[test]
    fn test_setup_step1_template_with_error() {
        let template = SetupStep1Template {
            error_message: Some("Passwords do not match".to_string()),
            prefill_username: "admin".to_string(),
        };
        let rendered = template.render().expect("Step 1 error template should render");
        assert!(rendered.contains("Passwords do not match"));
        assert!(rendered.contains("admin")); // Prefilled username
    }

    #[test]
    fn test_setup_step2_template_renders() {
        let template = SetupStep2Template {
            api_token: "gorp_tk_abcdef1234567890abcdef1234567890".to_string(),
        };
        let rendered = template.render().expect("Step 2 template should render");
        assert!(rendered.contains("gorp_tk_abcdef1234567890abcdef1234567890"));
        assert!(rendered.contains("step 2/3"));
        assert!(rendered.contains("X-API-Key"));
    }

    #[test]
    fn test_setup_step3_template_renders_no_platforms() {
        let template = SetupStep3Template {
            matrix_configured: false,
            telegram_configured: false,
        };
        let rendered = template.render().expect("Step 3 template should render");
        assert!(rendered.contains("step 3/3"));
        assert!(rendered.contains("Not configured"));
    }

    #[test]
    fn test_setup_step3_template_renders_with_platforms() {
        let template = SetupStep3Template {
            matrix_configured: true,
            telegram_configured: true,
        };
        let rendered = template.render().expect("Step 3 template should render");
        assert!(rendered.contains("Connected"));
    }

    #[test]
    fn test_login_template_renders() {
        let template = LoginTemplate {
            error_message: None,
        };
        let rendered = template.render().expect("Login template should render");
        assert!(rendered.contains("Sign In"));
        assert!(rendered.contains("Username"));
        assert!(rendered.contains("Password"));
    }

    #[test]
    fn test_login_template_with_error() {
        let template = LoginTemplate {
            error_message: Some("Invalid username or password".to_string()),
        };
        let rendered = template.render().expect("Login error template should render");
        assert!(rendered.contains("Invalid username or password"));
    }
}
