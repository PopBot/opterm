//! Explicitly activated links from visible terminal cells. Never opens a target itself.
use std::path::{Path, PathBuf};

use crate::terminal::{Cell, Screen};

const MAX_TARGET: usize = 4096;
const MAX_LOGICAL_LINE: usize = 8192;
const MAX_WRAPPED_ROWS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Url,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// Safe URL or local file path for an explicit Open/Copy gesture.
    pub target: String,
    pub kind: LinkKind,
}

/// Finds OSC 8 metadata first, then a bounded textual URL or local path under a cell.
/// Only rows marked as soft wraps are joined. Relative paths need an absolute cwd.
pub fn link_at(screen: &Screen, row: usize, col: usize, cwd: Option<&Path>) -> Option<Link> {
    let hovered = screen.cells.iter().find(|cell| covers(cell, row, col))?;
    if let Some(uri) = &hovered.hyperlink {
        return validate_target(uri, cwd);
    }
    let mut first = row;
    let mut last = row;
    while first > 0 && screen.wrapped_rows.contains(&(first - 1)) {
        first -= 1;
        if row - first >= MAX_WRAPPED_ROWS {
            return None;
        }
    }
    while screen.wrapped_rows.contains(&last) {
        last = last.checked_add(1)?;
        if last - first >= MAX_WRAPPED_ROWS {
            return None;
        }
    }
    let mut chars = Vec::new();
    let mut hit = None;
    for visible_row in first..=last {
        let mut column = 0;
        // Renderable content is sorted by row and column by the terminal kernel.
        for cell in screen.cells.iter().filter(|cell| cell.row == visible_row) {
            if cell.col < column {
                continue;
            }
            if cell.col.saturating_sub(column) + chars.len() > MAX_LOGICAL_LINE {
                return None;
            }
            chars.extend(std::iter::repeat_n(' ', cell.col - column));
            if covers(cell, row, col) {
                hit = Some(chars.len());
            }
            chars.extend(cell.text.chars());
            if chars.len() > MAX_LOGICAL_LINE {
                return None;
            }
            column = cell.col + if cell.wide { 2 } else { 1 };
        }
    }
    let hit = hit?;
    if chars
        .get(hit)
        .is_none_or(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    let mut start = hit;
    let mut end = hit + 1;
    while start > 0 && !boundary(chars[start - 1]) {
        start -= 1;
    }
    while end < chars.len() && !boundary(chars[end]) {
        end += 1;
    }
    while start < end && "([{<\"'`".contains(chars[start]) {
        start += 1;
    }
    while let Some(&tail) = chars.get(end.saturating_sub(1)).filter(|_| end > start) {
        let unmatched = match tail {
            ')' => unbalanced(&chars[start..end], '(', ')'),
            ']' => unbalanced(&chars[start..end], '[', ']'),
            '}' => unbalanced(&chars[start..end], '{', '}'),
            _ => false,
        };
        if unmatched || ".,;!>\"'`".contains(tail) {
            end -= 1;
        } else {
            break;
        }
    }
    // A click on surrounding punctuation must not activate the enclosed target.
    if hit < start || hit >= end {
        return None;
    }
    validate_target(&chars[start..end].iter().collect::<String>(), cwd)
}

fn covers(cell: &Cell, row: usize, col: usize) -> bool {
    cell.row == row && col >= cell.col && col < cell.col + if cell.wide { 2 } else { 1 }
}
fn boundary(c: char) -> bool {
    c.is_whitespace() || c.is_control()
}
fn unbalanced(chars: &[char], open: char, close: char) -> bool {
    chars.iter().filter(|&&c| c == close).count() > chars.iter().filter(|&&c| c == open).count()
}

pub fn validate_target(target: &str, cwd: Option<&Path>) -> Option<Link> {
    if target.is_empty() || target.len() > MAX_TARGET || target.chars().any(char::is_control) {
        return None;
    }
    let (scheme, rest) = target.split_once(':').unwrap_or(("", target));
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        let authority = rest.strip_prefix("//")?.split(['/', '?', '#']).next()?;
        if authority.is_empty()
            || target.chars().any(char::is_whitespace)
            || target.contains('\\')
            || authority.starts_with(':')
            || authority.ends_with('@')
            || decoded_has_control(target)
        {
            return None;
        }
        return Some(Link {
            target: target.to_owned(),
            kind: LinkKind::Url,
        });
    }
    if scheme.eq_ignore_ascii_case("mailto") {
        let address = rest.split('?').next()?;
        if !address.contains('@')
            || address.starts_with('@')
            || address.ends_with('@')
            || target.chars().any(char::is_whitespace)
            || decoded_has_control(target)
        {
            return None;
        }
        return Some(Link {
            target: target.to_owned(),
            kind: LinkKind::Url,
        });
    }
    let path = if scheme.eq_ignore_ascii_case("file") {
        let rest = rest.strip_prefix("//")?;
        let (authority, path) = rest.split_once('/')?;
        if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
            return None;
        }
        let decoded = percent_decode(&format!("/{path}"))?;
        if decoded.chars().any(char::is_control) {
            return None;
        }
        #[cfg(windows)]
        let decoded = if decoded.as_bytes().get(2) == Some(&b':') {
            decoded[1..].to_owned()
        } else {
            decoded
        };
        PathBuf::from(decoded)
    } else {
        // Schemes and shell-like words are never passed to the platform opener.
        let path = Path::new(target);
        if !path.is_absolute()
            && !target.starts_with("./")
            && !target.starts_with("../")
            && !(scheme.is_empty() && target.contains('/') && !target.starts_with('-'))
        {
            return None;
        }
        #[cfg(not(windows))]
        if !scheme.is_empty() && !target.starts_with('/') {
            return None;
        }
        path.to_path_buf()
    };
    let path = if path.is_absolute() {
        path
    } else {
        cwd.filter(|p| p.is_absolute())?.join(path)
    };
    Some(Link {
        target: path.to_string_lossy().into_owned(),
        kind: LinkKind::File,
    })
}

fn decoded_has_control(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.windows(3).any(|part| {
        part[0] == b'%'
            && hex(part[1])
                .zip(hex(part[2]))
                .is_some_and(|(a, b)| (a * 16 + b).is_ascii_control())
    })
}
fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
fn percent_decode(value: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut input = value.bytes();
    while let Some(byte) = input.next() {
        bytes.push(if byte == b'%' {
            hex(input.next()?)? * 16 + hex(input.next()?)?
        } else {
            byte
        });
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screen(rows: &[&str], wraps: &[usize]) -> Screen {
        let mut screen = Screen::empty();
        screen.wrapped_rows = wraps.to_vec();
        for (row, text) in rows.iter().enumerate() {
            for (col, ch) in text.chars().enumerate() {
                screen.cells.push(Cell {
                    row,
                    col,
                    text: ch.to_string(),
                    fg: [255; 3],
                    bg: [0; 3],
                    wide: false,
                    underline: false,
                    selected: false,
                    search_match: false,
                    search_current: false,
                    hyperlink: None,
                });
            }
        }
        screen
    }
    #[test]
    fn punctuation_is_trimmed_but_balanced_url_parentheses_remain() {
        let screen = screen(&["See (https://example.com/a(b))."], &[]);
        assert_eq!(
            link_at(&screen, 0, 10, None).unwrap().target,
            "https://example.com/a(b)"
        );
        assert!(link_at(&screen, 0, 4, None).is_none());
        assert!(link_at(&screen, 0, 29, None).is_none());
    }
    #[test]
    fn only_soft_wraps_join_and_hover_on_either_row_matches() {
        let joined = screen(&["https://exam", "ple.com/one"], &[0]);
        assert_eq!(
            link_at(&joined, 0, 2, None).unwrap().target,
            "https://example.com/one"
        );
        assert_eq!(
            link_at(&joined, 1, 3, None).unwrap().target,
            "https://example.com/one"
        );
        let separate = screen(&["https://exam", "ple.com/one"], &[]);
        assert_eq!(
            link_at(&separate, 0, 2, None).unwrap().target,
            "https://exam"
        );
        assert!(link_at(&separate, 1, 3, None).is_none());
    }
    #[test]
    fn explicit_osc8_metadata_overrides_label_and_rejects_unsafe_scheme() {
        let mut screen = screen(&["click"], &[]);
        screen.cells[2].hyperlink = Some("https://example.com".into());
        assert_eq!(
            link_at(&screen, 0, 2, None).unwrap().target,
            "https://example.com"
        );
        screen.cells[2].hyperlink = Some("javascript:alert(1)".into());
        assert!(link_at(&screen, 0, 2, None).is_none());
    }
    #[test]
    fn wide_character_second_column_maps_to_same_url() {
        let mut screen = screen(&["https://example.com/界"], &[]);
        screen.cells.last_mut().unwrap().wide = true;
        let col = screen.cells.last().unwrap().col + 1;
        assert_eq!(
            link_at(&screen, 0, col, None).unwrap().target,
            "https://example.com/界"
        );
    }
    #[test]
    fn local_paths_need_safe_cwd_and_file_uris_are_decoded() {
        assert_eq!(
            validate_target("/tmp/a", None).unwrap().kind,
            LinkKind::File
        );
        assert!(validate_target("src/main.rs", None).is_none());
        assert_eq!(
            validate_target("./src/main.rs", Some(Path::new("/tmp")))
                .unwrap()
                .target,
            "/tmp/./src/main.rs"
        );
        assert_eq!(
            validate_target("file:///tmp/a%20b", None).unwrap().target,
            "/tmp/a b"
        );
        assert!(validate_target("file://remote/tmp/a", None).is_none());
        assert!(validate_target("file:///tmp/%00bad", None).is_none());
        assert!(validate_target("file:///tmp/%GG", None).is_none());
    }
    #[test]
    fn unsafe_targets_and_unbounded_lines_are_rejected() {
        for target in [
            "javascript:alert(1)",
            "data:text/html,test",
            "command:rm",
            "https:///bad",
            "https://bad\n.example",
            "https://example.com/%0a",
        ] {
            assert!(validate_target(target, None).is_none(), "{target:?}");
        }
        assert!(validate_target("mailto:hello@example.com", None).is_some());
        let long = "https://example.com/".to_owned() + &"a".repeat(MAX_LOGICAL_LINE);
        assert!(link_at(&screen(&[&long], &[]), 0, 2, None).is_none());
    }
}
