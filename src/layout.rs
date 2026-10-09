//! Window-local split geometry; session ownership remains in the service.
use eframe::egui::{self, Color32, Rect, Sense};

#[derive(Clone, Copy, PartialEq)]
pub enum Axis {
    Right,
    Below,
}

pub enum Layout {
    Pane(u64),
    Split {
        id: u64,
        axis: Axis,
        ratio: f32,
        first: Box<Layout>,
        second: Box<Layout>,
    },
}
impl Layout {
    pub fn ids(&self) -> Vec<u64> {
        match self {
            Self::Pane(id) => vec![*id],
            Self::Split { first, second, .. } => {
                let mut ids = first.ids();
                ids.extend(second.ids());
                ids
            }
        }
    }
    pub fn contains(&self, id: u64) -> bool {
        match self {
            Self::Pane(pane) => *pane == id,
            Self::Split { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }
    pub fn split(&mut self, target: u64, new: u64, axis: Axis) -> bool {
        match self {
            Self::Pane(id) if *id == target => {
                *self = Self::Split {
                    id: new,
                    axis,
                    ratio: 0.5,
                    first: Box::new(Self::Pane(target)),
                    second: Box::new(Self::Pane(new)),
                };
                true
            }
            Self::Split { first, second, .. } => {
                first.split(target, new, axis) || second.split(target, new, axis)
            }
            _ => false,
        }
    }
    pub fn retain(self, ids: &[u64]) -> Option<Self> {
        match self {
            Self::Pane(id) => ids.contains(&id).then_some(Self::Pane(id)),
            Self::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => match (first.retain(ids), second.retain(ids)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    id,
                    axis,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (first, second) => first.or(second),
            },
        }
    }
    /// Move the closest divider of this orientation along the focused pane's path.
    pub fn resize(&mut self, focused: u64, axis: Axis, delta: f32) -> bool {
        match self {
            Self::Split {
                axis: orientation,
                ratio,
                first,
                second,
                ..
            } => {
                let branch = if first.contains(focused) {
                    first
                } else if second.contains(focused) {
                    second
                } else {
                    return false;
                };
                if branch.resize(focused, axis, delta) {
                    return true;
                }
                if *orientation == axis {
                    *ratio = (*ratio + delta).clamp(0.1, 0.9);
                    return true;
                }
                false
            }
            _ => false,
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, panes: &mut Vec<(u64, Rect)>) {
        match self {
            Self::Pane(id) => panes.push((*id, rect)),
            Self::Split {
                id,
                axis,
                ratio,
                first,
                second,
            } => {
                let horizontal = *axis == Axis::Right;
                let length = if horizontal {
                    rect.width()
                } else {
                    rect.height()
                };
                let gap = length.min(5.0);
                let available = (length - gap).max(0.0);
                let minimum = (80.0 / available.max(1.0)).min(0.45);
                *ratio = ratio.clamp(minimum, 1.0 - minimum);
                let origin = if horizontal { rect.left() } else { rect.top() };
                let mut boundary = origin + available * *ratio;
                let divider = |at| {
                    if horizontal {
                        Rect::from_min_max(
                            egui::pos2(at, rect.top()),
                            egui::pos2(at + gap, rect.bottom()),
                        )
                    } else {
                        Rect::from_min_max(
                            egui::pos2(rect.left(), at),
                            egui::pos2(rect.right(), at + gap),
                        )
                    }
                };
                let response = ui
                    .interact(
                        divider(boundary),
                        egui::Id::new(("divider", *id)),
                        Sense::drag(),
                    )
                    .on_hover_cursor(if horizontal {
                        egui::CursorIcon::ResizeHorizontal
                    } else {
                        egui::CursorIcon::ResizeVertical
                    });
                if response.dragged()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let coordinate = if horizontal { pos.x } else { pos.y };
                    *ratio =
                        ((coordinate - origin) / available.max(1.0)).clamp(minimum, 1.0 - minimum);
                    boundary = origin + available * *ratio;
                }
                let divider = divider(boundary);
                let color = Color32::from_gray(if response.hovered() || response.dragged() {
                    130
                } else {
                    65
                });
                let center = divider.center();
                let points = if horizontal {
                    [
                        egui::pos2(center.x, rect.top()),
                        egui::pos2(center.x, rect.bottom()),
                    ]
                } else {
                    [
                        egui::pos2(rect.left(), center.y),
                        egui::pos2(rect.right(), center.y),
                    ]
                };
                ui.painter().line_segment(points, (1.0, color));
                let (a, b) = if horizontal {
                    (
                        Rect::from_min_max(rect.min, egui::pos2(divider.left(), rect.bottom())),
                        Rect::from_min_max(egui::pos2(divider.right(), rect.top()), rect.max),
                    )
                } else {
                    (
                        Rect::from_min_max(rect.min, egui::pos2(rect.right(), divider.top())),
                        Rect::from_min_max(egui::pos2(rect.left(), divider.bottom()), rect.max),
                    )
                };
                first.show(ui, a, panes);
                second.show(ui, b, panes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_nested_panes_collapses_only_their_branch() {
        let mut layout = Layout::Pane(1);
        assert!(layout.split(1, 2, Axis::Right));
        assert!(layout.split(2, 3, Axis::Below));
        let layout = layout.retain(&[1, 3]).unwrap();
        assert_eq!(layout.ids(), vec![1, 3]);
        let Layout::Split { axis, .. } = &layout else {
            panic!("Outer split was lost")
        };
        assert!(*axis == Axis::Right);
        assert_eq!(layout.retain(&[3]).unwrap().ids(), vec![3]);
    }
    #[test]
    fn keyboard_resize_changes_nearest_matching_divider() {
        let mut layout = Layout::Pane(1);
        layout.split(1, 2, Axis::Right);
        layout.split(2, 3, Axis::Below);
        assert!(layout.resize(3, Axis::Right, -0.05));
        assert!(layout.resize(3, Axis::Below, 0.05));
        let Layout::Split { ratio, second, .. } = layout else {
            unreachable!()
        };
        assert!((ratio - 0.45).abs() < 0.001);
        let Layout::Split { ratio, .. } = *second else {
            unreachable!()
        };
        assert!((ratio - 0.55).abs() < 0.001);
    }
}
