//! Bounded native terminal accessibility. Output never changes keyboard focus.
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};

use crate::terminal::Screen;
use eframe::egui::{self, Response, Ui, UiBuilder, Vec2, accesskit};

const MAX_ROWS: usize = 128;
const MAX_COLUMNS: usize = 512;
const MAX_BYTES: usize = 32 * 1024;
const OUTPUT_INTERVAL: Duration = Duration::from_millis(100);
const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(2);
type SelectionPoints = ((usize, usize), (usize, usize));

#[derive(Clone, Default)]
struct Row {
    value: String,
    lengths: Vec<u8>,
    positions: Vec<f32>,
    widths: Vec<f32>,
    columns: Vec<(usize, usize)>,
}

#[derive(Clone)]
struct State {
    rows: Vec<Row>,
    cursor: Option<(usize, usize)>,
    selection: Option<SelectionPoints>,
    fingerprint: u64,
    published: Instant,
    announced: Instant,
    notice: String,
    announcement_number: u64,
    copy_mode: bool,
}

/// Adds a terminal and one text run per visible row, only while AccessKit is active.
/// `active` controls announcements, not keyboard focus. Caller retains its focus model.
/// For keyboard copy mode, supply the frozen screen with its selected cells/cursor.
pub fn expose(
    ui: &mut Ui,
    response: &Response,
    screen: &Screen,
    title: &str,
    active: bool,
    cell_size: Vec2,
    copy_mode: bool,
) {
    let ctx = ui.ctx().clone();
    if ctx
        .accesskit_node_builder(response.id, |node| {
            node.set_role(accesskit::Role::Terminal);
            node.set_label(format!(
                "Terminal: {}",
                title.chars().take(120).collect::<String>()
            ));
            node.set_read_only();
            node.add_action(accesskit::Action::Focus);
            node.set_live(accesskit::Live::Off);
        })
        .is_none()
    {
        return;
    }

    let now = Instant::now();
    let key = response.id.with("accessible-terminal-state");
    let old = ctx.data(|data| data.get_temp::<State>(key));
    let fingerprint = fingerprint(screen);
    let interactive = ctx.input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Text(_)
                    | egui::Event::Paste(_)
                    | egui::Event::Key { pressed: true, .. }
                    | egui::Event::PointerButton { .. }
            )
        })
    });
    let changed = old
        .as_ref()
        .is_none_or(|state| state.fingerprint != fingerprint || state.copy_mode != copy_mode);
    let publish = changed
        && (interactive
            || old
                .as_ref()
                .is_none_or(|state| now.duration_since(state.published) >= OUTPUT_INTERVAL));
    let state = if publish {
        let (rows, selection) = rows(screen, cell_size.x);
        let should_announce = active
            && old
                .as_ref()
                .is_some_and(|state| now.duration_since(state.announced) >= ANNOUNCE_INTERVAL);
        let latest = rows
            .iter()
            .rev()
            .find(|row| !row.value.trim().is_empty())
            .map(|row| {
                row.value
                    .chars()
                    .filter(|ch| !ch.is_control())
                    .take(120)
                    .collect::<String>()
            })
            .unwrap_or_default();
        // A prompt or repeated output can leave the same final excerpt. Keep each
        // coalesced notice distinct so native bridges announce its actual change.
        let announcement_number = old
            .as_ref()
            .map_or(0, |state| state.announcement_number)
            .wrapping_add(u64::from(should_announce));
        State {
            rows,
            cursor: screen.cursor,
            selection,
            fingerprint,
            published: now,
            announced: if should_announce {
                now
            } else {
                old.as_ref().map_or(now, |state| state.announced)
            },
            notice: if should_announce {
                if copy_mode {
                    format!("Copy mode selection updated {announcement_number}")
                } else {
                    format!("Terminal output update {announcement_number}: {latest}")
                }
            } else {
                old.as_ref()
                    .map_or_else(|| "Terminal ready".into(), |state| state.notice.clone())
            },
            announcement_number,
            copy_mode,
        }
    } else {
        if changed {
            ctx.request_repaint_after(OUTPUT_INTERVAL);
        }
        old.unwrap_or_else(|| unreachable!("The first enabled accessibility pass always publishes"))
    };

    let mut ids = Vec::with_capacity(state.rows.len());
    for (row_index, row) in state.rows.iter().enumerate() {
        let row_ui = ui.new_child(
            UiBuilder::new()
                .id(response.id.with(("accessible-row", row_index)))
                .accessibility_parent(response.id),
        );
        let id = row_ui.id();
        ids.push(id);
        ctx.accesskit_node_builder(id, |node| {
            node.set_role(accesskit::Role::TextRun);
            node.set_value(row.value.clone());
            node.set_character_lengths(row.lengths.clone());
            node.set_character_positions(row.positions.clone());
            node.set_character_widths(row.widths.clone());
            node.set_word_lengths(word_lengths(row));
            node.set_text_direction(accesskit::TextDirection::LeftToRight);
            node.set_bounds(accesskit::Rect {
                x0: response.rect.left().into(),
                y0: (response.rect.top() + row_index as f32 * cell_size.y).into(),
                x1: response.rect.right().into(),
                y1: (response.rect.top() + (row_index + 1) as f32 * cell_size.y).into(),
            });
        });
    }
    let position = |(row, column): (usize, usize), end: bool| {
        let content = state.rows.get(row)?;
        let id = ids.get(row)?;
        let index = content
            .columns
            .iter()
            .position(|&(start, stop)| column >= start && column < stop)
            .unwrap_or_else(|| content.lengths.len().saturating_sub(1));
        Some(accesskit::TextPosition {
            node: accesskit::NodeId(id.value()),
            character_index: (index + usize::from(end)).min(content.lengths.len()),
        })
    };
    let selection = if let Some((anchor, focus)) = state.selection {
        position(anchor, false).zip(position(focus, true))
    } else {
        state
            .cursor
            .and_then(|cursor| position(cursor, false).map(|cursor| (cursor, cursor)))
    };
    ctx.accesskit_node_builder(response.id, |node| {
        node.set_description(if copy_mode { "Keyboard copy mode. Terminal output is frozen for selection." } else { "Live terminal. Text is available to assistive technology; commands use the terminal keyboard." });
        if let Some((anchor, focus)) = selection { node.set_text_selection(accesskit::TextSelection { anchor, focus }); }
        else { node.clear_text_selection(); }
    });
    let status_ui = ui.new_child(
        UiBuilder::new()
            .id(response.id.with("accessible-output-status"))
            .accessibility_parent(response.id),
    );
    ctx.accesskit_node_builder(status_ui.id(), |node| {
        node.set_role(accesskit::Role::Status);
        node.set_value(state.notice.clone());
        node.set_live(if active {
            accesskit::Live::Polite
        } else {
            accesskit::Live::Off
        });
        node.set_live_atomic();
    });
    ctx.data_mut(|data| data.insert_temp(key, state));
}

fn fingerprint(screen: &Screen) -> u64 {
    let mut hash = DefaultHasher::new();
    screen.cursor.hash(&mut hash);
    screen.wrapped_rows.hash(&mut hash);
    for cell in screen.cells.iter().take(MAX_ROWS * MAX_COLUMNS) {
        (cell.row, cell.col, &cell.text, cell.wide, cell.selected).hash(&mut hash);
    }
    hash.finish()
}

fn rows(screen: &Screen, width: f32) -> (Vec<Row>, Option<SelectionPoints>) {
    let last = screen
        .cells
        .iter()
        .map(|cell| cell.row)
        .chain(screen.cursor.map(|cursor| cursor.0))
        .max()
        .unwrap_or(0)
        .min(MAX_ROWS - 1);
    let mut result = vec![Row::default(); last + 1];
    let mut budget = MAX_BYTES;
    let mut selected = None;
    for (index, row) in result.iter_mut().enumerate() {
        let mut column = 0;
        for cell in screen
            .cells
            .iter()
            .filter(|cell| cell.row == index && cell.col < MAX_COLUMNS)
        {
            if cell.col < column {
                continue;
            }
            while column < cell.col && budget > 1 {
                push(row, " ", column, 1, width);
                column += 1;
                budget -= 1;
            }
            let text: String = cell.text.chars().take(16).collect();
            if text.is_empty() || budget <= text.len() + 1 {
                break;
            }
            let span = if cell.wide { 2 } else { 1 };
            push(row, &text, cell.col, span, width);
            budget -= text.len();
            column = cell.col + span;
            if cell.selected {
                match &mut selected {
                    None => selected = Some(((index, cell.col), (index, cell.col))),
                    Some((_, focus)) => *focus = (index, cell.col),
                }
            }
        }
        if let Some((_, cursor_col)) = screen.cursor.filter(|(cursor_row, _)| *cursor_row == index)
        {
            while column <= cursor_col.min(MAX_COLUMNS - 1) && budget > 1 {
                push(row, " ", column, 1, width);
                column += 1;
                budget -= 1;
            }
        }
        if !screen.wrapped_rows.contains(&index) && budget > 0 {
            push(row, "\n", column, 0, width);
            budget -= 1;
        }
    }
    (result, selected)
}

fn push(row: &mut Row, text: &str, column: usize, span: usize, width: f32) {
    row.value.push_str(text);
    row.lengths.push(text.len() as u8);
    row.positions.push(column as f32 * width);
    row.widths.push(span as f32 * width);
    row.columns.push((column, column + span.max(1)));
}

fn word_lengths(row: &Row) -> Vec<u8> {
    let mut words = Vec::new();
    let mut byte = 0;
    let mut count = 0usize;
    let mut at_word_end = false;
    for &length in &row.lengths {
        let character = row.value[byte..].chars().next().unwrap_or(' ');
        let is_word = character.is_alphanumeric() || character == '_';
        if (is_word && at_word_end || count == 255) && count > 0 {
            words.push(count as u8);
            count = 0;
        }
        count += 1;
        at_word_end = !is_word;
        byte += usize::from(length);
    }
    if count > 0 {
        words.push(count as u8);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::Cell;
    use eframe::egui::Id;

    fn screen() -> Screen {
        let mut screen = Screen::empty();
        for (column, (text, wide)) in [("a", false), ("界", true), ("e\u{301}", false)]
            .into_iter()
            .enumerate()
        {
            screen.cells.push(Cell {
                row: 0,
                col: if column == 2 { 3 } else { column },
                text: text.into(),
                fg: [255; 3],
                bg: [0; 3],
                wide,
                underline: false,
                selected: false,
                search_match: false,
                search_current: false,
                hyperlink: None,
            });
        }
        screen.cursor = Some((0, 4));
        screen
    }
    fn frame(ctx: &egui::Context, screen: &Screen, focus_find: bool) -> accesskit::TreeUpdate {
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.interact(
                    egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(400.0, 200.0)),
                    Id::new("test-terminal"),
                    egui::Sense::click_and_drag(),
                );
                let mut query = String::new();
                let find = ui.add(egui::TextEdit::singleline(&mut query).id(Id::new("test-find")));
                ctx.memory_mut(|memory| {
                    memory.request_focus(if focus_find { find.id } else { response.id })
                });
                expose(
                    ui,
                    &response,
                    screen,
                    "Fixture",
                    !focus_find,
                    Vec2::new(8.0, 16.0),
                    false,
                );
            });
        });
        output
            .platform_output
            .accesskit_update
            .expect("AccessKit enabled")
    }

    #[test]
    fn native_tree_has_terminal_text_unicode_caret_and_selected_range() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut screen = screen();
        screen.cells[1].selected = true;
        let update = frame(&ctx, &screen, false);
        let id = accesskit::NodeId(Id::new("test-terminal").value());
        let tree = accesskit_consumer::Tree::new(update, true);
        let terminal = tree.state().node_by_id(id).unwrap();
        assert_eq!(terminal.role(), accesskit::Role::Terminal);
        assert!(terminal.supports_text_ranges());
        assert_eq!(terminal.document_range().text(), "a界e\u{301} \n");
        assert_eq!(terminal.text_selection().unwrap().text(), "界");
        assert_eq!(tree.state().focus_id(), Some(id));
        screen.cells[1].selected = false;
        let (rows, _) = rows(&screen, 8.0);
        assert_eq!(rows[0].lengths, vec![1, 3, 3, 1, 1]);
        assert_eq!(rows[0].widths, vec![8.0, 16.0, 8.0, 8.0, 0.0]);
    }

    #[test]
    fn output_updates_keep_stable_ids_and_never_steal_find_focus() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut screen = screen();
        let before = frame(&ctx, &screen, false);
        screen.cells[0].text = "b".into();
        let after = frame(&ctx, &screen, true);
        assert_eq!(after.focus, accesskit::NodeId(Id::new("test-find").value()));
        let ids = |update: &accesskit::TreeUpdate| {
            update
                .nodes
                .iter()
                .filter(|(_, node)| {
                    matches!(
                        node.role(),
                        accesskit::Role::Terminal
                            | accesskit::Role::TextRun
                            | accesskit::Role::Status
                    )
                })
                .map(|(id, _)| *id)
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(ids(&before), ids(&after));
        let tree = accesskit_consumer::Tree::new(after, true);
        assert_eq!(
            tree.state().focus_id(),
            Some(accesskit::NodeId(Id::new("test-find").value()))
        );
    }

    #[test]
    fn rows_preserve_soft_wraps_and_bound_untrusted_sparse_cells() {
        let mut screen = screen();
        screen.wrapped_rows.push(0);
        screen.cells.push(Cell {
            row: usize::MAX,
            col: usize::MAX,
            ..screen.cells[0].clone()
        });
        let (rows, _) = rows(&screen, 8.0);
        assert_eq!(rows.len(), MAX_ROWS);
        assert!(!rows[0].value.ends_with('\n'));
        assert!(rows.iter().map(|row| row.value.len()).sum::<usize>() <= MAX_BYTES);
        assert!(rows.iter().all(|row| row.lengths.len() <= MAX_COLUMNS + 1));
    }

    #[test]
    fn output_bursts_are_coalesced_and_announcements_are_not_per_cell() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut screen = screen();
        let initial = frame(&ctx, &screen, false);
        screen.cells[0].text = "z".into();
        let next = frame(&ctx, &screen, false);
        let content = |update: &accesskit::TreeUpdate| {
            update
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == accesskit::Role::TextRun)
                .map(|(_, node)| node.value().unwrap_or(""))
                .collect::<String>()
        };
        assert_eq!(content(&initial), content(&next));
        assert_eq!(
            next.nodes
                .iter()
                .filter(|(_, node)| node.live() == Some(accesskit::Live::Polite))
                .count(),
            1
        );

        let key = Id::new("test-terminal").with("accessible-terminal-state");
        let mut previous_notice = String::new();
        for value in ["b", "c"] {
            ctx.data_mut(|data| {
                let mut state = data.get_temp::<State>(key).unwrap();
                state.announced = Instant::now() - ANNOUNCE_INTERVAL;
                state.published = Instant::now() - OUTPUT_INTERVAL;
                data.insert_temp(key, state);
            });
            // A different earlier row with the same final nonempty row must
            // still deliver a new, bounded native live-region value.
            screen.cells[0].text = value.into();
            screen.cells.push(Cell {
                row: 1,
                col: 0,
                text: "$".into(),
                ..screen.cells[0].clone()
            });
            let update = frame(&ctx, &screen, false);
            let notice = update
                .nodes
                .iter()
                .find(|(_, node)| node.role() == accesskit::Role::Status)
                .unwrap()
                .1
                .value()
                .unwrap()
                .to_owned();
            assert_ne!(notice, previous_notice);
            assert!(notice.ends_with('$'));
            assert!(notice.len() < 200);
            previous_notice = notice;
            screen.cells.pop();
        }
    }
}
