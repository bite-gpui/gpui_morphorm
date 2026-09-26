//! The tree the conformance test compares and the benchmark measures.
//!
//! One builder for both, so "the engines agree on this tree" and "here is what
//! each engine costs on this tree" are statements about the same thing.
//!
//! The tree is deliberately restricted to the subset the mapping in
//! `gpui_morphorm.rs` claims to handle directly: flex rows and columns,
//! paddings, gaps, explicit and percentage sizes, `flex_grow`, absolute
//! positioning, and one measured leaf. Anything the mapping cannot express — a
//! margin on an in-flow child, `flex_shrink`, `justify_content` together with an
//! independent `align_items` — is absent on purpose, because a difference there
//! would be a difference the mapping already documents rather than a defect.
//!
//! Every default here mirrors the facade's own `Style::default()`: `inset` is
//! `auto`, `margin` is zero (not `auto`, which CSS would read as "absorb the free
//! space" and centre the node), padding and borders are zero, and sizes are
//! `auto`.

use gpui_engine::{
    BoxedMeasureFn, Display, EngineLayoutStyle, FlexDirection, FlexWrap, LayoutEngine, LayoutId,
    MeasureContext, MeasureHandles, Overflow, Position,
};
use gpui_types::{
    AbsoluteLength, AvailableSpace, DefiniteLength, Edges, Length, Pixels, Point, Size,
};

/// The available space every layout is computed under.
pub const VIEWPORT_WIDTH: f32 = 400.0;
/// The available space every layout is computed under.
pub const VIEWPORT_HEIGHT: f32 = 300.0;

/// What the facade passes as `rem_size`.
pub const REM_SIZE: f32 = 16.0;

fn px(value: f32) -> AbsoluteLength {
    Pixels(value).into()
}

pub fn definite(value: f32) -> DefiniteLength {
    DefiniteLength::Absolute(px(value))
}

pub fn length(value: f32) -> Length {
    Length::Definite(definite(value))
}

pub fn auto() -> Length {
    Length::Auto
}

pub fn edges<T: Copy + std::fmt::Debug + Default + PartialEq>(value: T) -> Edges<T> {
    Edges {
        top: value,
        right: value,
        bottom: value,
        left: value,
    }
}

/// The available space, as the facade hands it to `compute_layout`.
pub fn available_space() -> Size<AvailableSpace> {
    Size {
        width: AvailableSpace::Definite(Pixels(VIEWPORT_WIDTH)),
        height: AvailableSpace::Definite(Pixels(VIEWPORT_HEIGHT)),
    }
}

/// The window size, as the facade hands it to `stretch_auto_size_to_fill`.
///
/// A facade stretches a window root before laying it out, so a driver that
/// wants to reproduce a window has to do the same; without it an `auto` root is
/// sized by `available_space` alone, which is what the substitution in
/// `compute_layout` is for.
pub fn viewport() -> Size<Pixels> {
    Size {
        width: Pixels(VIEWPORT_WIDTH),
        height: Pixels(VIEWPORT_HEIGHT),
    }
}

/// A style with every field at the facade's neutral value, so a case only states
/// what it is about.
pub fn neutral(display: Display) -> EngineLayoutStyle {
    EngineLayoutStyle {
        display,
        overflow: Point {
            x: Overflow::Visible,
            y: Overflow::Visible,
        },
        scrollbar_width: px(0.0),
        position: Position::Relative,
        inset: edges(auto()),
        size: Size {
            width: auto(),
            height: auto(),
        },
        min_size: Size {
            width: auto(),
            height: auto(),
        },
        max_size: Size {
            width: auto(),
            height: auto(),
        },
        aspect_ratio: None,
        margin: edges(length(0.0)),
        padding: edges(definite(0.0)),
        border_widths: edges(px(0.0)),
        align_items: None,
        align_self: None,
        align_content: None,
        justify_content: None,
        gap: Size {
            width: definite(0.0),
            height: definite(0.0),
        },
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::NoWrap,
        flex_basis: auto(),
        flex_grow: 0.0,
        flex_shrink: 1.0,
        grid_cols: None,
        grid_rows: None,
        grid_location: None,
    }
}

fn row(size: (f32, f32), padding: f32, gap: f32) -> EngineLayoutStyle {
    let mut style = neutral(Display::Flex);
    style.flex_direction = FlexDirection::Row;
    style.size = Size {
        width: length(size.0),
        height: length(size.1),
    };
    style.padding = edges(definite(padding));
    style.gap = Size {
        width: definite(gap),
        height: definite(gap),
    };
    style
}

fn column(gap: f32) -> EngineLayoutStyle {
    let mut style = neutral(Display::Flex);
    style.flex_direction = FlexDirection::Column;
    style.gap = Size {
        width: definite(gap),
        height: definite(gap),
    };
    style
}

fn sized(width: f32, height: f32) -> EngineLayoutStyle {
    let mut style = neutral(Display::Flex);
    style.size = Size {
        width: length(width),
        height: length(height),
    };
    style
}

fn grown() -> EngineLayoutStyle {
    let mut style = column(4.0);
    style.flex_grow = 1.0;
    style
}

fn absolute(right: f32, top: f32, size: f32) -> EngineLayoutStyle {
    let mut style = neutral(Display::Flex);
    style.position = Position::Absolute;
    style.inset = Edges {
        top: length(top),
        right: length(right),
        bottom: auto(),
        left: auto(),
    };
    style.size = Size {
        width: length(size),
        height: length(size),
    };
    style
}

/// A measured leaf: `auto` in both axes, its size decided by the callback, the
/// way a run of text is.
fn measured_leaf() -> EngineLayoutStyle {
    neutral(Display::Block)
}

/// The callback every measured leaf gets.
///
/// It ignores its constraints and hands back a fixed size, which is enough to
/// exercise the content-size path in both engines without dragging a font stack
/// into the comparison.
fn fixed_measure(width: f32, height: f32) -> BoxedMeasureFn {
    Box::new(
        move |_known: Size<Option<Pixels>>,
              _available: Size<AvailableSpace>,
              _context: &mut dyn MeasureContext| {
            Size {
                width: Pixels(width),
                height: Pixels(height),
            }
        },
    )
}

/// The context handed to `compute_layout`.
///
/// Neither engine's measure path here reads it — the callbacks above take their
/// answer from nothing at all — so reaching `handles` would mean the engine asked
/// for a facade handle the caller never supplied.
pub struct NullContext;

impl MeasureContext for NullContext {
    fn handles(&mut self) -> MeasureHandles<'_> {
        unreachable!("the measure callbacks in this tree do not read the facade handles")
    }
}

/// Builds the tree into `engine`, returning its nodes in pre-order.
///
/// The order is the contract between any two runs: index `i` in one engine's list
/// is the same element as index `i` in the other's.
pub fn build(engine: &mut dyn LayoutEngine, rem_size: Pixels, scale_factor: f32) -> Vec<LayoutId> {
    let mut ids = Vec::new();

    let leaf =
        engine.request_measured_layout(&measured_leaf(), rem_size, scale_factor, fixed_measure(70.0, 18.0));
    let a1 = engine.request_layout(&sized(60.0, 20.0), rem_size, scale_factor, &[]);
    let a2 = engine.request_layout(&sized(60.0, 20.0), rem_size, scale_factor, &[]);
    // A nested column, so the axes swap along the way.
    let a3a = engine.request_layout(&sized(24.0, 12.0), rem_size, scale_factor, &[]);
    let a3b = engine.request_layout(&sized(24.0, 12.0), rem_size, scale_factor, &[]);
    let a3 = engine.request_layout(&column(4.0), rem_size, scale_factor, &[a3a, a3b]);
    let a = engine.request_layout(&grown(), rem_size, scale_factor, &[a1, a2, a3, leaf]);

    // A percentage width, which only resolves once the parent's width is known.
    let mut b_wide = neutral(Display::Flex);
    b_wide.size = Size {
        width: Length::Definite(DefiniteLength::Fraction(0.5)),
        height: length(20.0),
    };
    let b1 = engine.request_layout(&b_wide, rem_size, scale_factor, &[]);
    let b2 = engine.request_layout(&sized(40.0, 40.0), rem_size, scale_factor, &[]);
    let mut b_column = column(4.0);
    b_column.size = Size {
        width: length(100.0),
        height: auto(),
    };
    let b = engine.request_layout(&b_column, rem_size, scale_factor, &[b1, b2]);

    let c = engine.request_layout(&absolute(5.0, 5.0, 30.0), rem_size, scale_factor, &[]);

    let root = engine.request_layout(
        &row((VIEWPORT_WIDTH, VIEWPORT_HEIGHT), 10.0, 8.0),
        rem_size,
        scale_factor,
        &[a, b, c],
    );

    // Pre-order, which is the order the comparisons assume.
    ids.push(root);
    ids.push(a);
    ids.push(a1);
    ids.push(a2);
    ids.push(a3);
    ids.push(a3a);
    ids.push(a3b);
    ids.push(leaf);
    ids.push(b);
    ids.push(b1);
    ids.push(b2);
    ids.push(c);
    ids
}
