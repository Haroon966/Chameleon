//! Pure helpers and unit tests (no PTY, no crossterm).

use std::collections::HashMap;

/// Parses a hex color string "#rrggbb" or "rrggbb" into (r, g, b). Returns None if invalid.
pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some((r, g, b))
}

pub fn strip_code_blocks(text: &str) -> String {
    let text = text.trim().to_string();
    let text = text
        .strip_prefix("```")
        .and_then(|s| s.strip_suffix("```"))
        .map(|s| s.trim())
        .unwrap_or(text.as_str())
        .to_string();
    let text = text
        .strip_prefix("bash")
        .or_else(|| text.strip_prefix("sh"))
        .map(|s| s.trim())
        .unwrap_or(text.as_str())
        .to_string();
    text
}

/// Returns a short risk label if the command looks destructive or needs extra care.
pub fn destructive_command_hint(cmd: &str) -> Option<&'static str> {
    let t = cmd.trim().to_lowercase();
    if t.contains("rm -rf") || t.contains("rm -fr") {
        return Some("recursive delete (rm -rf)");
    }
    if t.contains("mkfs") {
        return Some("disk/filesystem format");
    }
    if t.contains("dd ") && t.contains(" of=/dev/") {
        return Some("raw block device write (dd)");
    }
    if t.contains(">/dev/sd") || t.contains("> /dev/sd") {
        return Some("block device overwrite");
    }
    if t.contains(":(){:|:&};:") {
        return Some("fork bomb");
    }
    None
}

/// From a terminal line (possibly with shell prompt), return the part that is the command to complete.
pub fn command_part_of_line(line: &str) -> &str {
    let line = line.trim();
    for sep in [" $ ", " # ", "> ", "] "] {
        if let Some(i) = line.rfind(sep) {
            return line[i + sep.len()..].trim_start();
        }
    }
    line.trim_start()
}

/// Given the full line prefix and the AI reply (full completed command), return the suffix to append.
pub fn completion_suffix_from_reply(full_prefix: &str, command_part: &str, reply: &str) -> String {
    let text = strip_code_blocks(&reply.trim().to_string());
    if text.is_empty() {
        return String::new();
    }
    if text.starts_with(command_part) {
        text[command_part.len()..].trim_start().to_string()
    } else if text.starts_with(full_prefix.trim()) {
        text[full_prefix.trim().len()..].trim_start().to_string()
    } else {
        text
    }
}

pub const ABBREVS: &[(&str, &str)] = &[
    ("gco", "git checkout "),
    ("gst", "git status "),
    ("gci", "git commit "),
    ("gbr", "git branch "),
    ("glog", "git log "),
    ("gdiff", "git diff "),
    ("gadd", "git add "),
    ("gpush", "git push "),
    ("gpull", "git pull "),
    ("gmerge", "git merge "),
    ("gfetch", "git fetch "),
    ("gshow", "git show "),
    ("grebase", "git rebase "),
    ("greset", "git reset "),
    ("ll", "ls -la "),
    ("la", "ls -la "),
];

pub fn merged_abbrev_map(user: &HashMap<String, String>) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = ABBREVS
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    for (k, v) in user {
        m.insert(k.clone(), v.clone());
    }
    m
}

pub fn expand_abbrev(word: &str, map: &HashMap<String, String>) -> Option<String> {
    map.get(word.trim()).cloned()
}

pub fn split_first_word(suffix: &str) -> (String, String) {
    let s = suffix.trim_start();
    let word: String = s.chars().take_while(|c| !c.is_whitespace()).collect();
    let after_word: String = s.chars().skip(word.len()).collect();
    let rest = after_word.trim_start().to_string();
    let to_inject = if after_word != rest {
        format!("{} ", word)
    } else {
        word
    };
    (to_inject, rest)
}

pub fn history_suggestion(history: &[String], command_part: &str) -> Option<String> {
    if command_part.is_empty() {
        return None;
    }
    for cmd in history {
        let cmd = cmd.trim();
        if cmd.starts_with(command_part) && cmd.len() > command_part.len() {
            return Some(cmd[command_part.len()..].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_accepts_hash() {
        assert_eq!(parse_hex("#ff00aa"), Some((255, 0, 170)));
        assert_eq!(parse_hex("00ff00"), Some((0, 255, 0)));
        assert_eq!(parse_hex("bad"), None);
    }

    #[test]
    fn command_part_strips_common_prompts() {
        assert_eq!(command_part_of_line("user@host:~/p $ ls -la"), "ls -la");
        assert_eq!(command_part_of_line("git status"), "git status");
    }

    #[test]
    fn completion_suffix_strips_prefix() {
        let s = completion_suffix_from_reply("x", "git ", "git log --oneline");
        assert_eq!(s, "log --oneline");
    }

    #[test]
    fn destructive_hint_rm_rf() {
        assert!(destructive_command_hint("sudo rm -rf /tmp/x").is_some());
        assert!(destructive_command_hint("echo ok").is_none());
    }

    #[test]
    fn merged_abbrev_user_overrides_builtin() {
        let mut u = HashMap::new();
        u.insert("gco".to_string(), "git checkout -b ".to_string());
        let m = merged_abbrev_map(&u);
        assert_eq!(m.get("gco").map(|s| s.as_str()), Some("git checkout -b "));
        assert!(m.contains_key("gst"));
    }

    #[test]
    fn history_suggestion_first_match_wins() {
        let h = vec!["git diff".into(), "git status --short".into()];
        assert_eq!(history_suggestion(&h, "git "), Some("diff".into()));
        assert_eq!(
            history_suggestion(&h, "git s"),
            Some("tatus --short".into())
        );
    }
}
