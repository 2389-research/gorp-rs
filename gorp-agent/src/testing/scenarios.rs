// ABOUTME: Scenario test runner for comprehensive backend testing.
// ABOUTME: Loads scenarios from JSON, matches events, and validates backend behavior.

use crate::{AgentEvent, AgentHandle, ErrorCode};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

/// A test scenario loaded from JSON
#[derive(Debug, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub description: Option<String>,
    pub setup: Option<ScenarioSetup>,
    pub prompt: String,
    pub expected_events: Vec<EventMatcher>,
    pub assertions: Option<ScenarioAssertions>,
    pub timeout_ms: Option<u64>,
}

/// Setup configuration for a scenario
#[derive(Debug, Serialize, Deserialize)]
pub struct ScenarioSetup {
    pub files: Option<HashMap<String, String>>,
    pub mcp_servers: Option<Vec<String>>,
}

/// Matcher for expected events
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EventMatcher {
    Text { contains: String },
    ToolStart {
        name: String,
        input_contains: Option<Value>,
    },
    ToolEnd { name: String, success: bool },
    Result { contains: String },
    Error { code: Option<ErrorCode> },
    Custom { kind: String },
    Any { count: usize },
}

impl EventMatcher {
    /// Check if this matcher matches the given event
    pub fn matches(&self, event: &AgentEvent) -> bool {
        match (self, event) {
            (EventMatcher::Text { contains }, AgentEvent::Text(text)) => text.contains(contains),

            (
                EventMatcher::ToolStart {
                    name,
                    input_contains,
                },
                AgentEvent::ToolStart {
                    name: event_name,
                    input,
                    ..
                },
            ) => {
                if name != event_name {
                    return false;
                }
                if let Some(expected_input) = input_contains {
                    json_contains(input, expected_input)
                } else {
                    true
                }
            }

            (
                EventMatcher::ToolEnd { name, success },
                AgentEvent::ToolEnd {
                    name: event_name,
                    success: event_success,
                    ..
                },
            ) => name == event_name && success == event_success,

            (EventMatcher::Result { contains }, AgentEvent::Result { text, .. }) => {
                text.contains(contains)
            }

            (EventMatcher::Error { code }, AgentEvent::Error { code: event_code, .. }) => {
                if let Some(expected_code) = code {
                    expected_code == event_code
                } else {
                    true
                }
            }

            (EventMatcher::Custom { kind }, AgentEvent::Custom { kind: event_kind, .. }) => {
                kind == event_kind
            }

            (EventMatcher::Any { .. }, _) => true,

            _ => false,
        }
    }
}

/// Assertions to check after scenario completes
#[derive(Debug, Serialize, Deserialize)]
pub struct ScenarioAssertions {
    pub files: Option<HashMap<String, FileAssertion>>,
}

/// Assertion for a file's state
#[derive(Debug, Serialize, Deserialize)]
pub struct FileAssertion {
    pub contains: Option<String>,
    pub equals: Option<String>,
    pub not_exists: Option<bool>,
}

/// Result of running a scenario
pub struct ScenarioResult {
    pub name: String,
    pub passed: bool,
    pub duration: Duration,
    pub failures: Vec<String>,
}

/// Report from running multiple scenarios
pub struct ScenarioReport {
    pub results: Vec<ScenarioResult>,
    pub passed: usize,
    pub failed: usize,
}

/// Run a single scenario against a backend
pub async fn run_scenario(handle: &AgentHandle, scenario: &Scenario) -> ScenarioResult {
    let start = Instant::now();
    let mut failures = Vec::new();

    // Create a new session
    let session_id = match handle.new_session().await {
        Ok(id) => id,
        Err(e) => {
            failures.push(format!("Failed to create session: {}", e));
            return ScenarioResult {
                name: scenario.name.clone(),
                passed: false,
                duration: start.elapsed(),
                failures,
            };
        }
    };

    // Send the prompt and collect events
    let mut receiver = match handle.prompt(&session_id, &scenario.prompt).await {
        Ok(r) => r,
        Err(e) => {
            failures.push(format!("Failed to send prompt: {}", e));
            return ScenarioResult {
                name: scenario.name.clone(),
                passed: false,
                duration: start.elapsed(),
                failures,
            };
        }
    };

    let mut events = Vec::new();
    let timeout = Duration::from_millis(scenario.timeout_ms.unwrap_or(30000));
    let deadline = Instant::now() + timeout;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            failures.push("Scenario timed out waiting for events".to_string());
            break;
        }

        match tokio::time::timeout(remaining, receiver.recv()).await {
            Ok(Some(event)) => events.push(event),
            Ok(None) => break, // Stream closed
            Err(_) => {
                failures.push("Timed out waiting for next event".to_string());
                break;
            }
        }
    }

    // Match events against expected patterns
    let match_result = match_events(&events, &scenario.expected_events);
    if let Err(err) = match_result {
        failures.push(err);
    }

    // Check file assertions if specified
    if let Some(assertions) = &scenario.assertions {
        if let Some(file_assertions) = &assertions.files {
            for (path, assertion) in file_assertions {
                if let Err(err) = check_file_assertion(path, assertion) {
                    failures.push(err);
                }
            }
        }
    }

    ScenarioResult {
        name: scenario.name.clone(),
        passed: failures.is_empty(),
        duration: start.elapsed(),
        failures,
    }
}

/// Match events against expected patterns
fn match_events(events: &[AgentEvent], matchers: &[EventMatcher]) -> Result<(), String> {
    let mut event_idx = 0;

    for matcher in matchers {
        match matcher {
            EventMatcher::Any { count } => {
                // Any matcher consumes the specified number of events
                if event_idx + count > events.len() {
                    return Err(format!(
                        "Not enough events for Any matcher: needed {}, only {} remaining",
                        count,
                        events.len() - event_idx
                    ));
                }
                event_idx += count;
            }
            _ => {
                // Find the next matching event
                let mut found = false;
                while event_idx < events.len() {
                    if matcher.matches(&events[event_idx]) {
                        found = true;
                        event_idx += 1;
                        break;
                    }
                    event_idx += 1;
                }

                if !found {
                    return Err(format!(
                        "Expected event not found: {:?}. Remaining events: {}",
                        matcher,
                        events.len() - event_idx
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Check if target JSON contains all fields from expected JSON
fn json_contains(target: &Value, expected: &Value) -> bool {
    match (expected, target) {
        (Value::Object(exp_map), Value::Object(target_map)) => {
            for (key, exp_val) in exp_map {
                if let Some(target_val) = target_map.get(key) {
                    if !json_contains(target_val, exp_val) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            true
        }
        (exp, target) => exp == target,
    }
}

/// Check a file assertion
fn check_file_assertion(path: &str, assertion: &FileAssertion) -> Result<(), String> {
    if let Some(should_not_exist) = assertion.not_exists {
        let exists = std::path::Path::new(path).exists();
        if should_not_exist && exists {
            return Err(format!("File should not exist: {}", path));
        }
        if !should_not_exist && !exists {
            return Err(format!("File should exist: {}", path));
        }
        return Ok(());
    }

    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file {}: {}", path, e))?;

    if let Some(expected_contains) = &assertion.contains {
        if !contents.contains(expected_contains) {
            return Err(format!(
                "File {} does not contain expected text: {}",
                path, expected_contains
            ));
        }
    }

    if let Some(expected_equals) = &assertion.equals {
        if contents != *expected_equals {
            return Err(format!("File {} does not match expected content", path));
        }
    }

    Ok(())
}

/// Run all scenarios from a directory
pub async fn run_scenarios(handle: &AgentHandle, scenarios_dir: &Path) -> ScenarioReport {
    let mut results = Vec::new();

    // Find all JSON files in the directory
    if let Ok(entries) = std::fs::read_dir(scenarios_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                match load_scenario(&path) {
                    Ok(scenario) => {
                        let result = run_scenario(handle, &scenario).await;
                        results.push(result);
                    }
                    Err(e) => {
                        results.push(ScenarioResult {
                            name: path.display().to_string(),
                            passed: false,
                            duration: Duration::from_secs(0),
                            failures: vec![format!("Failed to load scenario: {}", e)],
                        });
                    }
                }
            }
        }
    }

    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.len() - passed;

    ScenarioReport {
        results,
        passed,
        failed,
    }
}

/// Load a scenario from a JSON file
pub fn load_scenario(path: &Path) -> Result<Scenario> {
    let contents = std::fs::read_to_string(path)?;
    let scenario: Scenario = serde_json::from_str(&contents)?;
    Ok(scenario)
}
