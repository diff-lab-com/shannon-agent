//! Streaming/text parsing helpers: `<think>` splitting, text-form tool-call
//! recovery, markdown bash detection, and truncation classification.
//!
//! Split out of `engine.rs` (Wave 2 architecture step).


/// `stop_reason` values meaning "output was cut off by the output token
/// limit before the model finished": OpenAI-compatible `length`,
/// Anthropic/Gemini `max_tokens`.
pub(super) fn is_truncation_stop(reason: Option<&str>) -> bool {
    matches!(reason, Some("length" | "max_tokens"))
}


/// Split inline `<think>...</think>` reasoning out of assistant text.
///
/// Returns `(saw_reasoning, visible_text)` where `visible_text` is the text
/// outside reasoning blocks (reasoning-family providers — MiniMax M-series,
/// GLM — stream `<think>` inline in the content deltas). An unclosed
/// `<think>` swallows the rest of the text: a response cut mid-reasoning
/// has no visible answer by definition.
pub(super) fn split_think_content(text: &str) -> (bool, String) {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";
    let mut visible = String::with_capacity(text.len());
    let mut rest = text;
    let mut saw_think = false;
    loop {
        match rest.find(OPEN) {
            Some(start) => {
                saw_think = true;
                visible.push_str(&rest[..start]);
                let after_open = &rest[start + OPEN.len()..];
                match after_open.find(CLOSE) {
                    Some(close) => rest = &after_open[close + CLOSE.len()..],
                    // Unclosed reasoning block — nothing visible after it.
                    None => return (true, visible),
                }
            }
            None => {
                visible.push_str(rest);
                return (saw_think, visible);
            }
        }
    }
}

/// Streaming splitter for inline `<think>...</think>` reasoning blocks
/// (WP-15 P0-2). Reasoning-family providers on the OpenAI wire (MiniMax
/// M-series, GLM) stream `<think>` inline in the content deltas instead of
/// using a thinking channel, so the raw delta stream has to be re-split:
/// reasoning goes out as [`QueryEvent::Thinking`], the answer stays a plain
/// `Text` event. Tag-aware across chunk boundaries — `<think>` / `</think>`
/// may split across deltas, so a tail that could be a tag prefix is held
/// back (up to 7 chars of display lag; flushed by `finish`).
#[derive(Default)]
pub(super) struct ThinkStreamSplitter {
    /// Bytes held back because they may be a partial `<think>`/`</think>` tag.
    pending: String,
    /// True while inside a `<think>` block.
    in_think: bool,
}

impl ThinkStreamSplitter {
    const OPEN: &'static str = "<think>";
    const CLOSE: &'static str = "</think>";

    /// Consume one content delta. Returns `(thinking, visible)` — either
    /// side may be empty for a given chunk.
    pub(super) fn feed(&mut self, chunk: &str) -> (String, String) {
        let mut thinking = String::new();
        let mut visible = String::new();
        let mut buf = std::mem::take(&mut self.pending);
        buf.push_str(chunk);
        loop {
            let (tag, is_open) = if self.in_think {
                (Self::CLOSE, false)
            } else {
                (Self::OPEN, true)
            };
            match buf.find(tag) {
                Some(pos) => {
                    let head = &buf[..pos];
                    if self.in_think {
                        thinking.push_str(head);
                    } else {
                        visible.push_str(head);
                    }
                    buf = buf[pos + tag.len()..].to_string();
                    self.in_think = is_open;
                }
                None => {
                    // Hold back a tail that could be a tag prefix split across
                    // the next chunk boundary. The tag is ASCII, so a real
                    // prefix always starts on a char boundary — walk back only
                    // along boundaries (a raw `buf[len-n..]` panics on
                    // multi-byte text, e.g. Chinese replies).
                    let max_keep = tag.len().saturating_sub(1).min(buf.len());
                    let mut keep = 0;
                    for n in 1..=max_keep {
                        if !buf.is_char_boundary(buf.len() - n) {
                            continue;
                        }
                        if tag.starts_with(&buf[buf.len() - n..]) {
                            keep = n;
                            break;
                        }
                    }
                    let split_at = buf.len() - keep;
                    if self.in_think {
                        thinking.push_str(&buf[..split_at]);
                    } else {
                        visible.push_str(&buf[..split_at]);
                    }
                    self.pending = buf[split_at..].to_string();
                    return (thinking, visible);
                }
            }
        }
    }

    /// Flush at stream end. A dangling partial tag is literal text; an
    /// unclosed `<think>` swallows the tail (a response cut mid-reasoning
    /// has no visible answer by definition). Idempotent.
    pub(super) fn finish(&mut self) -> (String, String) {
        let pending = std::mem::take(&mut self.pending);
        if self.in_think {
            self.in_think = false;
            (pending, String::new())
        } else {
            (String::new(), pending)
        }
    }
}

/// WP-15 P0-1 fallback: extract a Bash tool command from an assistant reply
/// that tried to call the tool as a markdown code block instead of through
/// the native tool-calling API (observed in the field with MiniMax M-series).
///
/// Deliberately conservative — false positives mean executing example code
/// from an ordinary answer, so the fallback only fires when the *whole*
/// visible reply is one bare shell block (prose outside the fence ≤
/// `MAX_PROSE_CHARS`, one block, shell-ish or missing language tag):
///
/// - "```\nrm -rf build/\n```" → Some("rm -rf build/")
/// - "Creating the file:\n```bash\ncat > a.txt <<EOF\nhi\nEOF\n```" → Some(...)
/// - a long explanation that merely *contains* an example block → None
pub(super) fn markdown_bash_command(text: &str) -> Option<String> {
    const MAX_PROSE_CHARS: usize = 200;
    const SHELL_LANGS: &[&str] = &["bash", "sh", "shell", "zsh", "console"];

    let mut blocks: Vec<(bool, String)> = Vec::new(); // (shell_candidate, content)
    let mut prose_len = 0usize;
    let mut in_fence = false;
    let mut fence_shell = false;
    let mut fence_content = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if in_fence {
                blocks.push((fence_shell, std::mem::take(&mut fence_content)));
                in_fence = false;
            } else {
                in_fence = true;
                let lang = rest.trim().to_ascii_lowercase();
                fence_shell = lang.is_empty() || SHELL_LANGS.contains(&lang.as_str());
            }
            continue;
        }
        if in_fence {
            fence_content.push_str(line);
            fence_content.push('\n');
        } else {
            prose_len += trimmed.chars().count();
        }
    }
    // Unclosed fence still counts (stream cut after the opening).
    if in_fence {
        blocks.push((fence_shell, std::mem::take(&mut fence_content)));
    }

    if blocks.len() != 1 || prose_len > MAX_PROSE_CHARS {
        return None;
    }
    let (shell, content) = blocks.into_iter().next()?;
    if !shell {
        return None;
    }
    let command = content.trim().to_string();
    if command.is_empty() {
        return None;
    }
    Some(command)
}

/// WP-15 P0-1 (upgraded): parse GLM/MiniMax-style *textual* tool calls out of
/// an assistant reply. When these models bypass the native tool-calling API,
/// they don't always fall back to a bare shell block — MiniMax M3 was observed
/// emitting its vendor-token-wrapped XML form as plain text:
///
/// ```text
/// ]<]minimax[>[<tool_call>
/// <invoke name="Write">
/// <parameter name="file_path">/tmp/demo.txt</parameter>
/// <parameter name="content">hello</parameter>
/// </invoke>
/// </tool_call>
/// ```
///
/// (The `] < ]minimax[ > [` prefix is the model's vendor special token leaking
/// into the text stream — the scan anchors on `<tool_call>` and ignores
/// anything before it.) Returns one `(name, input)` per `<invoke>`, or `None`
/// when the text contains no complete invoke. Parameter values that parse as
/// JSON keep their type; everything else becomes a string. Unclosed blocks are
/// skipped — a stream cut mid-call is a truncation case, not a tool call.
pub(super) fn parse_text_tool_calls(text: &str) -> Option<Vec<(String, serde_json::Value)>> {
    const CALL_OPEN: &str = "<tool_call>";
    const CALL_CLOSE: &str = "</tool_call>";
    const INVOKE_OPEN: &str = "<invoke name=\"";
    const INVOKE_CLOSE: &str = "</invoke>";
    const PARAM_OPEN: &str = "<parameter name=\"";
    const PARAM_CLOSE: &str = "</parameter>";

    let mut calls: Vec<(String, serde_json::Value)> = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find(CALL_OPEN) {
        let after = &rest[pos + CALL_OPEN.len()..];
        let Some(call_end) = after.find(CALL_CLOSE) else {
            break; // unclosed block — truncation, not a call
        };
        let block = &after[..call_end];
        rest = &after[call_end + CALL_CLOSE.len()..];

        let mut brest = block;
        while let Some(ipos) = brest.find(INVOKE_OPEN) {
            let iafter = &brest[ipos + INVOKE_OPEN.len()..];
            let Some(name_end) = iafter.find("\">") else {
                break;
            };
            let name = iafter[..name_end].trim().to_string();
            let Some(invoke_end) = iafter.find(INVOKE_CLOSE) else {
                break;
            };
            let body = &iafter[name_end + 2..invoke_end];
            brest = &iafter[invoke_end + INVOKE_CLOSE.len()..];
            if name.is_empty() {
                continue;
            }

            let mut input = serde_json::Map::new();
            let mut prest = body;
            while let Some(ppos) = prest.find(PARAM_OPEN) {
                let pafter = &prest[ppos + PARAM_OPEN.len()..];
                let Some(key_end) = pafter.find("\">") else {
                    break;
                };
                let key = pafter[..key_end].trim().to_string();
                let Some(param_end) = pafter.find(PARAM_CLOSE) else {
                    break;
                };
                let raw = pafter[key_end + 2..param_end].trim();
                prest = &pafter[param_end + PARAM_CLOSE.len()..];
                if key.is_empty() {
                    continue;
                }
                let value = serde_json::from_str(raw)
                    .unwrap_or_else(|_| serde_json::Value::String(raw.to_string()));
                input.insert(key, value);
            }
            calls.push((name, serde_json::Value::Object(input)));
        }
    }
    if calls.is_empty() { None } else { Some(calls) }
}

/// True when the response carries no tool calls and no substantive
/// user-facing answer:
///
/// - empty text (covers reasoning that arrived only via thinking deltas), or
/// - reasoning markup present and the visible residue is shorter than
///   `min_answer_chars` — a think-only response.
///
/// Text *without* any reasoning markup is treated as a deliberate (possibly
/// terse) answer and never nudged: nudging every short "Done." would wreck
/// normal sessions for no eval benefit — the measured failure mode is always
/// reasoning-dominated.
pub(super) fn is_think_only_response(text: &str, tool_use_count: usize, min_answer_chars: usize) -> bool {
    if tool_use_count > 0 {
        return false;
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return true;
    }
    let (saw_think, visible) = split_think_content(trimmed);
    if !saw_think {
        return false;
    }
    // `.max(1)` makes threshold 0 mean "nudge only when the visible answer is
    // blank" (0 < 0 would never fire — see DEFAULT_THINK_ONLY_MIN_ANSWER_CHARS).
    visible.trim().chars().count() < min_answer_chars.max(1)
}

