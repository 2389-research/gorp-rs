// ABOUTME: Tests for DispatchCommand parser in the orchestrator module.
// ABOUTME: Validates parsing of all DISPATCH control plane commands from message bodies.

use gorp::orchestrator::DispatchCommand;

#[test]
fn test_parse_dispatch_create() {
    let cmd = DispatchCommand::parse("!create research");
    assert_eq!(
        cmd,
        DispatchCommand::Create {
            name: "research".to_string(),
            workspace: None
        }
    );
}

#[test]
fn test_parse_dispatch_create_with_workspace() {
    let cmd = DispatchCommand::parse("!create research /home/harper/ws/research");
    assert_eq!(
        cmd,
        DispatchCommand::Create {
            name: "research".to_string(),
            workspace: Some("/home/harper/ws/research".to_string()),
        }
    );
}

#[test]
fn test_parse_dispatch_join() {
    let cmd = DispatchCommand::parse("!join research");
    assert_eq!(
        cmd,
        DispatchCommand::Join {
            name: "research".to_string()
        }
    );
}

#[test]
fn test_parse_dispatch_leave() {
    assert_eq!(DispatchCommand::parse("!leave"), DispatchCommand::Leave);
}

#[test]
fn test_parse_dispatch_list() {
    assert_eq!(DispatchCommand::parse("!list"), DispatchCommand::List);
}

#[test]
fn test_parse_dispatch_status() {
    let cmd = DispatchCommand::parse("!status research");
    assert_eq!(
        cmd,
        DispatchCommand::Status {
            name: "research".to_string()
        }
    );
}

#[test]
fn test_parse_dispatch_tell() {
    let cmd = DispatchCommand::parse("!tell research summarize the paper");
    assert_eq!(
        cmd,
        DispatchCommand::Tell {
            session: "research".to_string(),
            message: "summarize the paper".to_string(),
        }
    );
}

#[test]
fn test_parse_dispatch_read() {
    let cmd = DispatchCommand::parse("!read research 5");
    assert_eq!(
        cmd,
        DispatchCommand::Read {
            session: "research".to_string(),
            count: Some(5)
        }
    );
}

#[test]
fn test_parse_dispatch_read_default_count() {
    let cmd = DispatchCommand::parse("!read research");
    assert_eq!(
        cmd,
        DispatchCommand::Read {
            session: "research".to_string(),
            count: None
        }
    );
}

#[test]
fn test_parse_dispatch_broadcast() {
    let cmd = DispatchCommand::parse("!broadcast hey everyone");
    assert_eq!(
        cmd,
        DispatchCommand::Broadcast {
            message: "hey everyone".to_string()
        }
    );
}

#[test]
fn test_parse_dispatch_delete() {
    let cmd = DispatchCommand::parse("!delete research");
    assert_eq!(
        cmd,
        DispatchCommand::Delete {
            name: "research".to_string()
        }
    );
}

#[test]
fn test_parse_dispatch_help() {
    assert_eq!(DispatchCommand::parse("!help"), DispatchCommand::Help);
}

#[test]
fn test_parse_dispatch_unknown() {
    let cmd = DispatchCommand::parse("hello there");
    assert_eq!(
        cmd,
        DispatchCommand::Unknown("hello there".to_string())
    );
}

#[test]
fn test_parse_dispatch_unknown_command() {
    let cmd = DispatchCommand::parse("!frobnicate");
    assert_eq!(
        cmd,
        DispatchCommand::Unknown("!frobnicate".to_string())
    );
}

// Edge case: commands are case-insensitive
#[test]
fn test_parse_dispatch_case_insensitive() {
    assert_eq!(
        DispatchCommand::parse("!CREATE research"),
        DispatchCommand::Create {
            name: "research".to_string(),
            workspace: None
        }
    );
    assert_eq!(DispatchCommand::parse("!HELP"), DispatchCommand::Help);
    assert_eq!(DispatchCommand::parse("!List"), DispatchCommand::List);
}

// Edge case: leading/trailing whitespace is trimmed
#[test]
fn test_parse_dispatch_whitespace_trimmed() {
    assert_eq!(DispatchCommand::parse("  !help  "), DispatchCommand::Help);
    assert_eq!(
        DispatchCommand::parse("  !create research  "),
        DispatchCommand::Create {
            name: "research".to_string(),
            workspace: None
        }
    );
}

// Edge case: missing required args produce Unknown
#[test]
fn test_parse_dispatch_missing_args() {
    assert_eq!(
        DispatchCommand::parse("!create"),
        DispatchCommand::Unknown("!create".to_string())
    );
    assert_eq!(
        DispatchCommand::parse("!delete"),
        DispatchCommand::Unknown("!delete".to_string())
    );
    assert_eq!(
        DispatchCommand::parse("!status"),
        DispatchCommand::Unknown("!status".to_string())
    );
    assert_eq!(
        DispatchCommand::parse("!join"),
        DispatchCommand::Unknown("!join".to_string())
    );
    assert_eq!(
        DispatchCommand::parse("!tell"),
        DispatchCommand::Unknown("!tell".to_string())
    );
    assert_eq!(
        DispatchCommand::parse("!tell research"),
        DispatchCommand::Unknown("!tell research".to_string())
    );
    assert_eq!(
        DispatchCommand::parse("!read"),
        DispatchCommand::Unknown("!read".to_string())
    );
    assert_eq!(
        DispatchCommand::parse("!broadcast"),
        DispatchCommand::Unknown("!broadcast".to_string())
    );
}

// Edge case: empty input
#[test]
fn test_parse_dispatch_empty_input() {
    assert_eq!(
        DispatchCommand::parse(""),
        DispatchCommand::Unknown("".to_string())
    );
}

// Edge case: just whitespace
#[test]
fn test_parse_dispatch_whitespace_only() {
    assert_eq!(
        DispatchCommand::parse("   "),
        DispatchCommand::Unknown("".to_string())
    );
}
