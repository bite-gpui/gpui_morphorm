//! The swap, at the facade, under load.
//!
//! `Application::with_layout_engine` takes a factory, so an engine is chosen at
//! startup, before any window exists, and the authoring layer never names it.
//!
//! ```sh
//! cargo run                              # morphorm, 50 changing buttons
//! BITE_LAYOUT_ENGINE=taffy cargo run     # the facade's default, same tree
//! BITE_BUTTONS=1000 cargo run            # make the layout unmissable
//! ```
//!
//! The readout is the frame pipeline's own layout phase, so the two engines can
//! be compared on screen rather than in a table. It is the *phase*, not the whole
//! frame: `paint` is reported beside it because the pipeline budgets them
//! separately.
//!
//! Every frame is a different frame — the icons and the row heights change with
//! it, so nothing about the layout repeats and neither engine can serve a stale
//! result. That is also why the loop never goes quiet.
//!
//! Two things are worth watching for, and both are the mapping's documented
//! limits rather than bugs in the window. The two buttons in a row divide their
//! row differently under each engine, because CSS grows an item beyond its own
//! content while morphorm's `Stretch` divides the free space from zero — so a
//! wider label makes a wider button on one and not on the other. And nothing
//! here uses a grid, because morphorm does not place grid items itself.
//!
//! Needs a display and a GPU, so it is not part of the automated suite: the
//! engine's correctness is `tests/`, and its cost on an identical tree is
//! `benches/`.

use gpui::*;
use gpui_morphorm::MorphormLayoutEngine;
use std::cell::RefCell;
use std::rc::Rc;

/// Which engine to run. `morphorm` is the swap; anything else leaves the facade's
/// own engine in place, which is taffy.
const ENGINE_VAR: &str = "BITE_LAYOUT_ENGINE";
/// How many buttons to lay out.
const BUTTONS_VAR: &str = "BITE_BUTTONS";
/// How many buttons share a row.
const COLUMNS: usize = 2;

struct Demo {
    engine: &'static str,
    buttons: usize,
    frame: usize,
    metrics: Rc<RefCell<PhaseMetrics>>,
}

/// The height of a row, changing with the frame so everything below it moves.
fn row_height(row: usize, frame: usize) -> f32 {
    28.0 + ((row + frame) % 3) as f32 * 4.0
}

/// One button: an icon whose width changes with the frame, and a label.
fn button(index: usize, frame: usize) -> Div {
    div()
        .flex()
        .gap_1()
        .p_2()
        .flex_grow(1.0)
        .rounded_sm()
        .bg(rgb(0x1c2a20))
        .child(
            div()
                .w(px(10.0 + ((index + frame) % 4) as f32 * 4.0))
                .h(px(16.0))
                .rounded_sm()
                .bg(if index % 3 == 0 {
                    rgb(0xc77158)
                } else {
                    rgb(0x35493b)
                }),
        )
        .child(
            div()
                .h(px(16.0))
                .text_color(rgb(0xfaf6f0))
                .child(label(index, frame)),
        )
}

/// A label whose text length changes with the button and the frame.
fn label(index: usize, frame: usize) -> String {
    "■".repeat(2 + (index + frame) % 6)
}

impl Render for Demo {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // Advance first, so this frame draws the state it is about to report, and
        // keep asking for frames: the button content changes every one.
        self.frame = self.frame.wrapping_add(1);
        window.request_animation_frame();

        let readout = {
            let metrics = self.metrics.borrow();
            let frames = metrics.frames.max(1) as f32;
            format!(
                "{} buttons · {} · layout+prepaint {:.2} ms/frame · paint {:.2} ms/frame · {} frames",
                self.buttons,
                self.engine,
                metrics.layout.as_secs_f32() * 1000.0 / frames,
                metrics.paint.as_secs_f32() * 1000.0 / frames,
                metrics.frames,
            )
        };

        let rows = (0..self.buttons.div_ceil(COLUMNS)).map(|row| {
            div()
                .flex()
                .gap_1()
                .h(px(row_height(row, self.frame)))
                .children((0..COLUMNS).filter_map(|column| {
                    let index = row * COLUMNS + column;
                    (index < self.buttons).then(|| button(index, self.frame))
                }))
        });

        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .size_full()
            .overflow_hidden()
            .bg(rgb(0x121813))
            .text_sm()
            .text_color(rgb(0xa8a29e))
            .child(readout)
            .children(rows)
    }
}

fn main() {
    let requested = std::env::var(ENGINE_VAR).unwrap_or_else(|_| "morphorm".to_string());
    let buttons = std::env::var(BUTTONS_VAR)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(50);
    let engine: &'static str = if requested == "morphorm" {
        "morphorm"
    } else {
        "taffy"
    };

    let metrics = Rc::new(RefCell::new(PhaseMetrics::default()));

    let app = application();
    let app = if requested == "morphorm" {
        // The swap. Drop this branch and the default engine lays the same tree
        // out instead — nothing below changes either way.
        app.with_layout_engine(|| Box::new(MorphormLayoutEngine::new()))
    } else {
        app
    };

    app.with_frame_pipeline({
        let metrics = metrics.clone();
        move |_| Box::new(StandardImmediatePipeline.instrumented(metrics.clone()))
    })
    .run(move |cx: &mut App| {
        cx.open_window(WindowOptions::default(), move |_, cx| {
            cx.new(|_| Demo {
                engine,
                buttons,
                frame: 0,
                metrics: metrics.clone(),
            })
        })
        .unwrap();
        cx.activate(true);
    });
}
