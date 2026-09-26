//! What each layout engine costs, measured through the trait.
//!
//! Two columns over one tree, in the shape `parley-demo/benches/text_system.rs`
//! established for the text system: the same work driven through the same
//! boundary, so the difference is the engine rather than the harness.
//!
//! Nothing below names a concrete engine type outside the setup closures. Every
//! call is made through `&mut dyn LayoutEngine`, which is the trait
//! `Application::with_layout_engine` swaps.
//!
//! Two groups, because the facade pays two separate costs per frame:
//!
//! - `frame`: `clear`, build the tree, compute the layout. The facade starts
//!   every frame this way.
//! - `bounds`: `layout_bounds` for every node, which is what painting asks for.
//!   This is where the two engines differ structurally — morphorm stores each
//!   node's position relative to its parent, so the window-relative origin is a
//!   walk to the root, where taffy's is already there.
//!
//! Run: `cargo bench --bench layout_engine`.

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use gpui_engine::{LayoutEngine, LayoutId};
use gpui_types::Pixels;
use gpui_morphorm::{MorphormLayoutEngine, buttons, sample};
use std::cell::Cell;

const SCALE: f32 = 1.0;

/// How many buttons the stress group lays out. The first is the shape the
/// example renders; the rest are there to show how the gap between the engines
/// grows with the tree.
const BUTTON_COUNTS: [usize; 3] = [50, 200, 1000];

fn build_and_compute(engine: &mut dyn LayoutEngine, scale_factor: f32) -> Vec<LayoutId> {
    engine.clear();
    let ids = sample::build(engine, Pixels(sample::REM_SIZE), scale_factor);
    engine.stretch_auto_size_to_fill(ids[0], sample::viewport(), scale_factor);
    engine.compute_layout(
        ids[0],
        sample::available_space(),
        scale_factor,
        &mut sample::NullContext,
    );
    ids
}

/// One frame of the button tree: clear, build, and lay out.
fn build_and_compute_buttons(
    engine: &mut dyn LayoutEngine,
    count: usize,
    frame: usize,
    scale_factor: f32,
) -> Vec<LayoutId> {
    engine.clear();
    let ids = buttons::build(engine, count, frame, Pixels(sample::REM_SIZE), scale_factor);
    engine.stretch_auto_size_to_fill(ids[0], buttons::viewport(count), scale_factor);
    engine.compute_layout(
        ids[0],
        buttons::available_space(count),
        scale_factor,
        &mut sample::NullContext,
    );
    ids
}

fn frame_costs(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame");

    group.bench_function("taffy", |b| {
        b.iter_batched(
            gpui_engine_default::default_layout_engine,
            |mut engine| build_and_compute(engine.as_mut(), SCALE).len(),
            BatchSize::SmallInput,
        );
    });

    group.bench_function("morphorm", |b| {
        b.iter_batched(
            || Box::new(MorphormLayoutEngine::new()) as Box<dyn LayoutEngine>,
            |mut engine| build_and_compute(engine.as_mut(), SCALE).len(),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

fn bounds_costs(c: &mut Criterion) {
    let mut group = c.benchmark_group("bounds");

    group.bench_function("taffy", |b| {
        b.iter_batched(
            || {
                let mut engine = gpui_engine_default::default_layout_engine();
                let ids = build_and_compute(engine.as_mut(), SCALE);
                (engine, ids)
            },
            |(mut engine, ids)| {
                ids.iter()
                    .map(|id| engine.layout_bounds(*id, SCALE))
                    .fold(0.0f32, |sum, bounds| sum + bounds.size.width.0)
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("morphorm", |b| {
        b.iter_batched(
            || {
                let mut engine = Box::new(MorphormLayoutEngine::new()) as Box<dyn LayoutEngine>;
                let ids = build_and_compute(engine.as_mut(), SCALE);
                (engine, ids)
            },
            |(mut engine, ids)| {
                ids.iter()
                    .map(|id| engine.layout_bounds(*id, SCALE))
                    .fold(0.0f32, |sum, bounds| sum + bounds.size.width.0)
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// The stress case: many buttons in two columns, with the content different
/// every iteration so no frame is a repeat of the last.
fn button_costs(c: &mut Criterion) {
    let mut group = c.benchmark_group("buttons");

    for count in BUTTON_COUNTS {
        group.bench_with_input(BenchmarkId::new("taffy", count), &count, |b, &count| {
            let frame = Cell::new(0usize);
            b.iter_batched(
                || {
                    let next = frame.get();
                    frame.set(next.wrapping_add(1));
                    (gpui_engine_default::default_layout_engine(), next)
                },
                |(mut engine, frame)| {
                    build_and_compute_buttons(engine.as_mut(), count, frame, SCALE).len()
                },
                BatchSize::SmallInput,
            );
        });

        group.bench_with_input(BenchmarkId::new("morphorm", count), &count, |b, &count| {
            let frame = Cell::new(0usize);
            b.iter_batched(
                || {
                    let next = frame.get();
                    frame.set(next.wrapping_add(1));
                    (Box::new(MorphormLayoutEngine::new()) as Box<dyn LayoutEngine>, next)
                },
                |(mut engine, frame)| {
                    build_and_compute_buttons(engine.as_mut(), count, frame, SCALE).len()
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

criterion_group!(benches, frame_costs, bounds_costs, button_costs);
criterion_main!(benches);
