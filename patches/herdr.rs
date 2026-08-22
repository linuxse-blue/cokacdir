//! Herdr agent provider.
//!
//! This adapter forwards a Cokacdir turn to an already-running Herdr agent,
//! waits for the agent to settle, then reads the terminal output back.
//! Prompt boundaries recognize Codex (`› `), AGY (`> `), and Grok (`❯ `).

use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::services::claude::{
    attach_cancel_cgroup, detach_into_own_pgroup, enhanced_path_for_bin, kill_child_tree,
    send_success_terminal, CancelToken, StreamMessage,
};

const DEFAULT_TIMEOUT_MS: u64 = 30 * 60 * 1000;
const DEFAULT_READ_LINES: u32 = 1000;
const DEFAULT_SCREEN_LINES: u32 = 120;
const COMPLETION_READ_LINES: u32 = 120;
const COMPLETION_PREVIEW_CHARS: usize = 1200;

static ACTIVE_PROMPTS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

struct ActivePromptGuard {
    target: String,
}

impl ActivePromptGuard {
    fn new(target: &str) -> Self {
        let mut prompts = ACTIVE_PROMPTS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *prompts.entry(target.to_string()).or_insert(0) += 1;
        Self {
            target: target.to_string(),
        }
    }
}

impl Drop for ActivePromptGuard {
    fn drop(&mut self) {
        let mut prompts = ACTIVE_PROMPTS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = prompts.get_mut(&self.target) {
            *count -= 1;
            if *count == 0 {
                prompts.remove(&self.target);
            }
        }
    }
}

pub fn is_herdr_model(model: Option<&str>) -> bool {
    model
        .map(|model| model == "herdr" || model.starts_with("herdr:"))
        .unwrap_or(false)
}

pub fn strip_herdr_prefix(model: &str) -> Option<&str> {
    model
        .strip_prefix("herdr:")
        .filter(|target| !target.is_empty())
}

pub fn is_valid_target(target: &str) -> bool {
    let mut chars = target.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && target.len() <= 32
}

pub fn target_from_model(model: Option<&str>) -> Result<String, String> {
    if let Some(target) = model.and_then(strip_herdr_prefix) {
        if is_valid_target(target) {
            return Ok(target.to_string());
        }
        return Err(
            "Invalid Herdr agent name. Use 1-32 lowercase letters, digits, '-' or '_'.".to_string(),
        );
    }

    let target = std::env::var("COKAC_HERDR_AGENT").map_err(|_| {
        "Herdr agent is not configured. Use /model herdr:<agent-name> or set COKAC_HERDR_AGENT."
            .to_string()
    })?;
    if is_valid_target(&target) {
        Ok(target)
    } else {
        Err("COKAC_HERDR_AGENT contains an invalid Herdr agent name.".to_string())
    }
}

fn herdr_path() -> Option<String> {
    if let Ok(path) = std::env::var("COKAC_HERDR_PATH") {
        if !path.is_empty() && Path::new(&path).is_file() {
            return Some(path);
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let user_local = format!("{home}/.local/bin/herdr");
        if Path::new(&user_local).is_file() {
            return Some(user_local);
        }
    }

    for path in ["/usr/local/bin/herdr", "/usr/bin/herdr", "/bin/herdr"] {
        if Path::new(path).is_file() {
            return Some(path.to_string());
        }
    }

    let output = Command::new("which").arg("herdr").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty() && Path::new(&path).is_file()).then_some(path)
}

pub fn is_herdr_available() -> bool {
    herdr_path().is_some()
}

fn timeout_ms() -> u64 {
    std::env::var("COKAC_HERDR_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value >= 1000)
        .unwrap_or(DEFAULT_TIMEOUT_MS)
}

fn read_lines() -> u32 {
    std::env::var("COKAC_HERDR_READ_LINES")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_READ_LINES)
}

fn screen_lines() -> u32 {
    std::env::var("COKAC_HERDR_SCREEN_LINES")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SCREEN_LINES)
}

fn command(bin: &str) -> Command {
    let mut command = Command::new(bin);
    command.env("PATH", enhanced_path_for_bin(bin));
    command
}

fn output_error(action: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    if detail.is_empty() {
        format!(
            "Herdr {action} failed with exit code {}.",
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )
    } else {
        format!("Herdr {action} failed: {detail}")
    }
}

fn read_agent_lines(bin: &str, target: &str, lines: u32) -> Result<String, String> {
    let output = command(bin)
        .args([
            "agent",
            "read",
            target,
            "--source",
            "recent-unwrapped",
            "--lines",
            &lines.to_string(),
            "--format",
            "text",
        ])
        .output()
        .map_err(|error| format!("Failed to run Herdr agent read: {error}"))?;
    if !output.status.success() {
        return Err(output_error("agent read", &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).replace('\r', ""))
}

fn read_agent(bin: &str, target: &str) -> Result<String, String> {
    read_agent_lines(bin, target, read_lines())
}

pub fn agent_status(target: &str) -> Result<String, String> {
    if !is_valid_target(target) {
        return Err("Invalid Herdr agent name.".to_string());
    }
    let bin = herdr_path().ok_or_else(|| "Herdr provider is not installed.".to_string())?;
    let output = command(&bin)
        .args(["agent", "get", target])
        .output()
        .map_err(|error| format!("Failed to run Herdr agent get: {error}"))?;
    if !output.status.success() {
        return Err(output_error("agent get", &output));
    }
    parse_agent_status(&output.stdout)
}

fn parse_agent_status(output: &[u8]) -> Result<String, String> {
    let value: Value = serde_json::from_slice(output)
        .map_err(|error| format!("Herdr returned invalid agent JSON: {error}"))?;
    if value.get("error").is_some() {
        return Err(format!("Herdr agent get failed: {value}"));
    }
    value
        .pointer("/result/agent/agent_status")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "Herdr agent status is missing from the response.".to_string())
}

pub fn is_cokacdir_prompt_active(target: &str) -> bool {
    ACTIVE_PROMPTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(target)
        .copied()
        .unwrap_or(0)
        > 0
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut truncated = text.chars().take(max_chars).collect::<String>();
    truncated.push_str("\n…");
    truncated
}

pub fn completion_preview(target: &str) -> Result<String, String> {
    if !is_valid_target(target) {
        return Err("Invalid Herdr agent name.".to_string());
    }
    let bin = herdr_path().ok_or_else(|| "Herdr provider is not installed.".to_string())?;
    let output = read_agent_lines(&bin, target, COMPLETION_READ_LINES)?;
    let response = grok_final_response(&output, "")
        .or_else(|| codex_final_response(&output))
        .unwrap_or_else(|| {
            output
                .lines()
                .rev()
                .take(30)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string()
        });
    Ok(truncate_chars(&response, COMPLETION_PREVIEW_CHARS))
}

pub fn read_screen(model: Option<&str>) -> Result<(String, String), String> {
    let target = target_from_model(model)?;
    let bin = herdr_path().ok_or_else(|| "Herdr provider is not installed.".to_string())?;
    let output = command(&bin)
        .args([
            "agent",
            "read",
            &target,
            "--source",
            "visible",
            "--lines",
            &screen_lines().to_string(),
            "--format",
            "text",
        ])
        .output()
        .map_err(|error| format!("Failed to run Herdr agent read: {error}"))?;
    if !output.status.success() {
        return Err(output_error("screen read", &output));
    }
    let screen = String::from_utf8_lossy(&output.stdout)
        .replace('\r', "")
        .trim_end()
        .to_string();
    Ok((target, screen))
}

fn snapshot_delta(before: &str, after: &str) -> String {
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    let max_overlap = before_lines.len().min(after_lines.len());
    for overlap in (1..=max_overlap).rev() {
        if before_lines[before_lines.len() - overlap..] == after_lines[..overlap] {
            return after_lines[overlap..].join("\n").trim().to_string();
        }
    }
    after.trim().to_string()
}

fn is_tui_separator(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("─ Worked for ")
        || line.starts_with("Worked for ")
        || line.starts_with("Help improve")
        || line.starts_with('╭')
        || line.starts_with("Shift+Tab")
        || (line.chars().count() >= 20 && line.chars().all(|character| character == '─'))
}

fn take_until_separator(output: &str) -> String {
    output
        .lines()
        .take_while(|line| !is_tui_separator(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_codex_prompt_line(line: &str) -> bool {
    line.trim_start().starts_with('›')
}

fn is_codex_status_footer(line: &str) -> bool {
    let trimmed = line.trim();
    (trimmed.starts_with("gpt-") && trimmed.contains('·'))
        || (trimmed.contains("~/") && trimmed.contains('·'))
}

fn is_codex_tool_or_notice(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("• Ran ")
        || trimmed.starts_with("• Explored")
        || trimmed.starts_with("• Thought")
        || trimmed.starts_with("• Model changed")
        || trimmed.starts_with('⚠')
}

fn take_until_codex_end(output: &str) -> String {
    let mut lines = output.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };
    let rest = lines.take_while(|line| !is_codex_prompt_line(line) && !is_codex_status_footer(line));
    std::iter::once(first).chain(rest).collect::<Vec<_>>().join("\n")
}

fn each_line(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut start = 0;
    std::iter::from_fn(move || {
        if start >= text.len() {
            return None;
        }
        let rest = &text[start..];
        let offset = start;
        start = match rest.find('\n') {
            Some(index) => start + index + 1,
            None => text.len(),
        };
        let line = if start > offset && text.as_bytes().get(start - 1) == Some(&b'\n') {
            &text[offset..start - 1]
        } else {
            &text[offset..]
        };
        Some((offset, line))
    })
}

fn strip_tui_right_gutter(line: &str) -> &str {
    line.trim_end_matches(|character: char| character == '█' || character.is_whitespace())
}

fn strip_trailing_clock(line: &str) -> &str {
    let trimmed = strip_tui_right_gutter(line);
    let Some(without_meridiem) = trimmed
        .strip_suffix(" AM")
        .or_else(|| trimmed.strip_suffix(" PM"))
    else {
        return trimmed;
    };
    let without_meridiem = without_meridiem.trim_end();
    let Some(colon) = without_meridiem.rfind(':') else {
        return trimmed;
    };
    let minutes = &without_meridiem[colon + 1..];
    if minutes.len() != 2 || !minutes.bytes().all(|b| b.is_ascii_digit()) {
        return trimmed;
    }
    let before_colon = &without_meridiem[..colon];
    let Some(hour_break) = before_colon.rfind(|character: char| !character.is_ascii_digit()) else {
        return trimmed;
    };
    let hour = &before_colon[hour_break + 1..];
    if hour.is_empty() || hour.len() > 2 {
        return trimmed;
    }
    let prefix = &before_colon[..=hour_break];
    if prefix.chars().rev().take_while(|character| *character == ' ').count() < 2 {
        return trimmed;
    }
    prefix.trim_end()
}

fn normalize_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_box_drawing_line(line: &str) -> bool {
    matches!(
        line.trim_start().chars().next(),
        Some('┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '│' | '─' | '╭' | '╮' | '╰' | '╯')
    )
}

fn is_grok_input_box_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("│ ❯")
        || trimmed.starts_with("│❯")
        || trimmed.starts_with('╭')
        || trimmed.starts_with('╰')
}

fn is_grok_chrome_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    matches!(trimmed.chars().next(), Some('◆' | '❙' | '┃' | '◈'))
        || trimmed.starts_with("Thought for ")
        || trimmed == "▼"
        || trimmed == "▲"
}

fn clean_grok_content_line(line: &str) -> String {
    let without_bar = strip_tui_right_gutter(line)
        .trim_end_matches('│')
        .trim_end();
    strip_trailing_clock(without_bar).trim().to_string()
}

fn is_grok_status_bar(line: &str) -> bool {
    let trimmed = line.trim();
    (trimmed.contains("K / ") && (trimmed.contains('✓') || trimmed.contains('%')))
        || (trimmed.starts_with('~') && trimmed.contains('/'))
        || trimmed.contains('▲')
}

fn is_herdr_unwrap_duplicate(line: &str) -> bool {
    line.trim_start().starts_with('┆')
}

fn is_outer_quote_frame(line: &str) -> bool {
    let trimmed = line.trim();
    let starts_frame = trimmed.starts_with('┌') || trimmed.starts_with('└');
    starts_frame && !trimmed.contains('┬') && !trimmed.contains('┴')
}

fn unwrap_grok_quote_line(line: &str) -> Option<String> {
    let cleaned = clean_grok_content_line(line);
    if cleaned.is_empty() {
        return Some(String::new());
    }
    if is_herdr_unwrap_duplicate(&cleaned) || is_outer_quote_frame(&cleaned) || is_grok_status_bar(&cleaned)
    {
        return None;
    }
    if let Some(inner) = cleaned.strip_prefix('│') {
        let inner = inner.trim_end_matches('│').trim();
        if inner.starts_with('┌')
            || inner.starts_with('├')
            || inner.starts_with('└')
            || inner.starts_with('│')
        {
            return Some(inner.to_string());
        }
        let pipe_count = cleaned.chars().filter(|character| *character == '│').count();
        let is_table = pipe_count > 2
            || cleaned.contains('┼')
            || cleaned.contains('┬')
            || cleaned.contains('├');
        if !is_table {
            return Some(inner.to_string());
        }
    }
    Some(cleaned)
}

fn looks_like_grok_tui(text: &str) -> bool {
    let grok_marks = text.contains('❯')
        || text.lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("◆ ") || trimmed.starts_with("Thought for ")
        });
    grok_marks && !text.lines().any(|line| line.trim_start().starts_with('›'))
}

fn grok_echo_text(line: &str) -> String {
    prompt_echo_text(line)
}

fn prompt_echo_text(line: &str) -> String {
    let trimmed = line.trim_start();
    let rest = if let Some(rest) = trimmed.strip_prefix("❯ ") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("› ") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("> ") {
        rest
    } else {
        trimmed
    };
    normalize_ws(strip_trailing_clock(rest))
}

fn find_marked_prompt_offset(after: &str, first_line: &str, marker: &str) -> Option<usize> {
    let expected = compact_chars(first_line);
    if expected.is_empty() {
        return None;
    }
    let mut found = None;
    for (offset, line) in each_line(after) {
        if is_grok_input_box_line(line) {
            continue;
        }
        let leading = line.chars().take_while(|character| *character == ' ').count();
        if marker == "> " && leading > 2 {
            continue;
        }
        let trimmed = line.trim_start();
        if !trimmed.starts_with(marker) {
            continue;
        }
        let echoed = compact_chars(&prompt_echo_text(line));
        if echoed.is_empty() {
            continue;
        }
        if expected.starts_with(&echoed) || echoed.starts_with(&expected) {
            found = Some(offset);
        }
    }
    found
}

fn slice_after_prompt_echo(region: &str, prompt: &str) -> String {
    let lines: Vec<&str> = region.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let start = consume_prompt_echo(&lines, prompt);
    lines.get(start..).unwrap_or(&[]).join("\n")
}

fn grok_line_matches_prompt(line: &str, prompt_first: &str) -> bool {
    let echoed = grok_echo_text(line);
    !echoed.is_empty() && (prompt_first.starts_with(&echoed) || echoed.starts_with(prompt_first))
}

fn following_is_real_grok_turn(text: &str, offset: usize) -> bool {
    let window: Vec<&str> = text[offset..].lines().take(16).collect();
    let joined = window.join("\n");
    let hook = joined.find("◆ ").or_else(|| joined.find("Thought for "));
    let replay = joined.find('▲');
    match (hook, replay) {
        (Some(hook_at), Some(replay_at)) => hook_at < replay_at,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => false,
    }
}

fn find_grok_prompt_offset(after: &str, first_line: &str) -> Option<usize> {
    let mut first = None;
    let mut with_hooks = None;
    for (offset, line) in each_line(after) {
        if is_grok_input_box_line(line) {
            continue;
        }
        if !grok_line_matches_prompt(line, first_line) {
            continue;
        }
        if first.is_none() {
            first = Some(offset);
        }
        if following_is_real_grok_turn(after, offset) {
            with_hooks = Some(offset);
        }
    }
    with_hooks.or(first)
}

fn grok_skip_wrapped_prompt(lines: &[&str]) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let indent = lines[0].chars().take_while(|character| *character == ' ').count();
    let mut index = 1;
    while index < lines.len() {
        let line = lines[index];
        if line.trim().is_empty() || is_tui_separator(line) || is_grok_chrome_line(line) {
            break;
        }
        let spaces = line.chars().take_while(|character| *character == ' ').count();
        if spaces <= indent || line.trim_start().starts_with('❯') {
            break;
        }
        index += 1;
    }
    index
}

fn compact_chars(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn is_truncated_prompt_echo(text: &str) -> bool {
    let trimmed = text.trim_end();
    trimmed.ends_with('…') || trimmed.ends_with("...")
}

fn consume_prompt_echo(lines: &[&str], prompt: &str) -> usize {
    let expected = compact_chars(prompt);
    if expected.is_empty() {
        return grok_skip_wrapped_prompt(lines);
    }
    let mut seen = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 && (is_tui_separator(line) || is_grok_chrome_line(line)) {
            return index;
        }
        let piece = grok_echo_text(line);
        if piece.is_empty() {
            if !seen.is_empty() && seen.len() >= expected.len() {
                return index;
            }
            continue;
        }
        let truncated = is_truncated_prompt_echo(&piece);
        let compact_piece = compact_chars(piece.trim_end_matches('…').trim_end_matches("..."));
        seen.push_str(&compact_piece);
        if truncated || seen == expected || seen.starts_with(&expected) {
            return index + 1;
        }
        if !expected.starts_with(&seen) {
            return if index == 0 { 1 } else { index };
        }
    }
    1.min(lines.len())
}

fn grok_final_response(turn_output: &str, prompt: &str) -> Option<String> {
    if !looks_like_grok_tui(turn_output) {
        return None;
    }
    let prompt_first = prompt.lines().next().unwrap_or("").trim();
    let start = if prompt_first.is_empty() {
        let mut found = None;
        for (offset, line) in each_line(turn_output) {
            if is_grok_input_box_line(line) || !line.contains("❯ ") {
                continue;
            }
            if grok_echo_text(line).is_empty() {
                continue;
            }
            if found.is_none() {
                found = Some(offset);
            }
            if following_is_real_grok_turn(turn_output, offset) {
                found = Some(offset);
            }
        }
        found
    } else {
        find_grok_prompt_offset(turn_output, prompt_first)
    };
    let region = start.map(|offset| &turn_output[offset..]).unwrap_or(turn_output);
    let lines: Vec<&str> = region.lines().collect();
    if lines.is_empty() {
        return None;
    }
    let body_start = if lines[0].contains("❯ ") {
        consume_prompt_echo(&lines, prompt)
    } else {
        0
    };
    let mut body = Vec::new();
    for line in lines.iter().skip(body_start) {
        if is_tui_separator(line) || is_grok_input_box_line(line) {
            break;
        }
        if line.contains("❯ ") {
            continue;
        }
        if is_grok_status_bar(line) || is_herdr_unwrap_duplicate(line) {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if body.last().is_some_and(|entry: &String| !entry.is_empty()) {
                body.push(String::new());
            }
            continue;
        }
        if is_grok_chrome_line(line) {
            continue;
        }
        let Some(cleaned) = unwrap_grok_quote_line(line) else {
            continue;
        };
        if cleaned == "▼" || cleaned == "▲" || cleaned.is_empty() {
            if cleaned.is_empty()
                && body.last().is_some_and(|entry: &String| !entry.is_empty())
            {
                body.push(String::new());
            }
            continue;
        }
        if body.last().is_some_and(|entry| entry == &cleaned) {
            continue;
        }
        body.push(cleaned);
    }
    while body.last().is_some_and(String::is_empty) {
        body.pop();
    }
    let output = strip_legacy_markers(&body.join("\n"));
    if output.trim().is_empty() {
        None
    } else {
        Some(output)
    }
}

fn extract_final_response(turn_output: &str, prompt: &str) -> String {
    grok_final_response(turn_output, prompt)
        .or_else(|| codex_final_response(turn_output))
        .unwrap_or_else(|| {
            let stripped = strip_legacy_markers(turn_output);
            unwrap_terminal_text(stripped.lines().map(str::to_string))
        })
}

fn current_turn_output(before: &str, after: &str, prompt: &str) -> String {
    let prompt = prompt.trim();
    if !prompt.is_empty() {
        let first_line = prompt.lines().next().unwrap_or("").trim();
        if !first_line.is_empty() {
            if let Some(index) = find_marked_prompt_offset(after, first_line, "› ") {
                return slice_after_prompt_echo(&take_until_codex_end(&after[index..]), prompt);
            }
            if let Some(index) = find_marked_prompt_offset(after, first_line, "> ") {
                return slice_after_prompt_echo(&take_until_separator(&after[index..]), prompt);
            }
            if let Some(index) = find_grok_prompt_offset(after, first_line) {
                return take_until_separator(&after[index..]);
            }
        }
    }
    snapshot_delta(before, after)
}

fn strip_legacy_markers(response: &str) -> String {
    response
        .lines()
        .filter(|line| {
            let line = line.trim();
            !line.starts_with("COKACDIR_RESPONSE_BEGIN_")
                && !line.starts_with("COKACDIR_RESPONSE_END_")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn is_ordered_list_item(line: &str) -> bool {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0
        && line
            .get(digits..)
            .is_some_and(|suffix| suffix.starts_with(". ") || suffix.starts_with(") "))
}

fn is_markdown_block_start(line: &str) -> bool {
    let is_indented_code = line.starts_with('\t') || line.starts_with("    ");
    let line = line.trim_start();
    line.starts_with("```")
        || line.starts_with("~~~")
        || line.starts_with("# ")
        || line.starts_with("## ")
        || line.starts_with("### ")
        || line.starts_with("#### ")
        || line.starts_with("##### ")
        || line.starts_with("###### ")
        || line.starts_with("- ")
        || line.starts_with("* ")
        || line.starts_with("+ ")
        || line.starts_with("• ")
        || line.starts_with("> ")
        || line.starts_with('|')
        || is_box_drawing_line(line)
        || is_indented_code
        || is_ordered_list_item(line)
}

fn prevents_wrapped_continuation(line: &str) -> bool {
    let is_indented_code = line.starts_with('\t') || line.starts_with("    ");
    let line = line.trim_start();
    line.starts_with('#')
        || line.starts_with('>')
        || line.starts_with('|')
        || line.starts_with("```")
        || line.starts_with("~~~")
        || is_box_drawing_line(line)
        || is_indented_code
}

fn unwrap_terminal_text(lines: impl IntoIterator<Item = String>) -> String {
    let mut output: Vec<String> = Vec::new();
    let mut in_code_fence = false;

    for raw_line in lines {
        let line = raw_line.trim_end();
        let trimmed = line.trim();
        let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");

        if in_code_fence {
            output.push(line.to_string());
            if is_fence {
                in_code_fence = false;
            }
            continue;
        }

        if trimmed.is_empty() {
            if output.last().is_some_and(|line| !line.is_empty()) {
                output.push(String::new());
            }
            continue;
        }

        if is_fence {
            output.push(line.to_string());
            in_code_fence = true;
            continue;
        }

        let starts_block = is_markdown_block_start(line);
        let can_join = !starts_block
            && output.last().is_some_and(|previous| {
                !previous.is_empty() && !prevents_wrapped_continuation(previous)
            });

        match (can_join, output.last_mut()) {
            (true, Some(previous)) => {
                previous.push(' ');
                previous.push_str(trimmed);
            }
            _ => output.push(line.to_string()),
        }
    }

    while output.last().is_some_and(String::is_empty) {
        output.pop();
    }
    output.join("\n")
}

fn codex_final_response(turn_output: &str) -> Option<String> {
    let mut response_lines = Vec::new();
    for line in turn_output.lines() {
        if is_codex_prompt_line(line) || is_codex_status_footer(line) {
            break;
        }
        if is_codex_tool_or_notice(line) {
            continue;
        }
        let trimmed = line.trim_start();
        if line.starts_with(' ') || line.starts_with('\t') || trimmed.starts_with('•') {
            response_lines.push(trimmed);
        } else if line.trim().is_empty() {
            response_lines.push("");
        }
    }

    let cleaned = response_lines.join("\n");
    let output = strip_legacy_markers(&cleaned);

    if output.trim().is_empty() {
        None
    } else {
        Some(output)
    }
}

fn interrupt_agent(bin: &str, target: &str) {
    let _ = command(bin)
        .args(["agent", "send-keys", target, "ctrl+c"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub fn execute_command_streaming(
    prompt: &str,
    sender: Sender<StreamMessage>,
    cancel_token: Option<std::sync::Arc<CancelToken>>,
    model: Option<&str>,
) -> Result<(), String> {
    let bin = herdr_path()
        .ok_or_else(|| "Herdr CLI not found. Set COKAC_HERDR_PATH or install herdr.".to_string())?;
    let target = target_from_model(model)?;
    let _active_prompt = ActivePromptGuard::new(&target);
    let before = read_agent(&bin, &target)?;

    let mut prompt_command = command(&bin);
    prompt_command
        .args([
            "agent",
            "prompt",
            &target,
            prompt,
            "--wait",
            "--until",
            "idle",
            "--until",
            "done",
            "--until",
            "blocked",
            "--timeout",
            &timeout_ms().to_string(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    detach_into_own_pgroup(&mut prompt_command);
    attach_cancel_cgroup(&mut prompt_command, cancel_token.as_ref());
    let mut child = prompt_command
        .spawn()
        .map_err(|error| format!("Failed to run Herdr agent prompt: {error}"))?;

    if let Some(token) = cancel_token.as_ref() {
        let mut child_pid = token
            .child_pid
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *child_pid = Some(child.id());
        drop(child_pid);
        if token.cancelled.load(Ordering::Relaxed) {
            kill_child_tree(&mut child);
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|error| format!("Failed waiting for Herdr agent: {error}"))?;

    if let Some(token) = cancel_token.as_ref() {
        let mut child_pid = token
            .child_pid
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *child_pid = None;
        if token.cancelled.load(Ordering::Relaxed) {
            drop(child_pid);
            interrupt_agent(&bin, &target);
            return Ok(());
        }
    }

    if !output.status.success() {
        return Err(output_error("agent prompt", &output));
    }
    let prompt_result: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Herdr returned invalid prompt JSON: {error}"))?;
    if prompt_result.get("error").is_some() {
        return Err(format!("Herdr agent prompt failed: {prompt_result}"));
    }

    let after = read_agent(&bin, &target)?;
    let turn_output = current_turn_output(&before, &after, prompt);
    let response = extract_final_response(&turn_output, prompt);
    if response.trim().is_empty() {
        return Err("Herdr agent completed without readable terminal output.".to_string());
    }

    send_success_terminal(&sender, Some(response.clone()), response, None)
        .map_err(|error| format!("Failed to publish Herdr response: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_models_and_targets() {
        assert!(is_herdr_model(Some("herdr")));
        assert!(is_herdr_model(Some("herdr:worker_1")));
        assert!(!is_herdr_model(Some("codex")));
        assert_eq!(strip_herdr_prefix("herdr:worker"), Some("worker"));
        assert!(is_valid_target("worker-1"));
        assert!(!is_valid_target("Worker"));
        assert!(!is_valid_target("../worker"));
    }

    #[test]
    fn extracts_wrapped_codex_long_prompt() {
        let after = concat!(
            "› old prompt\n",
            "• old answer\n",
            "› 다른 설명 없이 아래 한 줄만 그대로 출력해: QA_LONG_LINE 시작 이 문장은 의도적으로 매우 길어서 터미널에서 여러 줄로 접힐 수 있지만 의미상으로는 하나의 문장이어야 한다.\n",
            "  중간에 빈 줄이나 목록을 넣지 말고 이 문장만 출력한다. QA_LONG_LINE 끝\n",
            "\n",
            "• QA_LONG_LINE 시작 이 문장은 의도적으로 매우 길어서 터미널에서 여러 줄로 접힐 수 있지만 의미상으로는 하나의 문장이어야 한다. 중간에 빈 줄이나 목록을 넣지 말고 이 문장만\n",
            "  출력한다. QA_LONG_LINE 끝\n",
            "\n",
            "› Run /review on my current changes\n",
        );
        let prompt = "다른 설명 없이 아래 한 줄만 그대로 출력해: QA_LONG_LINE 시작 이 문장은 의도적으로 매우 길어서 터미널에서 여러 줄로 접힐 수 있지만 의미상으로는 하나의 문장이어야 한다. 중간에 빈 줄이나 목록을 넣지 말고 이 문장만 출력한다. QA_LONG_LINE 끝";
        let extracted = extract_final_response(&current_turn_output("", after, prompt), prompt);
        assert!(extracted.contains("QA_LONG_LINE 시작"), "{}", extracted);
        assert!(extracted.contains("QA_LONG_LINE 끝"), "{}", extracted);
        assert!(!extracted.contains("old answer"), "{}", extracted);
        assert!(!extracted.contains("Run /review"), "{}", extracted);
    }

    #[test]
    fn extracts_wrapped_agy_long_prompt() {
        let after = concat!(
            "> 헤르메스 에이전트에 대해서 궁금한 것이 있어\n",
            "  old body\n",
            "> 다른 설명 없이 아래 한 줄만 그대로 출력해: QA_LONG_LINE 시작 이 문장은 의도적으로 매우 길어서 터미널에서 여러 줄로 접힐 수 있지만 의미상으로는 하나의 문장이어야 한다.\n",
            "  중간에 빈 줄이나 목록을 넣지 말고 이 문장만 출력한다. QA_LONG_LINE 끝\n",
            "\n",
            "  QA_LONG_LINE 시작 이 문장은 의도적으로 매우 길어서 터미널에서 여러 줄로 접힐 수 있지만 의미상으로는 하나의 문장이어야 한다. 중간에 빈 줄이나 목록을 넣지 말고 이\n",
            "  문장만 출력한다. QA_LONG_LINE 끝\n",
            "────────────────────────────────────────────────────────────\n",
        );
        let prompt = "다른 설명 없이 아래 한 줄만 그대로 출력해: QA_LONG_LINE 시작 이 문장은 의도적으로 매우 길어서 터미널에서 여러 줄로 접힐 수 있지만 의미상으로는 하나의 문장이어야 한다. 중간에 빈 줄이나 목록을 넣지 말고 이 문장만 출력한다. QA_LONG_LINE 끝";
        let extracted = extract_final_response(&current_turn_output("", after, prompt), prompt);
        assert!(extracted.contains("QA_LONG_LINE 시작"), "{}", extracted);
        assert!(!extracted.contains("old body"), "{}", extracted);
        assert!(!extracted.contains("헤르메스"), "{}", extracted);
    }

    #[test]
    fn extracts_codex_bullet_answer_without_composer() {
        let before = "› old prompt\n\n• old answer\n";
        let after = concat!(
            "› Reply with exactly CODEX_PARSE_OK on the first line, then a 2-row markdown table with columns A,B and rows 1,x and 2,y. No other text.\n",
            "\n",
            "• CODEX_PARSE_OK\n",
            "\n",
            "   A      B\n",
            "  ━━━━━  ━━━━━\n",
            "   1      x\n",
            "  ─────  ─────\n",
            "   2      y\n",
            "\n",
            "› Run /review on my current changes\n",
            "\n",
            "  gpt-5.6-luna max · ~/dev/md-rag-api\n",
        );
        let prompt = "Reply with exactly CODEX_PARSE_OK on the first line, then a 2-row markdown table with columns A,B and rows 1,x and 2,y. No other text.";
        let turn = current_turn_output(before, after, prompt);
        let extracted = extract_final_response(&turn, prompt);
        assert!(extracted.starts_with("• CODEX_PARSE_OK") || extracted.starts_with("CODEX_PARSE_OK"), "{}", extracted);
        assert!(extracted.contains("1      x"), "{}", extracted);
        assert!(!extracted.contains("Run /review"), "{}", extracted);
        assert!(!extracted.contains("gpt-5.6-luna"), "{}", extracted);
        assert!(grok_final_response(&turn, prompt).is_none());
    }

    #[test]
    fn extracts_codex_final_response() {
        let snapshot = "  상태를 확인하겠습니다.\n\n  - 첫 번째 항목\n  - 두 번째 항목";
        assert_eq!(
            codex_final_response(snapshot).as_deref(),
            Some("상태를 확인하겠습니다.\n\n- 첫 번째 항목\n- 두 번째 항목")
        );
    }

    const GROK_PROBE_PROMPT: &str =
        "Reply with exactly the token GROK_HERDR_PROBE_OK and nothing else.";

    const GROK_PROBE_AFTER: &str = "\n  /tmp/grok-herdr-probe                                               14K / 500K\n\n\n     ❯ Reply with exactly the token GROK_HERDR_PROBE_OK and nothing    6:07 PM\n       else.\n\n\n     ◆ user_prompt_submit  [hooks: 2]\n     ◆ Thought for 0.0s\n\n     GROK_HERDR_PROBE_OK                                               6:07 PM\n\n     Worked for 2.4s                                          stop  [hooks: 2]\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n  Help improve Grok                                           [Opt out] [Opt in]\n  Off by default. Opt-in to allow SpaceXAI to retain coding\n  data, e.g., prompts, traces, & metrics, for training and\n  debugging purposes. Change anytime via settings.\n  Read Terms and Privacy Policy.\n\n  ╭────────────────────────────────────────────────────────────────────────────╮\n  │ ❯                                                                          │\n  ╰───────────────────────────────────────── Grok 4.6 (high) · always-approve ─╯\n\n  Shift+Tab:mode  │  Ctrl+.:shortcuts\n";

    const GROK_PROBE_BEFORE: &str = "\n  /tmp/grok-herdr-probe\n\n\n\n\n                                  ⠀⠀⠀⠀⠀⠀⣀⣀⡀⠀⠀⠀⢀⠄\n                New worktree                                 ctrl+w\n                Resume session                               ctrl+s\n\n  Help improve Grok                                           [Opt out] [Opt in]\n  Off by default. Opt-in to allow SpaceXAI to retain coding\n  data, e.g., prompts, traces, & metrics, for training and\n  debugging purposes. Change anytime via settings.\n  Read Terms and Privacy Policy.\n\n  ╭────────────────────────────────────────────────────────────────────────────╮\n  │ ❯                                                                          │\n  ╰───────────────────────────────────────── Grok 4.6 (high) · always-approve ─╯\n\n                                                      Grok Build  1.0.4 [stable]\n";

    #[test]
    fn strips_grok_clock_only_after_wide_padding() {
        assert_eq!(
            strip_trailing_clock("     GROK_HERDR_PROBE_OK                                               6:07 PM"),
            "     GROK_HERDR_PROBE_OK"
        );
        assert_eq!(
            strip_trailing_clock("meet at 6:07 PM"),
            "meet at 6:07 PM"
        );
    }

    #[test]
    fn ignores_grok_input_box_when_finding_prompt() {
        assert!(is_grok_input_box_line(
            "  │ ❯                                                                          │"
        ));
        assert!(!is_grok_input_box_line(
            "     ❯ Reply with exactly the token GROK_HERDR_PROBE_OK and nothing    6:07 PM"
        ));
        let offset = find_grok_prompt_offset(GROK_PROBE_AFTER, GROK_PROBE_PROMPT)
            .expect("user prompt line");
        assert!(
            GROK_PROBE_AFTER[offset..].starts_with("     ❯ Reply with exactly"),
            "offset {} should start at the user prompt line",
            offset
        );
        assert!(
            !GROK_PROBE_AFTER[..offset].contains("❯ Reply"),
            "should pick the last user prompt, not a later input box"
        );
    }

    #[test]
    fn extracts_grok_probe_without_chrome() {
        assert_eq!(
            grok_final_response(GROK_PROBE_AFTER, GROK_PROBE_PROMPT).as_deref(),
            Some("GROK_HERDR_PROBE_OK")
        );
        assert_eq!(
            grok_final_response(GROK_PROBE_AFTER, "").as_deref(),
            Some("GROK_HERDR_PROBE_OK")
        );
    }

    #[test]
    fn extracts_grok_turn_from_before_after_snapshots() {
        let turn = current_turn_output(GROK_PROBE_BEFORE, GROK_PROBE_AFTER, GROK_PROBE_PROMPT);
        assert_eq!(
            extract_final_response(&turn, GROK_PROBE_PROMPT),
            "GROK_HERDR_PROBE_OK"
        );
        assert!(!extract_final_response(&turn, GROK_PROBE_PROMPT).contains("Help improve"));
        assert!(!extract_final_response(&turn, GROK_PROBE_PROMPT).contains("user_prompt_submit"));
    }

    #[test]
    fn extracts_grok_multiline_answer_and_skips_tool_chrome() {
        let snapshot = concat!(
            "     ❯ 현재 프로젝트 확인해봐                                          6:01 PM\n",
            "\n",
            "     ◆ Thought for 1.2s\n",
            "     ❙  ◈ Read 6 files  [hooks: 14]\n",
            "     ┃  ◆ Run Inspect running containers\n",
            "\n",
            "     이 저장소는 브라우저 Compose 프로젝트가 아닙니다.\n",
            "\n",
            "     남은 후속 작업\n",
            "\n",
            "     • P1 독립 외부 백업\n",
            "\n",
            "     Worked for 1m15s                                          stop  [hooks: 2]\n",
            "\n",
            "  Help improve Grok\n",
            "  ╭────────────────────────────────────────╮\n",
            "  │ ❯                                      │\n",
            "  ╰──────────── Grok 4.6 (high) · always-approve ─╯\n",
        );
        assert_eq!(
            grok_final_response(snapshot, "현재 프로젝트 확인해봐").as_deref(),
            Some("이 저장소는 브라우저 Compose 프로젝트가 아닙니다.\n\n남은 후속 작업\n\n• P1 독립 외부 백업")
        );
    }

    #[test]
    fn strips_grok_scrollbar_before_clock() {
        assert_eq!(
            strip_trailing_clock(
                "     ┌───────┬──────────┬───────────┐                                  6:38 PM   █"
            ),
            "     ┌───────┬──────────┬───────────┐"
        );
        assert!(!is_grok_input_box_line(
            "     │ 항목  │ 값       │ 비고      │                                            █"
        ));
        assert!(is_grok_input_box_line(
            "  │ ❯                                                                          │"
        ));
    }

    #[test]
    fn extracts_grok_box_table_without_stopping_at_rows() {
        let snapshot = concat!(
            "     ❯ 마크다운 테이블 보내봐                                          6:38 PM\n",
            "\n",
            "     ◆ user_prompt_submit  [hooks: 2]                                            █\n",
            "     ◆ Thought for 0.0s                                                          █\n",
            "                                                                                 █\n",
            "     ┌───────┬──────────┬───────────┐                                  6:38 PM   █\n",
            "     │ 항목  │ 값       │ 비고      │                                            █\n",
            "     ├───────┼──────────┼───────────┤                                            █\n",
            "     │ 상태  │ 정상     │ 확인 완료 │                                            █\n",
            "     └───────┴──────────┴───────────┘\n",
            "                                         ▼\n",
            "  Help improve Grok\n",
            "  ╭────────────────────────────────────────╮\n",
            "  │ ❯                                      │\n",
            "  ╰──────────── Grok 4.6 (high) · always-approve ─╯\n",
        );
        let extracted =
            grok_final_response(snapshot, "마크다운 테이블 보내봐").expect("grok table");
        assert!(extracted.starts_with("┌───────┬──────────┬───────────┐"));
        assert!(extracted.contains("│ 항목  │ 값       │ 비고"));
        assert!(extracted.contains("│ 상태  │ 정상     │ 확인 완료"));
        assert!(extracted.ends_with("└───────┴──────────┴───────────┘"));
        assert!(!extracted.contains('█'));
        assert!(!extracted.contains("6:38 PM"));
        assert!(!extracted.contains("Help improve"));
    }

    #[test]
    fn prefers_real_turn_over_sticky_prompt_header() {
        let snapshot = concat!(
            "     ❯ 그럼이것을 200명에게 서비스 한다고 하면 서버 사양은?             7:37 PM\n",
            "     ◆ user_prompt_submit  [hooks: 2]\n",
            "     200명은 현재 설계 범위 밖이라 나눠 계산하겠습니다.\n",
            "     ┌────────────────────────────────────────┐\n",
            "     │   1) 지금처럼 200명 브라우저를 항상 켜 둠 │\n",
            "     │   2) 계정 200명, 동시 20명               │\n",
            "     │   • 한 대로 밀려면 RAM 256GiB            │\n",
            "     ~/hermes-agent                          122K / 500K │ 3/3 ✓\n",
            "     ❯ 그럼이것을 200명에게 서비스 한다고 하면 서버 사양은?             7:37 PM\n",
            "                                                                                     ▲\n",
            "     │   3) 브라우저 없이 Agent만 200명         │\n",
            "     └────────────────────────────────────────┘\n",
            "     Worked for 1m5s                                          stop  [hooks: 2]\n",
        );
        let prompt = "그럼이것을 200명에게 서비스 한다고 하면 서버 사양은?";
        let extracted = grok_final_response(snapshot, prompt).expect("sticky header");
        assert!(extracted.contains("200명은 현재 설계 범위 밖"), "{}", extracted);
        assert!(extracted.contains("1) 지금처럼 200명"), "{}", extracted);
        assert!(extracted.contains("한 대로 밀려면"), "{}", extracted);
        assert!(extracted.contains("3) 브라우저 없이"), "{}", extracted);
        assert!(!extracted.contains('▲'), "{}", extracted);
    }

    #[test]
    fn consumes_mid_word_wrapped_grok_prompt() {
        let snapshot = concat!(
            "     ❯ 다른 설명 없이 GitHub 파이프 마크다운 테이블만 출력. 열은 이    6:43 PM\n",
            "       름,수량,비고. 행은 사과 2 빨강, 배 1 노랑.\n",
            "\n",
            "     ◆ Thought for 0.0s\n",
            "     ┌──────┬──────┬──────┐                                            6:43 PM\n",
            "     │ 이름 │ 수량 │ 비고 │\n",
            "     │ 사과 │ 2    │ 빨강 │\n",
            "     └──────┴──────┴──────┘\n",
            "     Worked for 2.0s                                          stop  [hooks: 2]\n",
        );
        let prompt = "다른 설명 없이 GitHub 파이프 마크다운 테이블만 출력. 열은 이름,수량,비고. 행은 사과 2 빨강, 배 1 노랑.";
        let extracted = grok_final_response(snapshot, prompt).expect("wrapped prompt");
        assert!(
            extracted.starts_with("┌──────┬──────┬──────┐"),
            "{}",
            extracted
        );
        assert!(!extracted.contains("름,수량"));
        assert!(extracted.contains("│ 사과 │ 2    │ 빨강"));
    }

    #[test]
    fn consumes_truncated_grok_prompt_echo() {
        let snapshot = concat!(
            "     ❯ 다른 설명 없이 아래 10줄을 그대로 출력해:                       6:43 PM\n",
            "       L01 첫번째\n",
            "       L02 두번째 …\n",
            "\n",
            "     ◆ Thought for 0.0s\n",
            "     L01 첫번째 L02 두번째 L03 세번째 L04 네번째 L05 다섯번째 L06 여   6:43 PM\n",
            "     섯번째 L07 일곱번째 L08 여덟번째 L09 아홉번째 L10 열번째\n",
            "     Worked for 2.2s                                          stop  [hooks: 2]\n",
        );
        let prompt = "다른 설명 없이 아래 10줄을 그대로 출력해:\nL01 첫번째\nL02 두번째\nL03 세번째";
        let extracted = grok_final_response(snapshot, prompt).expect("truncated echo");
        assert!(
            extracted.starts_with("L01 첫번째 L02 두번째"),
            "{}",
            extracted
        );
        assert!(!extracted.contains('…'));
    }

    #[test]
    fn keeps_grok_wrapped_paragraphs_without_scrollbar() {
        let snapshot = concat!(
            "     ❯ 10줄 문구 보내봐                                                6:37 PM\n",
            "\n",
            "     한 줄씩 내려가며 읽는 짧은 문구입니다.\n",
            "\n",
            "     지금 이 문장은 두 번째 줄입니다. 세 번째는 조금 더 길게 이어집              █\n",
            "     니다. 네 번째에서 호흡을 한 번 고르고, 다섯 번째는 다시 짧게.               █\n",
            "     Worked for 6.0s                                          stop  [hooks: 2]\n",
            "  Help improve Grok\n",
        );
        let extracted = grok_final_response(snapshot, "10줄 문구 보내봐").expect("wrapped");
        assert_eq!(
            extracted,
            "한 줄씩 내려가며 읽는 짧은 문구입니다.\n\n지금 이 문장은 두 번째 줄입니다. 세 번째는 조금 더 길게 이어집\n니다. 네 번째에서 호흡을 한 번 고르고, 다섯 번째는 다시 짧게."
        );
        assert!(!extracted.contains('█'));
    }

    #[test]
    fn grok_extractor_does_not_claim_codex_output() {
        let snapshot = "  상태를 확인하겠습니다.\n\n  - 첫 번째 항목";
        assert_eq!(grok_final_response(snapshot, "상태를 확인").as_deref(), None);
        assert_eq!(
            extract_final_response(snapshot, "상태를 확인"),
            "상태를 확인하겠습니다.\n\n- 첫 번째 항목"
        );
    }
}
