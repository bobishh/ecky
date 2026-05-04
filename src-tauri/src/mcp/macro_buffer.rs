use crate::models::{AppError, AppResult};
use sha2::{Digest, Sha256};

pub fn source_digest(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

pub fn number_lines(source: &str) -> String {
    source
        .lines()
        .enumerate()
        .map(|(idx, line)| format!("{}: {}", idx + 1, line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn replace_line_range(
    source: &str,
    start_line: usize,
    end_line: usize,
    replacement: &str,
) -> AppResult<String> {
    if start_line == 0 {
        return Err(AppError::validation("startLine must be 1 or greater."));
    }
    if end_line < start_line {
        return Err(AppError::validation(
            "endLine must be greater than or equal to startLine.",
        ));
    }

    let had_trailing_newline = source.ends_with('\n');
    let mut lines = source.lines().map(str::to_string).collect::<Vec<_>>();
    if start_line > lines.len() || end_line > lines.len() {
        return Err(AppError::validation(format!(
            "Line range {}-{} is outside buffer line count {}.",
            start_line,
            end_line,
            lines.len()
        )));
    }

    let replacement_lines = replacement.lines().map(str::to_string).collect::<Vec<_>>();
    lines.splice(start_line - 1..end_line, replacement_lines);

    let mut next = lines.join("\n");
    if had_trailing_newline {
        next.push('\n');
    }
    Ok(next)
}

pub fn assert_expected_digest(source: &str, expected_digest: &str) -> AppResult<()> {
    let actual = source_digest(source);
    if actual != expected_digest {
        return Err(AppError::validation(format!(
            "Buffer digest mismatch: expected {}, actual {}.",
            expected_digest, actual
        )));
    }
    Ok(())
}

pub fn apply_unified_patch(source: &str, patch: &str) -> AppResult<String> {
    let mut next = source.to_string();
    let mut lines = patch.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with("@@") {
            continue;
        }
        let (start_line, remove_count) = parse_hunk_header(line)?;
        let mut removed = Vec::new();
        let mut added = Vec::new();
        while let Some(peeked) = lines.peek().copied() {
            if peeked.starts_with("@@") {
                break;
            }
            let Some(hunk_line) = lines.next() else {
                break;
            };
            if let Some(rest) = hunk_line.strip_prefix('-') {
                removed.push(rest.to_string());
            } else if let Some(rest) = hunk_line.strip_prefix('+') {
                added.push(rest.to_string());
            } else if let Some(rest) = hunk_line.strip_prefix(' ') {
                removed.push(rest.to_string());
                added.push(rest.to_string());
            }
        }

        if removed.len() != remove_count {
            return Err(AppError::validation(format!(
                "Patch hunk expected {} removed/context lines, found {}.",
                remove_count,
                removed.len()
            )));
        }
        let end_line = start_line + remove_count.saturating_sub(1);
        let current = current_range_text(&next, start_line, end_line)?;
        let expected = removed.join("\n");
        if current != expected {
            return Err(AppError::validation(
                "Patch hunk context did not match buffer.",
            ));
        }
        next = replace_line_range(&next, start_line, end_line, &added.join("\n"))?;
    }
    Ok(next)
}

fn current_range_text(source: &str, start_line: usize, end_line: usize) -> AppResult<String> {
    if start_line == 0 || end_line < start_line {
        return Err(AppError::validation("Invalid line range."));
    }
    let lines = source.lines().collect::<Vec<_>>();
    if start_line > lines.len() || end_line > lines.len() {
        return Err(AppError::validation(
            "Patch hunk line range is outside buffer.",
        ));
    }
    Ok(lines[start_line - 1..end_line].join("\n"))
}

fn parse_hunk_header(line: &str) -> AppResult<(usize, usize)> {
    let old_range = line
        .split_whitespace()
        .find(|part| part.starts_with('-'))
        .ok_or_else(|| AppError::validation("Patch hunk is missing old range."))?;
    let old_range = old_range.trim_start_matches('-');
    let (start, count) = old_range
        .split_once(',')
        .map(|(start, count)| (start, count))
        .unwrap_or((old_range, "1"));
    let start = start
        .parse::<usize>()
        .map_err(|_| AppError::validation("Patch hunk start line is invalid."))?;
    let count = count
        .parse::<usize>()
        .map_err(|_| AppError::validation("Patch hunk line count is invalid."))?;
    Ok((start, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::AppErrorCode;

    #[test]
    fn line_range_replace_uses_one_based_inclusive_lines() {
        let source = "alpha\nbeta\ngamma\n";

        let next = replace_line_range(source, 2, 2, "BETA\nDELTA").expect("replace");

        assert_eq!(next, "alpha\nBETA\nDELTA\ngamma\n");
    }

    #[test]
    fn expected_digest_rejects_stale_edit() {
        let err = assert_expected_digest("alpha\n", "sha256:not-current")
            .expect_err("stale digest should fail");

        assert_eq!(err.code, AppErrorCode::Validation);
        assert!(err.message.contains("digest mismatch"));
    }

    #[test]
    fn unified_patch_replaces_matching_hunk() {
        let source = "alpha\nbeta\ngamma\n";
        let patch = "@@ -2,1 +2,2 @@\n-beta\n+BETA\n+DELTA";

        let next = apply_unified_patch(source, patch).expect("patch");

        assert_eq!(next, "alpha\nBETA\nDELTA\ngamma\n");
    }

    #[test]
    fn numbered_lines_are_one_based() {
        assert_eq!(number_lines("alpha\nbeta\n"), "1: alpha\n2: beta");
    }
}
