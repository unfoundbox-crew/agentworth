use serde_json::Value;

/// Normalizes tool names into a canonical MCP format (`mcp:<server>:<tool>`).
/// Handles:
/// - Explicit `call_mcp_tool` / `call_mcp` / `mcp_call` invocations with arguments containing `ServerName` / `server_name` and `ToolName` / `tool_name`
/// - Anthropic Claude MCP tool call naming (`mcp__<server>__<tool>`)
/// - Underscore-separated MCP names (`mcp_<server>_<tool>`)
/// - Developer tool naming (`developer__<server>__<tool>` or `developer__<tool>`)
/// - Standard or already normalized tool names
pub fn normalize_mcp_tool_name(raw_name: &str, arguments: &Value) -> String {
    let lower_raw = raw_name.to_lowercase();

    // 1. Check for explicit call_mcp_tool invocations
    if lower_raw == "call_mcp_tool" || lower_raw == "call_mcp" || lower_raw == "mcp_call" {
        let server = arguments
            .get("ServerName")
            .or_else(|| arguments.get("server_name"))
            .or_else(|| arguments.get("server"))
            .or_else(|| arguments.get("serverName"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let tool = arguments
            .get("ToolName")
            .or_else(|| arguments.get("tool_name"))
            .or_else(|| arguments.get("tool"))
            .or_else(|| arguments.get("toolName"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !server.is_empty() && !tool.is_empty() {
            return format!("mcp:{}:{}", server, tool);
        } else if !tool.is_empty() {
            return format!("mcp:{}", tool);
        } else if !server.is_empty() {
            return format!("mcp:{}", server);
        }
        return raw_name.to_string();
    }

    // 2. Check for mcp__<server>__<tool> or mcp__<tool>
    if let Some(rem) = raw_name.strip_prefix("mcp__") {
        if let Some((server, tool)) = rem.split_once("__") {
            return format!("mcp:{}:{}", server, tool);
        } else if let Some((server, tool)) = rem.split_once(':') {
            return format!("mcp:{}:{}", server, tool);
        } else {
            return format!("mcp:{}", rem);
        }
    }

    // 3. Check for developer__<server>__<tool> or developer__<tool>
    if let Some(rem) = raw_name.strip_prefix("developer__") {
        if let Some((server, tool)) = rem.split_once("__") {
            return format!("mcp:{}:{}", server, tool);
        } else {
            return format!("mcp:developer:{}", rem);
        }
    }

    // 4. Check for mcp_<server>_<tool> (e.g. eager MCP tool registration)
    if let Some(rem) = raw_name.strip_prefix("mcp_") {
        if let Some((server, tool)) = rem.split_once('_') {
            return format!("mcp:{}:{}", server, tool);
        } else {
            return format!("mcp:{}", rem);
        }
    }

    // 5. Check for developer_<tool>
    if let Some(rem) = raw_name.strip_prefix("developer_") {
        if let Some((server, tool)) = rem.split_once('_') {
            return format!("mcp:{}:{}", server, tool);
        } else {
            return format!("mcp:developer:{}", rem);
        }
    }

    raw_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_normalize_call_mcp_tool() {
        let args = json!({
            "ServerName": "chrome-devtools",
            "ToolName": "navigate_page"
        });
        assert_eq!(
            normalize_mcp_tool_name("call_mcp_tool", &args),
            "mcp:chrome-devtools:navigate_page"
        );

        let args_snake = json!({
            "server_name": "postgres",
            "tool_name": "execute_query"
        });
        assert_eq!(
            normalize_mcp_tool_name("call_mcp_tool", &args_snake),
            "mcp:postgres:execute_query"
        );
    }

    #[test]
    fn test_normalize_mcp_double_underscore() {
        assert_eq!(
            normalize_mcp_tool_name("mcp__postgres__query", &json!({})),
            "mcp:postgres:query"
        );
        assert_eq!(
            normalize_mcp_tool_name("mcp__github__create_issue", &json!({})),
            "mcp:github:create_issue"
        );
    }

    #[test]
    fn test_normalize_developer_prefix() {
        assert_eq!(
            normalize_mcp_tool_name("developer__postgres__query", &json!({})),
            "mcp:postgres:query"
        );
        assert_eq!(
            normalize_mcp_tool_name("developer__text_editor", &json!({})),
            "mcp:developer:text_editor"
        );
    }

    #[test]
    fn test_normalize_standard_tool_name() {
        assert_eq!(normalize_mcp_tool_name("Bash", &json!({})), "Bash");
        assert_eq!(
            normalize_mcp_tool_name("run_command", &json!({})),
            "run_command"
        );
    }
}
