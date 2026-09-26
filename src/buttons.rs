//! A tree whose layout costs something: many buttons, two columns, and the
//! content changing every frame.
//!
//! The tree in `sample.rs` is small enough that both engines finish in tens of
//! microseconds, which is useful for comparing *correctness* and useless for
//! asking what a layout costs a frame. This one is sized to be noticed: every
//! button is a container with a measured label plus an icon whose width changes
//! with the frame, every row is a different height, and nothing about the layout
//! repeats from one frame to the next.
//!
//! # Why the tree is shaped the way it is
//!
//! Three constructs are absent on purpose, because morphorm cannot express them
//! and a tree that used them would be comparing two different layouts rather
//! than two engines. The conformance test would catch that, and did:
//!
//! - **`flex_grow`.** CSS grows an item *beyond its own content*; morphorm's
//!   `Units::Stretch` divides the free space by factor *from zero*. With one
//!   grown item the two agree — which is why `sample.rs` can use it — and with
//!   two carrying different content they do not: two `flex_grow: 1` buttons whose
//!   labels differ came out 170 and 168 px wide under taffy and 169 and 169 under
//!   morphorm. Buttons here have an explicit width instead.
//! - **An auto cross axis with unequal children.** CSS stretches auto-cross
//!   children of a row to the tallest sibling; morphorm gives each its own
//!   content height. Rows here have an explicit height, so the stretch both
//!   engines perform is the one they agree on.
//! - **Two-column *grid*.** morphorm does not place grid items itself — it reads
//!   each child's `column_start` and `row_start` and defaults both to zero, so a
//!   grid with no explicit placement stacks everything in the first cell. Taffy
//!   generates that placement and this engine does not synthesise it. The two
//!   columns here are a column of rows, which is the flexbox path both engines
//!   implement identically.
//!
//! `gpui_morphorm.rs`'s module docs carry the same three as mapping limits.

use gpui_engine::{BoxedMeasureFn, Display, FlexDirection, LayoutEngine, LayoutId, MeasureContext};
use gpui_types::{AvailableSpace, Pixels, Size};

use crate::sample;

/// How many buttons share a row.
pub const COLUMNS: usize = 2;

/// The width the column of buttons is laid out in.
pub const CONTENT_WIDTH: f32 = 360.0;

const PADDING: f32 = 8.0;
const GAP: f32 = 6.0;
const LABEL_HEIGHT: f32 = 16.0;
/// The tallest a row gets, so a viewport can be sized without overflowing.
const MAX_ROW_HEIGHT: f32 = 36.0;
/// Two buttons and the gap between them exactly fill the content box, so the
/// widest each can be is decided by the container rather than by `flex_grow`.
const BUTTON_WIDTH: f32 = (CONTENT_WIDTH - PADDING * 2.0 - GAP) / COLUMNS as f32;

/// The window size, as the facade hands it to `stretch_auto_size_to_fill`.
pub fn viewport(count: usize) -> Size<Pixels> {
    let rows = count.div_ceil(COLUMNS);
    Size {
        width: Pixels(CONTENT_WIDTH),
        height: Pixels(rows as f32 * MAX_ROW_HEIGHT + rows.saturating_sub(1) as f32 * GAP + PADDING * 2.0),
    }
}

/// The same viewport as a `compute_layout` constraint.
pub fn available_space(count: usize) -> Size<AvailableSpace> {
    let viewport = viewport(count);
    Size {
        width: AvailableSpace::Definite(viewport.width),
        height: AvailableSpace::Definite(viewport.height),
    }
}

/// The height of a row, which changes with the frame so everything below it
/// moves.
fn row_height(row: usize, frame: usize) -> f32 {
    28.0 + ((row + frame) % 3) as f32 * 4.0
}

/// A label whose width changes with the button and the frame, the way a label
/// whose text changed would.
fn label(index: usize, frame: usize) -> BoxedMeasureFn {
    let width = 40.0 + ((index + frame) % 7) as f32 * 5.0;
    Box::new(
        move |_known: Size<Option<Pixels>>,
              _available: Size<AvailableSpace>,
              _context: &mut dyn MeasureContext| {
            Size {
                width: Pixels(width),
                height: Pixels(LABEL_HEIGHT),
            }
        },
    )
}

/// Builds `count` buttons in [`COLUMNS`] columns, with `frame` deciding what is
/// on them. Returns the nodes in pre-order, root first.
pub fn build(
    engine: &mut dyn LayoutEngine,
    count: usize,
    frame: usize,
    rem_size: Pixels,
    scale_factor: f32,
) -> Vec<LayoutId> {
    let mut rows: Vec<(LayoutId, Vec<(LayoutId, LayoutId, LayoutId)>)> =
        Vec::with_capacity(count.div_ceil(COLUMNS));

    for row_index in 0..count.div_ceil(COLUMNS) {
        let mut parts = Vec::with_capacity(COLUMNS);
        let mut buttons = Vec::with_capacity(COLUMNS);
        for column in 0..COLUMNS {
            let index = row_index * COLUMNS + column;
            if index >= count {
                break;
            }

            // The icon's width changes with the frame, so a button's contents are
            // never the same two frames running.
            let mut icon = sample::neutral(Display::Flex);
            icon.size = Size {
                width: sample::length(10.0 + ((index + frame) % 4) as f32 * 4.0),
                height: sample::length(LABEL_HEIGHT),
            };
            let icon = engine.request_layout(&icon, rem_size, scale_factor, &[]);

            let label = engine.request_measured_layout(
                &sample::neutral(Display::Block),
                rem_size,
                scale_factor,
                label(index, frame),
            );

            let mut button = sample::neutral(Display::Flex);
            button.flex_direction = FlexDirection::Row;
            button.gap = Size {
                width: sample::definite(GAP),
                height: sample::definite(GAP),
            };
            button.padding = sample::edges(sample::definite(6.0));
            button.size = Size {
                width: sample::length(BUTTON_WIDTH),
                height: sample::auto(),
            };
            let button = engine.request_layout(&button, rem_size, scale_factor, &[icon, label]);
            buttons.push(button);
            parts.push((button, icon, label));
        }

        let mut row = sample::neutral(Display::Flex);
        row.flex_direction = FlexDirection::Row;
        row.gap = Size {
            width: sample::definite(GAP),
            height: sample::definite(GAP),
        };
        // An explicit height, so the stretch the buttons do against it is the
        // kind both engines perform.
        row.size = Size {
            width: sample::auto(),
            height: sample::length(row_height(row_index, frame)),
        };
        let row_ids = buttons.clone();
        let row = engine.request_layout(&row, rem_size, scale_factor, &row_ids);
        rows.push((row, parts));
    }

    let row_ids: Vec<LayoutId> = rows.iter().map(|(row, _)| *row).collect();
    let mut root = sample::neutral(Display::Flex);
    root.flex_direction = FlexDirection::Column;
    root.gap = Size {
        width: sample::definite(GAP),
        height: sample::definite(GAP),
    };
    root.padding = sample::edges(sample::definite(PADDING));
    let root = engine.request_layout(&root, rem_size, scale_factor, &row_ids);

    // Pre-order: the root, then each row and everything under it. The order is
    // the contract between any two runs, as in `sample`, and it covers every node
    // the tree owns — the leaves included, since that is where content sizing
    // happens and the icons are the part that changes every frame.
    let mut ordered = Vec::with_capacity(1 + count * 3);
    ordered.push(root);
    for (row, parts) in rows {
        ordered.push(row);
        for (button, icon, label) in parts {
            ordered.push(button);
            ordered.push(icon);
            ordered.push(label);
        }
    }
    ordered
}
