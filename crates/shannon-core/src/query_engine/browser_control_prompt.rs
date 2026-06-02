//! Browser control system prompt injection.
//!
//! When Playwright/Chrome DevTools MCP tools are detected, this module generates
//! instructions so the LLM knows how to interact with web pages.

/// Returns the browser control system prompt if browser MCP tools are detected.
pub fn browser_control_prompt(tool_names: &[String]) -> Option<String> {
    let has_browser_tools = tool_names.iter().any(|n| {
        let n_lower = n.to_lowercase();
        n_lower.contains("playwright")
            || n_lower.contains("chrome-devtools")
            || n_lower.contains("browser_")
            || n_lower.contains("screenshot")
            || matches!(
                n.as_str(),
                "browser_navigate"
                    | "browser_click"
                    | "browser_snapshot"
                    | "browser_type"
                    | "browser_take_screenshot"
                    | "browser_fill_form"
                    | "browser_press_key"
                    | "browser_hover"
                    | "browser_select_option"
                    | "browser_evaluate"
                    | "browser_tabs"
            )
    });

    if !has_browser_tools {
        return None;
    }

    Some(BROWSER_CONTROL_PROMPT.to_string())
}

const BROWSER_CONTROL_PROMPT: &str = "\
# Browser Control

You can interact with web browsers through browser automation tools. Use these to test web applications, debug UI issues, capture screenshots, and verify frontend behavior.

## Workflow

1. **Navigate**: Open a URL with `browser_navigate` or `navigate_page`
2. **Observe**: Take a snapshot (`browser_snapshot`) or screenshot (`browser_take_screenshot`) to see the page state
3. **Interact**: Click elements, type text, fill forms, press keys
4. **Verify**: Take another snapshot/screenshot to confirm the result
5. **Iterate**: Repeat until the task is complete

## Key Principles

- **Snapshot first**: Always take a snapshot before interacting to get the current page state and element UIDs
- **Use UIDs**: Elements are identified by unique IDs (uid) from the snapshot — always use the latest snapshot's UIDs
- **Fill forms efficiently**: Use `browser_fill_form` to fill multiple fields at once instead of individual calls
- **Wait when needed**: Use `browser_wait_for` to wait for text or elements to appear after navigation or interaction
- **Check console errors**: Use `browser_console_messages` to check for JavaScript errors when debugging

## Common Patterns

### Test a web page
```
1. browser_navigate({ url: \"http://localhost:3000\" })
2. browser_snapshot()
3. browser_click({ uid: \"submit-button\" })
4. browser_wait_for({ text: [\"Success\"] })
5. browser_snapshot()  // verify result
```

### Debug a UI issue
```
1. browser_navigate({ url: \"http://localhost:3000/problem-page\" })
2. browser_take_screenshot()  // visual check
3. browser_console_messages({ level: \"error\" })  // check for errors
4. browser_snapshot()  // inspect element structure
```

### Fill and submit a form
```
1. browser_snapshot()  // get element UIDs
2. browser_fill_form({ elements: [{uid: \"name\", value: \"test\"}, {uid: \"email\", value: \"test@example.com\"}] })
3. browser_click({ uid: \"submit\" })
4. browser_wait_for({ text: [\"Submitted\"] })
```

## Important Notes

- Coordinates in screenshots are viewport-relative CSS pixels
- Snapshot UIDs change after each navigation — always take a fresh snapshot
- For accessibility testing, use `lighthouse_audit`
- For performance analysis, use `performance_start_trace`
- Browser interactions are real — only interact with pages you intend to affect
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_prompt_without_browser_tools() {
        let tools = vec!["Bash".to_string(), "Read".to_string(), "Write".to_string()];
        assert!(browser_control_prompt(&tools).is_none());
    }

    #[test]
    fn test_prompt_with_playwright_tools() {
        let tools = vec![
            "Bash".to_string(),
            "browser_navigate".to_string(),
            "Read".to_string(),
        ];
        let prompt = browser_control_prompt(&tools);
        assert!(prompt.is_some());
        let text = prompt.unwrap();
        assert!(text.contains("browser_navigate"));
        assert!(text.contains("browser_snapshot"));
        assert!(text.contains("browser_click"));
    }

    #[test]
    fn test_prompt_with_chrome_devtools() {
        let tools = vec!["mcp__chrome-devtools__take_screenshot".to_string()];
        assert!(browser_control_prompt(&tools).is_some());
    }

    #[test]
    fn test_prompt_with_playwright_prefix() {
        let tools = vec!["mcp__plugin_playwright_playwright__browser_navigate".to_string()];
        assert!(browser_control_prompt(&tools).is_some());
    }

    #[test]
    fn test_prompt_content_completeness() {
        let tools = vec!["browser_navigate".to_string()];
        let prompt = browser_control_prompt(&tools).unwrap();
        assert!(prompt.contains("Workflow"));
        assert!(prompt.contains("Snapshot first"));
        assert!(prompt.contains("Common Patterns"));
        assert!(prompt.contains("browser_fill_form"));
        assert!(prompt.contains("browser_wait_for"));
        assert!(prompt.contains("console_messages"));
    }

    #[test]
    fn test_empty_tool_list() {
        assert!(browser_control_prompt(&[]).is_none());
    }

    #[test]
    fn test_browser_type_tool() {
        let tools = vec!["browser_type".to_string()];
        assert!(browser_control_prompt(&tools).is_some());
    }

    #[test]
    fn test_browser_take_screenshot_tool() {
        let tools = vec!["browser_take_screenshot".to_string()];
        assert!(browser_control_prompt(&tools).is_some());
    }

    #[test]
    fn test_snapshot_keyword_not_matching() {
        // "snapshot" alone shouldn't trigger — only specific browser tools
        let tools = vec!["snapshot".to_string()];
        assert!(browser_control_prompt(&tools).is_none());
    }

    #[test]
    fn test_screenshot_keyword_matching() {
        // "screenshot" in tool name should trigger (contains check)
        let tools = vec!["take_screenshot".to_string()];
        assert!(browser_control_prompt(&tools).is_some());
    }

    #[test]
    fn test_duplicate_tools() {
        let tools = vec![
            "browser_navigate".to_string(),
            "browser_navigate".to_string(),
        ];
        let prompt = browser_control_prompt(&tools);
        assert!(prompt.is_some());
        assert_eq!(prompt.unwrap().matches("Browser Control").count(), 1);
    }
}
