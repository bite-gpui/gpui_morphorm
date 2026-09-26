//! Two `LayoutEngine`s, one tree: how far apart are they?
//!
//! The claim a second layout engine has to earn is that it lays the same tree
//! out the same way. `sample::build` and `buttons::build` drive both engines,
//! node ids come out in the same pre-order, and every node's bounds are
//! compared.
//!
//! Run with `--nocapture` to see the worst delta for each case.

use gpui_engine::{Display, LayoutEngine};
use gpui_types::{Bounds, Pixels};
use gpui_morphorm::{MorphormLayoutEngine, buttons, sample};

/// The tolerance, in logical pixels.
///
/// The two engines agree exactly on every case here. The budget is not slack for
/// a known disagreement — there is none — it is the margin a future change to
/// either engine would have to exceed before this test failed loudly, which at a
/// fractional scale means landing on the other side of a half pixel.
const TOLERANCE: f32 = 0.5;

/// The worst disagreement between two runs, which node it was, and every node
/// that differed at all.
fn differences(expected: &[Bounds<Pixels>], actual: &[Bounds<Pixels>]) -> (f32, usize, Vec<String>) {
    let mut worst = 0.0f32;
    let mut worst_index = 0usize;
    let mut lines = Vec::new();

    for (index, (expected, actual)) in expected.iter().zip(actual.iter()).enumerate() {
        let delta = [
            expected.origin.x.0 - actual.origin.x.0,
            expected.origin.y.0 - actual.origin.y.0,
            expected.size.width.0 - actual.size.width.0,
            expected.size.height.0 - actual.size.height.0,
        ]
        .into_iter()
        .fold(0.0f32, |worst, delta| worst.max(delta.abs()));

        if delta > worst {
            worst = delta;
            worst_index = index;
        }
        if delta > 0.0 {
            lines.push(format!(
                "node {index}: taffy {} · morphorm {}",
                describe(expected),
                describe(actual)
            ));
        }
    }
    (worst, worst_index, lines)
}

fn describe(bounds: &Bounds<Pixels>) -> String {
    format!(
        "x {:.1}..{:.1}, y {:.1}..{:.1} ({:.1}x{:.1})",
        bounds.origin.x.0,
        bounds.origin.x.0 + bounds.size.width.0,
        bounds.origin.y.0,
        bounds.origin.y.0 + bounds.size.height.0,
        bounds.size.width.0,
        bounds.size.height.0,
    )
}

fn sample_layout(engine: &mut dyn LayoutEngine, scale_factor: f32) -> Vec<Bounds<Pixels>> {
    let ids = sample::build(engine, Pixels(sample::REM_SIZE), scale_factor);
    // As a facade does before laying a window out.
    engine.stretch_auto_size_to_fill(ids[0], sample::viewport(), scale_factor);
    engine.compute_layout(
        ids[0],
        sample::available_space(),
        scale_factor,
        &mut sample::NullContext,
    );
    ids.iter()
        .map(|id| engine.layout_bounds(*id, scale_factor))
        .collect()
}

fn buttons_layout(
    engine: &mut dyn LayoutEngine,
    count: usize,
    frame: usize,
    scale_factor: f32,
) -> Vec<Bounds<Pixels>> {
    let ids = buttons::build(
        engine,
        count,
        frame,
        Pixels(sample::REM_SIZE),
        scale_factor,
    );
    engine.stretch_auto_size_to_fill(ids[0], buttons::viewport(count), scale_factor);
    engine.compute_layout(
        ids[0],
        buttons::available_space(count),
        scale_factor,
        &mut sample::NullContext,
    );
    ids.iter()
        .map(|id| engine.layout_bounds(*id, scale_factor))
        .collect()
}

#[test]
fn morphorm_agrees_with_taffy_on_the_direct_subset() {
    let mut taffy = gpui_engine_default::default_layout_engine();
    let mut morphorm = MorphormLayoutEngine::new();

    // An integral scale, and two fractional ones: the last two are what the
    // device-pixel snapping in both engines has to survive.
    for scale_factor in [1.0f32, 1.5, 2.0] {
        let expected = sample_layout(taffy.as_mut(), scale_factor);
        let actual = sample_layout(&mut morphorm, scale_factor);
        assert_eq!(expected.len(), actual.len());

        let (worst, worst_index, lines) = differences(&expected, &actual);
        for line in &lines {
            println!("scale {scale_factor}: {line}");
        }
        println!(
            "scale {scale_factor}: worst delta {worst:.2} px, {} of {} nodes differ",
            lines.len(),
            expected.len()
        );

        assert!(
            worst <= TOLERANCE,
            "at scale {scale_factor} the worst delta {worst:.2} px exceeds the \
             {TOLERANCE:.2} px budget, at node {worst_index}",
        );
    }
}

/// The tree that costs something: fifty to a thousand buttons, two columns, and
/// the content different every frame.
///
/// This is the case the timings below are about, so it is held to the same
/// standard as the small tree: a faster engine that lays the tree out
/// differently is not a faster engine.
#[test]
fn morphorm_agrees_with_taffy_on_many_changing_buttons() {
    let mut taffy = gpui_engine_default::default_layout_engine();
    let mut morphorm = MorphormLayoutEngine::new();

    for (count, frame) in [(50usize, 0usize), (50, 7), (200, 3), (1000, 3)] {
        let expected = buttons_layout(taffy.as_mut(), count, frame, 1.0);
        let actual = buttons_layout(&mut morphorm, count, frame, 1.0);
        assert_eq!(expected.len(), actual.len());

        let (worst, worst_index, lines) = differences(&expected, &actual);
        for line in lines.iter().take(5) {
            println!("{count} buttons, frame {frame}: {line}");
        }
        println!(
            "{count} buttons, frame {frame}: worst delta {worst:.2} px, {} of {} nodes differ",
            lines.len(),
            expected.len()
        );

        assert!(
            worst <= TOLERANCE,
            "{count} buttons at frame {frame}: the worst delta {worst:.2} px exceeds \
             the {TOLERANCE:.2} px budget, at node {worst_index}",
        );
    }
}

/// The root must fill the viewport rather than collapse to nothing.
///
/// morphorm sizes a tree root from the root's *own* properties and reads them
/// through `to_px(0.0, 0.0)`, which makes an `auto` root zero. This is the case
/// the available-space substitution in `compute_layout` exists for: the trees in
/// `sample` and `buttons` both have sized content, so neither exercises it.
#[test]
fn an_auto_root_fills_the_available_space() {
    let scale_factor = 1.0;
    let mut engine = MorphormLayoutEngine::new();
    let root = engine.request_layout(
        &sample::neutral(Display::Flex),
        Pixels(sample::REM_SIZE),
        scale_factor,
        &[],
    );
    engine.compute_layout(
        root,
        sample::available_space(),
        scale_factor,
        &mut sample::NullContext,
    );

    let bounds = engine.layout_bounds(root, scale_factor);
    assert_eq!(bounds.size.width.0, sample::VIEWPORT_WIDTH);
    assert_eq!(bounds.size.height.0, sample::VIEWPORT_HEIGHT);
}
