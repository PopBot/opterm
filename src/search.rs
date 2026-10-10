//! Bounded, transient search of the live terminal grid. No transcript index.
use alacritty_terminal::{
    event::EventListener,
    grid::Dimensions,
    index::{Column, Line, Point},
    term::{Term, cell::Flags},
};
use anyhow::{Result, ensure};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const MAX_QUERY_BYTES: usize = 4096;
pub const MAX_SCAN_CELLS: usize = 262_144;
pub const MAX_SCAN_BYTES: usize = 1024 * 1024;
pub const MAX_MATCHES: usize = 2000;
pub const MAX_ALL_SESSIONS: usize = 256;
pub const MAX_ALL_CELLS: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Literal,
    Regex,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Case {
    #[default]
    Sensitive,
    Insensitive,
    Smart,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Options {
    pub mode: Mode,
    pub case: Case,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Summary {
    pub count: usize,
    /// One-based ordinal in oldest-to-newest order; None for read-only searches.
    pub current: Option<usize>,
    /// True when the scan, match, or time budget was reached. Count is a lower bound.
    pub truncated: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionResult {
    pub id: u64,
    pub summary: Summary,
    pub preview: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AllSummary {
    pub sessions: Vec<SessionResult>,
    pub truncated: bool,
}

pub fn compile(query: &str, options: Options) -> Result<Option<Regex>> {
    ensure!(
        query.len() <= MAX_QUERY_BYTES,
        "Search text exceeds 4096 bytes"
    );
    if query.is_empty() {
        return Ok(None);
    }
    let pattern = match options.mode {
        Mode::Literal => regex::escape(query),
        Mode::Regex => query.to_owned(),
    };
    let insensitive = match options.case {
        Case::Sensitive => false,
        Case::Insensitive => true,
        Case::Smart => !query.chars().any(char::is_uppercase),
    };
    Ok(Some(
        RegexBuilder::new(&pattern)
            .case_insensitive(insensitive)
            .multi_line(true)
            .size_limit(1024 * 1024)
            .dfa_size_limit(256 * 1024)
            .build()
            .map_err(|error| anyhow::anyhow!("Invalid search expression: {error}"))?,
    ))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Match {
    pub start: Point,
    pub end: Point,
}
impl Match {
    pub fn contains(self, point: Point) -> bool {
        self.start <= point && point <= self.end
    }
}
#[derive(Clone, Copy)]
struct Offset {
    byte: u32,
    end_byte: u32,
    line: i32,
    col: u32,
}
impl Offset {
    fn point(self) -> Point {
        Point::new(Line(self.line), Column(self.col as usize))
    }
}
pub(crate) struct Scan {
    pub matches: Vec<Match>,
    pub truncated: bool,
    pub preview: String,
}

impl Scan {
    pub fn contains(&self, point: Point) -> bool {
        let index = self.matches.partition_point(|found| found.start <= point);
        index > 0 && self.matches[index - 1].contains(point)
    }
}

/// Prefer the newest whole physical lines when the live grid exceeds the budget.
/// Soft-wrap boundaries are joined; hard newlines remain searchable as newlines.
pub(crate) fn scan<T: EventListener>(
    term: &Term<T>,
    regex: &Regex,
    cells_budget: usize,
    visible_only: bool,
) -> Scan {
    let columns = term.columns();
    let bottom = if visible_only {
        Line(term.screen_lines() as i32 - term.grid().display_offset() as i32 - 1)
    } else {
        term.grid().bottommost_line()
    };
    let actual_top = if visible_only {
        Line(-(term.grid().display_offset() as i32))
    } else {
        term.grid().topmost_line()
    };
    let lines_budget = (cells_budget / columns.max(1)).clamp(1, 32_768);
    let top = Line(actual_top.0.max(bottom.0 - lines_budget as i32 + 1));
    let mut truncated = top > actual_top;
    let mut rows = Vec::new();
    let mut total_bytes = 0;
    let mut inspected = 0;
    // Work newest-to-oldest so byte truncation retains the latest output too.
    for line in (top.0..=bottom.0).rev() {
        let mut text = String::new();
        let mut offsets = Vec::new();
        let row = &term.grid()[Line(line)];
        let wraps = row[Column(columns - 1)].flags.contains(Flags::WRAPLINE);
        let last = if wraps {
            columns
        } else {
            (0..columns)
                .rfind(|&col| {
                    let cell = &row[Column(col)];
                    cell.c != ' ' || cell.zerowidth().is_some()
                })
                .map_or(0, |col| col + 1)
        };
        for col in 0..last {
            inspected += 1;
            if inspected > cells_budget || total_bytes + text.len() + 32 > MAX_SCAN_BYTES {
                truncated = true;
                break;
            }
            let cell = &row[Column(col)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let byte = text.len() as u32;
            text.push(if cell.flags.contains(Flags::HIDDEN) {
                ' '
            } else {
                cell.c
            });
            if let Some(zero) = cell.zerowidth() {
                for c in zero.iter().take(8) {
                    text.push(*c);
                }
                truncated |= zero.len() > 8;
            }
            offsets.push(Offset {
                byte,
                end_byte: text.len() as u32,
                line,
                col: col as u32,
            });
        }
        if !wraps {
            text.push('\n');
        }
        total_bytes += text.len();
        rows.push((text, offsets));
        if inspected >= cells_budget || total_bytes + 32 >= MAX_SCAN_BYTES {
            truncated |= line > actual_top.0;
            break;
        }
    }
    let mut text = String::with_capacity(total_bytes);
    let mut offsets = Vec::with_capacity(inspected.min(cells_budget));
    for (row, mut map) in rows.into_iter().rev() {
        let base = text.len() as u32;
        for entry in &mut map {
            entry.byte += base;
            entry.end_byte += base;
        }
        offsets.extend(map);
        text.push_str(&row);
    }
    let mut matches = Vec::new();
    let mut preview = String::new();
    let started = Instant::now();
    for found in regex.find_iter(&text) {
        if matches.len() == MAX_MATCHES || started.elapsed() > Duration::from_millis(80) {
            truncated = true;
            break;
        }
        // Zero-width expressions have no terminal cell to highlight/navigate.
        if found.is_empty() {
            continue;
        }
        // Only glyph byte intervals overlap a match. Hard newline bytes have no
        // corresponding cell; combining characters share their base cell's interval.
        let start = offsets.partition_point(|entry| (entry.end_byte as usize) <= found.start());
        let end = offsets.partition_point(|entry| (entry.byte as usize) < found.end());
        if start >= end {
            continue;
        }
        if let (Some(first), Some(last)) = (offsets.get(start), offsets.get(end - 1)) {
            matches.push(Match {
                start: first.point(),
                end: last.point(),
            });
            if preview.is_empty() {
                let before = text[..found.start()]
                    .char_indices()
                    .rev()
                    .nth(32)
                    .map_or(0, |(byte, _)| byte);
                preview = text[before..]
                    .chars()
                    .take(160)
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect();
            }
        }
    }
    Scan {
        matches,
        truncated,
        preview,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::{event::Event, term::Config, vte::ansi::Processor};
    struct Listener;
    impl EventListener for Listener {
        fn send_event(&self, _: Event) {}
    }
    struct Size {
        rows: usize,
        cols: usize,
    }
    impl Dimensions for Size {
        fn total_lines(&self) -> usize {
            self.rows
        }
        fn screen_lines(&self) -> usize {
            self.rows
        }
        fn columns(&self) -> usize {
            self.cols
        }
    }
    fn terminal(text: &str, cols: usize) -> Term<Listener> {
        let mut term = Term::new(Config::default(), &Size { rows: 4, cols }, Listener);
        let mut processor: Processor = Processor::new();
        processor.advance(&mut term, text.as_bytes());
        term
    }
    #[test]
    fn literal_regex_and_explicit_case_work_with_unicode() {
        let term = terminal("Foo foo FOO a+b[0] 界e\u{301}", 60);
        let options = Options::default();
        let literal = compile("a+b[0]", options).unwrap().unwrap();
        assert_eq!(
            scan(&term, &literal, MAX_SCAN_CELLS, false).matches.len(),
            1
        );
        let sensitive = compile("foo", options).unwrap().unwrap();
        assert_eq!(
            scan(&term, &sensitive, MAX_SCAN_CELLS, false).matches.len(),
            1
        );
        for case in [Case::Insensitive, Case::Smart] {
            let regex = compile(
                "foo",
                Options {
                    mode: Mode::Literal,
                    case,
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(scan(&term, &regex, MAX_SCAN_CELLS, false).matches.len(), 3);
        }
        let smart = compile(
            "Foo",
            Options {
                case: Case::Smart,
                ..options
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(scan(&term, &smart, MAX_SCAN_CELLS, false).matches.len(), 1);
        let regex = compile(
            "f.o",
            Options {
                mode: Mode::Regex,
                case: Case::Insensitive,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(scan(&term, &regex, MAX_SCAN_CELLS, false).matches.len(), 3);
        let unicode = compile("界e\u{301}", options).unwrap().unwrap();
        let result = scan(&term, &unicode, MAX_SCAN_CELLS, false);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(
            result.matches[0].end.column.0 - result.matches[0].start.column.0,
            2
        );
        assert!(result.preview.contains("界e\u{301}"));
    }
    #[test]
    fn soft_wrapping_and_hard_newlines_remain_distinct() {
        let term = terminal("cross-boundary\r\nhard\r\nline", 10);
        let regex = compile("cross-boundary", Options::default())
            .unwrap()
            .unwrap();
        let result = scan(&term, &regex, MAX_SCAN_CELLS, false);
        assert_eq!(result.matches.len(), 1);
        assert!(result.matches[0].start.line < result.matches[0].end.line);
        let regex = compile("hardline", Options::default()).unwrap().unwrap();
        assert!(
            scan(&term, &regex, MAX_SCAN_CELLS, false)
                .matches
                .is_empty()
        );
        let regex = compile(
            "hard\\nline",
            Options {
                mode: Mode::Regex,
                case: Case::Sensitive,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(scan(&term, &regex, MAX_SCAN_CELLS, false).matches.len(), 1);
    }
    #[test]
    fn regex_hard_newline_edges_map_only_to_overlapping_glyphs() {
        let term = terminal("first\r\nsecond", 12);
        let options = Options {
            mode: Mode::Regex,
            case: Case::Sensitive,
        };
        let regex = compile("\\nsec", options).unwrap().unwrap();
        let result = scan(&term, &regex, MAX_SCAN_CELLS, false);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].start, Point::new(Line(1), Column(0)));
        assert_eq!(result.matches[0].end, Point::new(Line(1), Column(2)));
        let regex = compile("st\\n", options).unwrap().unwrap();
        let result = scan(&term, &regex, MAX_SCAN_CELLS, false);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].start, Point::new(Line(0), Column(3)));
        assert_eq!(result.matches[0].end, Point::new(Line(0), Column(4)));
        let regex = compile("\\n", options).unwrap().unwrap();
        assert!(
            scan(&term, &regex, MAX_SCAN_CELLS, false)
                .matches
                .is_empty()
        );
    }

    #[test]
    fn budgets_invalid_patterns_and_zero_width_matches_are_bounded() {
        assert!(
            compile(
                "[",
                Options {
                    mode: Mode::Regex,
                    case: Case::Sensitive
                }
            )
            .is_err()
        );
        assert!(compile(&"x".repeat(MAX_QUERY_BYTES + 1), Options::default()).is_err());
        assert!(compile("", Options::default()).unwrap().is_none());
        let term = terminal("old\r\nnew\r\nlatest", 12);
        let regex = compile(
            "old|latest",
            Options {
                mode: Mode::Regex,
                case: Case::Sensitive,
            },
        )
        .unwrap()
        .unwrap();
        let result = scan(&term, &regex, 24, false);
        assert!(result.truncated);
        assert_eq!(result.matches.len(), 1);
        assert!(result.preview.contains("latest"));
        let regex = compile(
            "^",
            Options {
                mode: Mode::Regex,
                case: Case::Sensitive,
            },
        )
        .unwrap()
        .unwrap();
        assert!(
            scan(&term, &regex, MAX_SCAN_CELLS, false)
                .matches
                .is_empty()
        );
    }
}
