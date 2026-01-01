// ABOUTME: Generic command parsing for chat bot commands
// ABOUTME: Platform-agnostic !command handling

/// Represents a parsed command from a chat message
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The command name (without prefix)
    pub name: String,
    /// Parsed arguments (handles quoted strings)
    pub args: Vec<String>,
    /// The raw argument string after the command name
    pub raw_args: String,
}

impl Command {
    /// Create a new command with name and arguments
    pub fn new(name: impl Into<String>, args: Vec<String>, raw_args: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            args,
            raw_args: raw_args.into(),
        }
    }

    /// Get the first argument if present
    pub fn first_arg(&self) -> Option<&str> {
        self.args.first().map(|s| s.as_str())
    }

    /// Get an argument by index
    pub fn arg(&self, index: usize) -> Option<&str> {
        self.args.get(index).map(|s| s.as_str())
    }

    /// Check if the command has a specific number of arguments
    pub fn has_args(&self, count: usize) -> bool {
        self.args.len() >= count
    }
}

/// Result of parsing a message
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseResult {
    /// A command was recognized
    Command(Command),
    /// A regular message (not a command)
    Message(String),
    /// Message should be ignored (empty, escape sequence, etc.)
    Ignore,
}

impl ParseResult {
    /// Returns true if this is a command
    pub fn is_command(&self) -> bool {
        matches!(self, ParseResult::Command(_))
    }

    /// Returns true if this is a regular message
    pub fn is_message(&self) -> bool {
        matches!(self, ParseResult::Message(_))
    }

    /// Returns true if this should be ignored
    pub fn is_ignore(&self) -> bool {
        matches!(self, ParseResult::Ignore)
    }

    /// Get the command if this is one
    pub fn as_command(&self) -> Option<&Command> {
        match self {
            ParseResult::Command(cmd) => Some(cmd),
            _ => None,
        }
    }

    /// Get the message text if this is a regular message
    pub fn as_message(&self) -> Option<&str> {
        match self {
            ParseResult::Message(msg) => Some(msg),
            _ => None,
        }
    }
}

/// Parse arguments from a string, respecting quoted strings
fn parse_args(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = c;
            }
            c if c == quote_char && in_quotes => {
                in_quotes = false;
                // Don't add empty strings from consecutive quotes
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    // Don't forget the last argument
    if !current.is_empty() {
        args.push(current);
    }

    args
}

/// Parse a chat message to determine if it's a command
///
/// # Arguments
/// * `body` - The message body to parse
/// * `bot_prefix` - The bot's command prefix (e.g., "!claude")
///
/// # Returns
/// * `ParseResult::Command` - If the message is a valid command
/// * `ParseResult::Message` - If the message is regular text
/// * `ParseResult::Ignore` - If the message should be ignored
///
/// # Command Recognition
/// Recognizes commands in these formats:
/// - `!command` - Simple command prefix
/// - `{bot_prefix} command` - Bot mention prefix (e.g., "!claude help")
///
/// # Escape Sequences
/// - Messages starting with `!!` are treated as regular messages (escape)
/// - Empty messages are ignored
pub fn parse_message(body: &str, bot_prefix: &str) -> ParseResult {
    let trimmed = body.trim();

    // Empty messages are ignored
    if trimmed.is_empty() {
        return ParseResult::Ignore;
    }

    // Escape sequence: !! at start means treat as regular message
    if trimmed.starts_with("!!") {
        let escaped = trimmed[2..].trim();
        if escaped.is_empty() {
            return ParseResult::Ignore;
        }
        return ParseResult::Message(escaped.to_string());
    }

    // Check for bot prefix style: "!claude command args"
    let bot_prefix_lower = bot_prefix.to_lowercase();
    let trimmed_lower = trimmed.to_lowercase();

    if trimmed_lower.starts_with(&bot_prefix_lower)
        && trimmed.len() > bot_prefix.len()
        && trimmed
            .chars()
            .nth(bot_prefix.len())
            .is_some_and(|c| c.is_whitespace())
    {
        let remainder = trimmed[bot_prefix.len()..].trim();
        if remainder.is_empty() {
            // Just the prefix with nothing after it
            return ParseResult::Command(Command::new("", Vec::new(), ""));
        }
        return parse_command_from_text(remainder);
    }

    // Check for simple !command style
    if trimmed.starts_with('!') && trimmed.len() > 1 {
        let after_bang = &trimmed[1..];
        // Must start with an alphabetic character
        if after_bang.chars().next().is_some_and(|c| c.is_alphabetic()) {
            return parse_command_from_text(after_bang);
        }
    }

    // Regular message
    ParseResult::Message(trimmed.to_string())
}

/// Parse command name and arguments from text (without the prefix)
fn parse_command_from_text(text: &str) -> ParseResult {
    let text = text.trim();
    if text.is_empty() {
        return ParseResult::Command(Command::new("", Vec::new(), ""));
    }

    // Split into command name and rest
    let parts: Vec<&str> = text.splitn(2, char::is_whitespace).collect();
    let name = parts[0].to_lowercase();
    let raw_args = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
    let args = parse_args(&raw_args);

    ParseResult::Command(Command::new(name, args, raw_args))
}

/// Trait for handling parsed commands
///
/// Implement this trait to handle commands from any chat platform.
/// The handler receives a parsed `Command` and returns a response.
pub trait CommandHandler: Send + Sync {
    /// The type of context passed to the handler (e.g., room info, user info)
    type Context;

    /// The type of response returned by the handler
    type Response;

    /// The error type for handler failures
    type Error;

    /// Handle a parsed command
    ///
    /// # Arguments
    /// * `command` - The parsed command
    /// * `context` - Platform-specific context
    ///
    /// # Returns
    /// * `Ok(Some(response))` - Command was handled successfully
    /// * `Ok(None)` - Command was not recognized by this handler
    /// * `Err(error)` - Command handling failed
    fn handle(
        &self,
        command: &Command,
        context: &Self::Context,
    ) -> Result<Option<Self::Response>, Self::Error>;
}

/// A registry of command handlers that can be matched against
pub struct CommandRegistry<C, R, E> {
    handlers: Vec<Box<dyn CommandHandler<Context = C, Response = R, Error = E>>>,
}

impl<C, R, E> Default for CommandRegistry<C, R, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<C, R, E> CommandRegistry<C, R, E> {
    /// Create a new empty command registry
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Register a command handler
    pub fn register<H>(&mut self, handler: H)
    where
        H: CommandHandler<Context = C, Response = R, Error = E> + 'static,
    {
        self.handlers.push(Box::new(handler));
    }

    /// Try to handle a command with registered handlers
    ///
    /// Returns the first successful handler response, or None if no handler matched.
    pub fn handle(&self, command: &Command, context: &C) -> Result<Option<R>, E> {
        for handler in &self.handlers {
            if let Some(response) = handler.handle(command, context)? {
                return Ok(Some(response));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_command() {
        let result = parse_message("!help", "!claude");
        assert!(matches!(
            result,
            ParseResult::Command(ref cmd) if cmd.name == "help"
        ));
    }

    #[test]
    fn test_parse_command_with_args() {
        let result = parse_message("!create my-channel", "!claude");
        match result {
            ParseResult::Command(cmd) => {
                assert_eq!(cmd.name, "create");
                assert_eq!(cmd.args, vec!["my-channel"]);
                assert_eq!(cmd.raw_args, "my-channel");
            }
            _ => panic!("Expected command"),
        }
    }

    #[test]
    fn test_parse_bot_prefix_command() {
        let result = parse_message("!claude help", "!claude");
        match result {
            ParseResult::Command(cmd) => {
                assert_eq!(cmd.name, "help");
                assert!(cmd.args.is_empty());
            }
            _ => panic!("Expected command"),
        }
    }

    #[test]
    fn test_parse_bot_prefix_with_args() {
        let result = parse_message("!claude create my-channel", "!claude");
        match result {
            ParseResult::Command(cmd) => {
                assert_eq!(cmd.name, "create");
                assert_eq!(cmd.args, vec!["my-channel"]);
            }
            _ => panic!("Expected command"),
        }
    }

    #[test]
    fn test_parse_escape_sequence() {
        let result = parse_message("!!not a command", "!claude");
        match result {
            ParseResult::Message(msg) => {
                assert_eq!(msg, "not a command");
            }
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_parse_regular_message() {
        let result = parse_message("hello world", "!claude");
        match result {
            ParseResult::Message(msg) => {
                assert_eq!(msg, "hello world");
            }
            _ => panic!("Expected message"),
        }
    }

    #[test]
    fn test_parse_empty_message() {
        let result = parse_message("", "!claude");
        assert!(matches!(result, ParseResult::Ignore));
    }

    #[test]
    fn test_parse_whitespace_only() {
        let result = parse_message("   ", "!claude");
        assert!(matches!(result, ParseResult::Ignore));
    }

    #[test]
    fn test_parse_just_exclamation() {
        let result = parse_message("!", "!claude");
        assert!(matches!(result, ParseResult::Message(_)));
    }

    #[test]
    fn test_parse_quoted_args() {
        let result = parse_message("!search \"hello world\" today", "!claude");
        match result {
            ParseResult::Command(cmd) => {
                assert_eq!(cmd.name, "search");
                assert_eq!(cmd.args, vec!["hello world", "today"]);
            }
            _ => panic!("Expected command"),
        }
    }

    #[test]
    fn test_parse_single_quoted_args() {
        let result = parse_message("!search 'hello world' today", "!claude");
        match result {
            ParseResult::Command(cmd) => {
                assert_eq!(cmd.name, "search");
                assert_eq!(cmd.args, vec!["hello world", "today"]);
            }
            _ => panic!("Expected command"),
        }
    }

    #[test]
    fn test_parse_case_insensitive_prefix() {
        let result = parse_message("!CLAUDE help", "!claude");
        match result {
            ParseResult::Command(cmd) => {
                assert_eq!(cmd.name, "help");
            }
            _ => panic!("Expected command"),
        }
    }

    #[test]
    fn test_command_first_arg() {
        let cmd = Command::new("test", vec!["arg1".into(), "arg2".into()], "arg1 arg2");
        assert_eq!(cmd.first_arg(), Some("arg1"));
        assert_eq!(cmd.arg(1), Some("arg2"));
        assert_eq!(cmd.arg(2), None);
    }

    #[test]
    fn test_command_has_args() {
        let cmd = Command::new("test", vec!["arg1".into()], "arg1");
        assert!(cmd.has_args(1));
        assert!(!cmd.has_args(2));
    }

    #[test]
    fn test_parse_result_methods() {
        let cmd_result = ParseResult::Command(Command::new("test", vec![], ""));
        assert!(cmd_result.is_command());
        assert!(!cmd_result.is_message());
        assert!(!cmd_result.is_ignore());

        let msg_result = ParseResult::Message("hello".into());
        assert!(!msg_result.is_command());
        assert!(msg_result.is_message());
        assert!(!msg_result.is_ignore());

        let ignore_result = ParseResult::Ignore;
        assert!(!ignore_result.is_command());
        assert!(!ignore_result.is_message());
        assert!(ignore_result.is_ignore());
    }

    #[test]
    fn test_non_alphabetic_after_bang() {
        // !123 should not be a command
        let result = parse_message("!123", "!claude");
        assert!(matches!(result, ParseResult::Message(_)));

        // !-test should not be a command
        let result = parse_message("!-test", "!claude");
        assert!(matches!(result, ParseResult::Message(_)));
    }

    #[test]
    fn test_escape_empty() {
        // !! followed by nothing should be ignored
        let result = parse_message("!!", "!claude");
        assert!(matches!(result, ParseResult::Ignore));
    }
}
