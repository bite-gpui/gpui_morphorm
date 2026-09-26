//! The engine, driven the way the facade drives it.
//!
//! `tests/layout_engine.rs` compares two engines through the trait, which is
//! where correctness is established. This file is about the *other* half: whether
//! the facade can use the engine at all.
//!
//! That is not the same question, and assuming it was cost a transparent window.
//! A facade does not call `request_layout` and `compute_layout` the way a
//! benchmark does — it builds a window root with an `auto` size, stretches that
//! root to the window, lays the tree out against the window's available space,
//! and reads bounds while painting. An engine can be indistinguishable from the
//! default one on a benchmark tree and still paint nothing in a real window, and
//! only a test that draws a frame can tell the two apart.
//!
//! Run: `cargo test --features test-support --test facade`.
//!
//! One trap is worth recording, because the error it produces does not mention
//! it: **do not `use gpui::*;` in a file that expands `#[gpui::test]`.** The glob
//! brings gpui's own `test` attribute macro into scope, which shadows the
//! built-in `#[test]` — and the `#[test]` that `#[gpui::test]` emits then resolves
//! to gpui's macro rather than the built-in one, which expands itself forever.
//! rustc reports it as `recursion limit reached while expanding #[test]`, with no
//! hint that the glob is at fault. `scroll-demo` sidesteps it the same way, with
//! a curated import list.

#![cfg(feature = "test-support")]

use gpui::prelude::*;
use gpui::{Context, IntoElement, Render, TestAppContext, Window, div, px, rgb, size};
use gpui_morphorm::MorphormLayoutEngine;
use std::rc::Rc;

const WINDOW_WIDTH: f32 = 800.0;
const WINDOW_HEIGHT: f32 = 600.0;

/// The colour the root paints, and the one the assertion looks for.
const ROOT: u32 = 0x112233;
/// The colour a row inside it paints.
const ROW: u32 = 0x445566;

/// A view shaped like the one `src/main.rs` renders: a full-size column with a
/// couple of rows in it.
struct Rows;

impl Render for Rows {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .size_full()
            .bg(rgb(ROOT))
            .child(div().h(px(20.0)).w_full().bg(rgb(ROW)))
            .child(div().h(px(20.0)).w_full().bg(rgb(ROW)))
    }
}

/// Draws one frame at [`WINDOW_WIDTH`]×[`WINDOW_HEIGHT`], optionally swapping the
/// engine first, and returns the quads the frame painted.
fn painted_quads(swapped: bool) -> Vec<gpui::Quad> {
    let mut cx = TestAppContext::single();
    if swapped {
        cx.update(|app| {
            app.set_layout_engine_factory(Rc::new(|| Box::new(MorphormLayoutEngine::new())));
        });
    }

    let (_view, cx) = cx.add_window_view(|_window, _cx| Rows);
    cx.simulate_resize(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)));
    cx.run_until_parked();
    cx.update(|window, _app| window.painted_quads())
}

/// Checks the shape a frame must have, whichever engine drew it.
fn assert_the_window_is_filled(quads: &[gpui::Quad]) {
    let root = quads
        .iter()
        .find(|quad| quad.background == rgb(ROOT).into())
        .unwrap_or_else(|| {
            panic!(
                "the root painted nothing; {} quads in the frame",
                quads.len()
            )
        });

    // The asserted size is relative to the window rather than an absolute
    // number, because the test platform runs at a scale factor of its own: what
    // matters is that the root covers the window, not that it is 800 wide in
    // whichever unit the scene happens to be in.
    println!("root {:?}", root.bounds);
    assert_eq!(root.bounds.origin.x.0, 0.0);
    assert_eq!(root.bounds.origin.y.0, 0.0);
    assert!(root.bounds.size.width.0 > 0.0, "the root collapsed");
    assert!(
        (root.bounds.size.width.0 / root.bounds.size.height.0 - WINDOW_WIDTH / WINDOW_HEIGHT).abs()
            < 0.01,
        "the root should be the window's shape: {:?}",
        root.bounds,
    );

    let rows: Vec<_> = quads
        .iter()
        .filter(|quad| quad.background == rgb(ROW).into())
        .collect();
    assert_eq!(rows.len(), 2, "expected both rows to be painted");
    for row in rows {
        // Inside the root, and narrower than it by the padding.
        println!("row {:?}", row.bounds);
        assert!(
            row.bounds.size.width.0 > 0.0 && row.bounds.size.width.0 <= root.bounds.size.width.0,
            "a row should fill the root's content box: {:?}",
            row.bounds,
        );
    }
}

/// The calibration for the test below: the default engine fills the window.
///
/// Worth having on its own, because it is what says the *harness* is right when
/// the next test fails.
#[gpui::test]
fn the_default_engine_fills_the_window(_cx: &mut TestAppContext) {
    assert_the_window_is_filled(&painted_quads(false));
}

/// The engine a window ends up with is the one whose root fills the window.
///
/// The assertion is deliberately about *painted area* rather than about bounds:
/// "the window is transparent" is what a user sees, and a root that measured
/// itself down to nothing is exactly how an engine produces it.
#[gpui::test]
fn the_swapped_engine_fills_the_window(_cx: &mut TestAppContext) {
    assert_the_window_is_filled(&painted_quads(true));
}
