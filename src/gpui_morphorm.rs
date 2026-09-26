//! A second [`LayoutEngine`], backed by `morphorm`.
//!
//! `gpui_engine` owns the layout contract and never names a solver, so an
//! alternative engine can back the same tree. The default one is taffy
//! (`bite-gp-engine-default`, in tree `gpui_engine_default`); this is a
//! different solver behind the same trait, written outside the tree, which is
//! what makes the layout seam a seam rather than a claim.
//!
//! # What maps, and what does not
//!
//! The vocabulary crossing the boundary is taffy's — `EngineLayoutStyle` is
//! deliberately a set of copies of taffy's style enums — while morphorm reads
//! its own vocabulary, and the two are not the same shape. Most of the mapping
//! is direct; where it is not, the choice made here is recorded, because a
//! second engine is only useful if you can see where its semantics end.
//!
//! Direct:
//!
//! - `display: Flex` + `flex_direction` → `LayoutType::Row`/`Column`.
//! - `display: Grid` → `LayoutType::Grid`; `display: Block` → `LayoutType::Column`
//!   (morphorm has no block flow, and a column is the closest single-axis stack).
//! - `display: None` → `Node::visible() == false`, which is how morphorm prunes.
//! - `position`, `aspect_ratio`, `size`, `min_size`, `max_size`, `gap`,
//!   `flex_wrap`, and the absolute `inset` offsets.
//! - Percentages and absolute/rem lengths, resolved through the same
//!   device-pixel snapping the default engine uses, so the two agree on
//!   sub-pixel edges.
//!
//! Approximated, and how:
//!
//! - **Main-axis size.** CSS has three axes of control — `flex_basis`, `size`,
//!   `flex_grow` — and morphorm has one `Units` per axis. A definite basis wins,
//!   then a definite size, then `flex_grow` becomes `Units::Stretch(factor)`,
//!   then the content size. `flex_shrink` has no equivalent and is dropped.
//! - **Cross-axis stretch.** `align_items: Stretch` — the CSS and taffy default
//!   — becomes `Units::Stretch(1.0)` on a child's cross axis when that axis is
//!   `auto`, for measured leaves and containers alike.
//! - **Measured leaves.** morphorm has one intrinsic-size hook and assigns
//!   *both* axes from its answer, where taffy resolves each axis independently.
//!   [`Node::content_size`] therefore echoes back whichever axis the layout
//!   already resolved and returns the callback's answer only for the axis still
//!   in doubt, which is what lets a stretched width and a measured height
//!   coexist the way CSS wants for text.
//! - **Alignment.** morphorm positions children with one nine-way `Alignment`
//!   covering both axes, so `justify_content` and `align_items` collapse into a
//!   single value and only those nine combinations exist. `SpaceBetween`,
//!   `SpaceEvenly`, `SpaceAround`, `Baseline` and per-child `align_self` have no
//!   equivalent and fall back to the start of their axis.
//! - **Margins.** morphorm's `left`/`right`/`top`/`bottom` spacing is read only
//!   for absolutely positioned children, so a margin on an in-flow child is
//!   dropped. On an absolute child it is folded into a definite inset, which is
//!   the offset CSS would apply; beside an `auto` inset it is dropped instead,
//!   because morphorm prefers the leading offset of each axis and an `auto`
//!   turned into `0px` would beat a definite `right` or `bottom`.
//! - **Borders and scrollbars.** morphorm has neither. Border widths and the
//!   `Overflow::Scroll` gutter are folded into `padding`, which is the same
//!   inset the content box sees.
//! - **Grid tracks.** `GridTemplateMinSize::Zero` becomes `Stretch(1.0)` and the
//!   `MinContent`/`MaxContent` variants become `Auto`.
//! - **Root sizing.** morphorm reads a tree root's extent with
//!   `to_px(0.0, 0.0)`, which is only meaningful for a `Pixels` extent: `Auto`
//!   and `Stretch` come out as zero, and a `Percentage` resolves against a parent
//!   the root does not have. A window root is usually `size_full` — `100%` in both
//!   axes — so the root's extent is resolved against `available_space` here, the
//!   way CSS resolves a root against the initial containing block. Getting this
//!   wrong paints an empty window, and nothing in the conformance comparison
//!   below would have caught it: `tests/facade.rs` is what caught it.
//! - **Reverse directions.** `Row` plus `Direction::RightToLeft` covers
//!   `FlexDirection::RowReverse`; `ColumnReverse` and `FlexWrap::WrapReverse`
//!   have no equivalent and lay out as their forward forms.
//! - **Relative insets.** A `position: relative` node's `inset` is a paint-time
//!   offset in CSS, and morphorm ignores it there too.
//!
//! `tests/layout_engine.rs` measures how far apart the two engines actually are
//! on a tree built from the direct subset, rather than trusting this list, and
//! `tests/facade.rs` draws a real frame through the facade — window root and all
//! — because a tree that agrees node for node can still paint nothing.

use gpui_engine::{
    AlignContent, AlignItems, BoxedMeasureFn, Display, EngineLayoutStyle, FlexDirection, FlexWrap,
    GridTemplate, GridTemplateMinSize, JustifyContent, LayoutEngine, LayoutId, MeasureContext,
    Overflow, Position,
};
use gpui_types::{
    AbsoluteLength, AvailableSpace, Bounds, DefiniteLength, Edges, GridLocation, GridPlacement,
    Length, Pixels, Point, Size, round_half_toward_zero, round_to_device_pixel,
};
use morphorm::{Alignment, Cache, Direction, LayoutType, LayoutWrap, Node, PositionType, Units};
use std::{cell::RefCell, collections::HashMap, ops::Range, slice};

/// The tree the conformance test compares and the benchmark measures.
pub mod sample;

/// A tree whose layout costs something: many buttons, two columns, and the
/// content changing every frame.
pub mod buttons;

/// A node in the tree morphorm walks.
///
/// The real data lives in [`Store`]; this is the handle morphorm's associated
/// types are keyed on.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct MorphNode(usize);

/// A measured leaf: the callback, and the results it has already produced.
///
/// `content_size` takes `&self`, and morphorm may ask the same leaf more than
/// once in a pass, so the callback needs interior mutability to be callable at
/// all. The memo beside it is not an optimisation for its own sake: taffy caches
/// measured sizes internally, and without one the comparison would be measuring
/// the presence of a cache rather than the layout algorithm.
struct MeasuredLeaf {
    measure: RefCell<BoxedMeasureFn>,
    memo: RefCell<Vec<MemoEntry>>,
}

/// One measured result, keyed by the constraints it was produced under.
///
/// `None` means "that axis was unconstrained", which is why the key is an
/// `Option` rather than a sentinel float.
struct MemoEntry {
    width: Option<f32>,
    height: Option<f32>,
    measured: (f32, f32),
}

/// One node, authored style and resolved units together.
struct NodeData {
    handle: MorphNode,
    parent: Option<usize>,
    children: Vec<MorphNode>,
    measured: Option<MeasuredLeaf>,

    /// What the facade supplied alongside the style. Lengths resolve against
    /// these, so every node keeps them.
    rem_size: Pixels,
    scale_factor: f32,

    // The style as authored.
    visible: bool,
    layout_type: LayoutType,
    position_type: PositionType,
    wrap: LayoutWrap,
    direction: Direction,
    alignment: Alignment,
    size: Size<Length>,
    min_size: Size<Length>,
    max_size: Size<Length>,
    aspect_ratio: Option<f32>,
    padding: Edges<DefiniteLength>,
    border_widths: Edges<AbsoluteLength>,
    scrollbar_width: AbsoluteLength,
    /// `(x, y)`: whether that axis reserves a scrollbar gutter.
    scroll_gutter: (bool, bool),
    /// Whether this node's `align_items` stretches an auto-sized child across
    /// the cross axis. Read when a child's axes are resolved.
    stretches_children: bool,
    /// Whether the parent's extent along *this* node's cross axis was definite,
    /// which is what decides whether that cross axis may stretch. See
    /// [`NodeData::cross_extent_definite`].
    parent_cross_extent_definite: bool,
    margin: Edges<Length>,
    inset: Edges<Length>,
    gap: Size<DefiniteLength>,
    flex_basis: Length,
    flex_grow: f32,
    grid_cols: Option<GridTemplate>,
    grid_rows: Option<GridTemplate>,
    grid_location: Option<GridLocation>,

    // The same style resolved into the units morphorm reads. Which of `width`
    // and `height` is the main axis depends on the *parent's* layout type, so
    // these are re-resolved when the parent is created. A node that never gets
    // a parent is the layout root, and resolves as if it were in a row.
    width: Units,
    height: Units,
    min_width: Units,
    min_height: Units,
    max_width: Units,
    max_height: Units,
}

/// A resolved placement on one axis, as the nine-way `Alignment` needs it.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Place {
    Start,
    Center,
    End,
}

impl Place {
    fn of(place: f32) -> Self {
        if place <= 0.0 {
            Place::Start
        } else if place >= 1.0 {
            Place::End
        } else {
            Place::Center
        }
    }
}

fn alignment_of(
    layout_type: LayoutType,
    justify: Option<JustifyContent>,
    align: Option<AlignItems>,
) -> Alignment {
    // morphorm's `Alignment` is (horizontal, vertical), and it is the parent's
    // property that positions the children. For a column the main axis is
    // vertical, so the two swap.
    let main = Place::of(match justify {
        Some(AlignContent::Center) => 0.5,
        Some(AlignContent::End | AlignContent::FlexEnd) => 1.0,
        // `Start`, the flex-start variants, and the three `Space*` values that
        // have no morphorm equivalent.
        _ => 0.0,
    });
    let cross = Place::of(match align {
        Some(AlignItems::Center) => 0.5,
        Some(AlignItems::End | AlignItems::FlexEnd) => 1.0,
        _ => 0.0,
    });
    let (horizontal, vertical) = if layout_type == LayoutType::Column {
        (cross, main)
    } else {
        (main, cross)
    };
    match (horizontal, vertical) {
        (Place::Start, Place::Start) => Alignment::TopLeft,
        (Place::Center, Place::Start) => Alignment::TopCenter,
        (Place::End, Place::Start) => Alignment::TopRight,
        (Place::Start, Place::Center) => Alignment::Left,
        (Place::Center, Place::Center) => Alignment::Center,
        (Place::End, Place::Center) => Alignment::Right,
        (Place::Start, Place::End) => Alignment::BottomLeft,
        (Place::Center, Place::End) => Alignment::BottomCenter,
        (Place::End, Place::End) => Alignment::BottomRight,
    }
}

/// `align_items` that is unset is `Stretch`, as in CSS and taffy.
fn stretches_cross(align: Option<AlignItems>) -> bool {
    matches!(align, None | Some(AlignItems::Stretch))
}

impl NodeData {
    /// Whether this node's size along the axis its *children* stretch across is
    /// definite.
    ///
    /// `align_items: Stretch` in CSS stretches a child to the flex line's cross
    /// size, and the line's cross size is the container's inner cross size only
    /// when that is definite; when the container is sized by its content, the
    /// line is sized to the content instead. morphorm has no notion of a
    /// hypothetical line size — `Units::Stretch` on a child means "the parent's
    /// cross extent", full stop — so stretching a child of an auto-sized parent
    /// hands it zero and collapses the parent with it. This is the test that
    /// keeps the mapping from doing that: stretch only where CSS would.
    fn cross_extent_definite(&self) -> bool {
        // For a column the children's cross axis is horizontal, and for a row it
        // is vertical, so the extent in question is this node's own size on
        // that axis.
        let extent = if self.layout_type == LayoutType::Column {
            self.width
        } else {
            self.height
        };
        match extent {
            Units::Pixels(_) | Units::Percentage(_) => true,
            // A stretched extent is as definite as whatever allocated it.
            Units::Stretch(_) => self.parent_cross_extent_definite,
            Units::Auto => false,
        }
    }

    /// Re-resolves both axes against the parent that has just been created.
    fn resolve(&mut self, parent_layout_type: LayoutType, parent_stretches_cross: bool) {
        let column = parent_layout_type == LayoutType::Column;
        // `size`, `min_size` and `max_size` are `(width, height)`, so the axes
        // are picked apart rather than assumed.
        let (main_size, cross_size) = if column {
            (self.size.height, self.size.width)
        } else {
            (self.size.width, self.size.height)
        };
        let (main_min, cross_min) = if column {
            (self.min_size.height, self.min_size.width)
        } else {
            (self.min_size.width, self.min_size.height)
        };
        let (main_max, cross_max) = if column {
            (self.max_size.height, self.max_size.width)
        } else {
            (self.max_size.width, self.max_size.height)
        };

        let main = self.resolve_main(main_size);
        let cross = self.resolve_cross(cross_size, parent_stretches_cross);
        let min_main = self.resolve_min(main_min);
        let min_cross = self.resolve_min(cross_min);
        let max_main = self.resolve_max(main_max);
        let max_cross = self.resolve_max(cross_max);

        if column {
            self.height = main;
            self.width = cross;
            self.min_height = min_main;
            self.min_width = min_cross;
            self.max_height = max_main;
            self.max_width = max_cross;
        } else {
            self.width = main;
            self.height = cross;
            self.min_width = min_main;
            self.min_height = min_cross;
            self.max_width = max_main;
            self.max_height = max_cross;
        }
    }

    /// The main axis: a definite `flex_basis` wins, then a definite `size`, then
    /// `flex_grow` as a share of the free space, then the content size.
    fn resolve_main(&self, size: Length) -> Units {
        if let Length::Definite(length) = self.flex_basis {
            return self.definite(length);
        }
        if let Length::Definite(length) = size {
            return self.definite(length);
        }
        if self.flex_grow > 0.0 {
            return Units::Stretch(self.flex_grow);
        }
        Units::Auto
    }

    fn resolve_cross(&self, size: Length, stretch: bool) -> Units {
        if let Length::Definite(length) = size {
            return self.definite(length);
        }
        if stretch {
            return Units::Stretch(1.0);
        }
        Units::Auto
    }

    fn resolve_min(&self, length: Length) -> Units {
        match length {
            Length::Definite(length) => self.definite(length),
            // morphorm's own default when a minimum is absent.
            Length::Auto => Units::Pixels(0.0),
        }
    }

    fn resolve_max(&self, length: Length) -> Units {
        match length {
            Length::Definite(length) => self.definite(length),
            Length::Auto => Units::Pixels(f32::MAX),
        }
    }

    fn definite(&self, length: DefiniteLength) -> Units {
        match length {
            DefiniteLength::Absolute(length) => Units::Pixels(round_to_device_pixel(
                length.to_pixels(self.rem_size).0,
                self.scale_factor,
            )),
            DefiniteLength::Fraction(fraction) => Units::Percentage(fraction * 100.0),
        }
    }

    fn length(&self, length: Length) -> Units {
        match length {
            Length::Definite(length) => self.definite(length),
            Length::Auto => Units::Auto,
        }
    }

    /// Padding as morphorm sees it: the authored padding, plus the border widths
    /// and the scrollbar gutter morphorm has no concept of. All three are the
    /// same inset as far as the content box is concerned.
    fn padding_units(&self) -> Edges<Units> {
        let resolve = |padding: DefiniteLength, border: AbsoluteLength, gutter: bool| -> Units {
            let border =
                round_to_device_pixel(border.to_pixels(self.rem_size).0, self.scale_factor);
            let gutter = if gutter {
                round_to_device_pixel(
                    self.scrollbar_width.to_pixels(self.rem_size).0,
                    self.scale_factor,
                )
            } else {
                0.0
            };
            match padding {
                DefiniteLength::Absolute(length) => Units::Pixels(
                    round_to_device_pixel(length.to_pixels(self.rem_size).0, self.scale_factor)
                        + border
                        + gutter,
                ),
                // A fractional padding plus a pixel border is not one unit. The
                // padding is the part the children are laid out against, so it
                // wins and the rest is dropped.
                DefiniteLength::Fraction(fraction) => Units::Percentage(fraction * 100.0),
            }
        };
        Edges {
            top: resolve(self.padding.top, self.border_widths.top, false),
            right: resolve(
                self.padding.right,
                self.border_widths.right,
                self.scroll_gutter.0,
            ),
            bottom: resolve(
                self.padding.bottom,
                self.border_widths.bottom,
                self.scroll_gutter.1,
            ),
            left: resolve(self.padding.left, self.border_widths.left, false),
        }
    }

    /// The offset morphorm applies to an absolutely positioned child: the inset
    /// with the margin folded in, which is the offset CSS would apply.
    ///
    /// An `auto` inset stays `auto` — morphorm reads the four offsets as a pair
    /// per axis and prefers the leading one, so turning an `auto` `left` into
    /// `0px` would make it beat a definite `right`. A margin beside an `auto`
    /// inset is therefore dropped rather than folded.
    fn spacing(&self, inset: Length, margin: Length) -> Units {
        match (self.length(inset), self.length(margin)) {
            (Units::Pixels(inset), Units::Pixels(margin)) => Units::Pixels(inset + margin),
            (Units::Percentage(inset), Units::Percentage(margin)) => {
                Units::Percentage(inset + margin)
            }
            (inset, _) => inset,
        }
    }

    fn grid_tracks(template: Option<GridTemplate>) -> Vec<Units> {
        let Some(template) = template else {
            return Vec::new();
        };
        let unit = match template.min_size {
            // `minmax(0, 1fr)`: equal shares of the free space.
            GridTemplateMinSize::Zero => Units::Stretch(1.0),
            // morphorm has no min/max-content track sizing, so both fall back to
            // sizing the track to its content.
            GridTemplateMinSize::MinContent | GridTemplateMinSize::MaxContent => Units::Auto,
        };
        vec![unit; template.repeat as usize]
    }

    fn grid_start(placement: &Range<GridPlacement>) -> Option<usize> {
        match placement.start {
            // Grid lines are 1-based and morphorm indexes are 0-based.
            GridPlacement::Line(line) => Some(line.saturating_sub(1) as usize),
            _ => None,
        }
    }

    fn grid_span(placement: &Range<GridPlacement>) -> Option<usize> {
        match placement.end {
            GridPlacement::Span(span) => Some(span as usize),
            _ => None,
        }
    }
}

/// The nodes and the geometry morphorm computed for them.
#[derive(Default)]
struct Store {
    nodes: Vec<NodeData>,
}

/// morphorm's computed geometry, indexed by node.
///
/// Positions are relative to the parent's border box, not to the window, so
/// [`MorphormLayoutEngine::layout_bounds`] accumulates them.
#[derive(Default)]
struct LayoutCache {
    bounds: Vec<(f32, f32, f32, f32)>,
}

impl Cache for LayoutCache {
    type Node = MorphNode;

    fn width(&self, node: &MorphNode) -> f32 {
        self.bounds.get(node.0).map_or(0.0, |bounds| bounds.2)
    }

    fn height(&self, node: &MorphNode) -> f32 {
        self.bounds.get(node.0).map_or(0.0, |bounds| bounds.3)
    }

    fn posx(&self, node: &MorphNode) -> f32 {
        self.bounds.get(node.0).map_or(0.0, |bounds| bounds.0)
    }

    fn posy(&self, node: &MorphNode) -> f32 {
        self.bounds.get(node.0).map_or(0.0, |bounds| bounds.1)
    }

    fn set_bounds(&mut self, node: &MorphNode, posx: f32, posy: f32, width: f32, height: f32) {
        if self.bounds.len() <= node.0 {
            self.bounds.resize(node.0 + 1, (0.0, 0.0, 0.0, 0.0));
        }
        self.bounds[node.0] = (posx, posy, width, height);
    }
}

impl Node for MorphNode {
    type Store = Store;
    type Tree = Store;
    type ChildIter<'t> = slice::Iter<'t, MorphNode>;
    type CacheKey = usize;
    type SubLayout<'a> = &'a mut dyn MeasureContext;

    fn key(&self) -> usize {
        self.0
    }

    fn children<'t>(&'t self, tree: &'t Store) -> Self::ChildIter<'t> {
        tree.nodes[self.0].children.iter()
    }

    fn parent<'t>(&'t self, tree: &'t Store) -> Option<&'t Self> {
        let parent = tree.nodes[self.0].parent?;
        Some(&tree.nodes[parent].handle)
    }

    fn visible(&self, store: &Store) -> bool {
        store.nodes[self.0].visible
    }

    fn layout_type(&self, store: &Store) -> Option<LayoutType> {
        Some(store.nodes[self.0].layout_type)
    }

    fn position_type(&self, store: &Store) -> Option<PositionType> {
        Some(store.nodes[self.0].position_type)
    }

    fn direction(&self, store: &Store) -> Option<Direction> {
        Some(store.nodes[self.0].direction)
    }

    fn wrap(&self, store: &Store) -> Option<LayoutWrap> {
        Some(store.nodes[self.0].wrap)
    }

    fn alignment(&self, store: &Store) -> Option<Alignment> {
        Some(store.nodes[self.0].alignment)
    }

    fn aspect_ratio(&self, store: &Store) -> Option<f32> {
        store.nodes[self.0].aspect_ratio
    }

    fn width(&self, store: &Store) -> Option<Units> {
        Some(store.nodes[self.0].width)
    }

    fn height(&self, store: &Store) -> Option<Units> {
        Some(store.nodes[self.0].height)
    }

    fn min_width(&self, store: &Store) -> Option<Units> {
        Some(store.nodes[self.0].min_width)
    }

    fn min_height(&self, store: &Store) -> Option<Units> {
        Some(store.nodes[self.0].min_height)
    }

    fn max_width(&self, store: &Store) -> Option<Units> {
        Some(store.nodes[self.0].max_width)
    }

    fn max_height(&self, store: &Store) -> Option<Units> {
        Some(store.nodes[self.0].max_height)
    }

    fn left(&self, store: &Store) -> Option<Units> {
        let node = &store.nodes[self.0];
        Some(node.spacing(node.inset.left, node.margin.left))
    }

    fn right(&self, store: &Store) -> Option<Units> {
        let node = &store.nodes[self.0];
        Some(node.spacing(node.inset.right, node.margin.right))
    }

    fn top(&self, store: &Store) -> Option<Units> {
        let node = &store.nodes[self.0];
        Some(node.spacing(node.inset.top, node.margin.top))
    }

    fn bottom(&self, store: &Store) -> Option<Units> {
        let node = &store.nodes[self.0];
        Some(node.spacing(node.inset.bottom, node.margin.bottom))
    }

    fn padding_left(&self, store: &Store) -> Option<Units> {
        Some(store.nodes[self.0].padding_units().left)
    }

    fn padding_right(&self, store: &Store) -> Option<Units> {
        Some(store.nodes[self.0].padding_units().right)
    }

    fn padding_top(&self, store: &Store) -> Option<Units> {
        Some(store.nodes[self.0].padding_units().top)
    }

    fn padding_bottom(&self, store: &Store) -> Option<Units> {
        Some(store.nodes[self.0].padding_units().bottom)
    }

    fn horizontal_gap(&self, store: &Store) -> Option<Units> {
        let node = &store.nodes[self.0];
        Some(node.definite(node.gap.width))
    }

    fn vertical_gap(&self, store: &Store) -> Option<Units> {
        let node = &store.nodes[self.0];
        Some(node.definite(node.gap.height))
    }

    fn min_horizontal_gap(&self, _store: &Store) -> Option<Units> {
        None
    }

    fn min_vertical_gap(&self, _store: &Store) -> Option<Units> {
        None
    }

    fn max_horizontal_gap(&self, _store: &Store) -> Option<Units> {
        None
    }

    fn max_vertical_gap(&self, _store: &Store) -> Option<Units> {
        None
    }

    fn grid_columns(&self, store: &Store) -> Option<Vec<Units>> {
        Some(NodeData::grid_tracks(store.nodes[self.0].grid_cols))
    }

    fn grid_rows(&self, store: &Store) -> Option<Vec<Units>> {
        Some(NodeData::grid_tracks(store.nodes[self.0].grid_rows))
    }

    fn column_start(&self, store: &Store) -> Option<usize> {
        store.nodes[self.0]
            .grid_location
            .as_ref()
            .and_then(|location| NodeData::grid_start(&location.column))
    }

    fn row_start(&self, store: &Store) -> Option<usize> {
        store.nodes[self.0]
            .grid_location
            .as_ref()
            .and_then(|location| NodeData::grid_start(&location.row))
    }

    fn column_span(&self, store: &Store) -> Option<usize> {
        store.nodes[self.0]
            .grid_location
            .as_ref()
            .and_then(|location| NodeData::grid_span(&location.column))
    }

    fn row_span(&self, store: &Store) -> Option<usize> {
        store.nodes[self.0]
            .grid_location
            .as_ref()
            .and_then(|location| NodeData::grid_span(&location.row))
    }

    /// The intrinsic size of a measured leaf, forwarded to the callback the
    /// facade stored.
    ///
    /// morphorm asks in *its* space (device pixels); the facade's callback
    /// answers in logical pixels, exactly as it does for the default engine, so
    /// the constraint is divided down and the answer multiplied back up.
    ///
    /// The two `Option` arguments are not the parent's dimensions, whatever
    /// their names say: they are this node's own already-computed size on each
    /// axis, present only when that axis was *not* `Auto`. morphorm assigns both
    /// axes from the one pair `content_size` returns, so an axis the layout
    /// already resolved — a stretched cross size, say — has to be echoed back or
    /// it would be overwritten by the content size. That is what makes "stretch
    /// the width, measure the height of the text in it" work.
    fn content_size(
        &self,
        store: &Store,
        sublayout: &mut Self::SubLayout<'_>,
        computed_width: Option<f32>,
        computed_height: Option<f32>,
    ) -> Option<(f32, f32)> {
        let node = &store.nodes[self.0];
        if !node.children.is_empty() {
            return None;
        }
        let leaf = node.measured.as_ref()?;
        let scale_factor = node.scale_factor;

        let cached = {
            let memo = leaf.memo.borrow();
            memo.iter()
                .find(|entry| {
                    same_constraint(entry.width, computed_width)
                        && same_constraint(entry.height, computed_height)
                })
                .map(|entry| entry.measured)
        };
        if let Some(measured) = cached {
            return Some(measured);
        }

        let available_space = Size {
            width: computed_width
                .map(|width| AvailableSpace::Definite(Pixels(width / scale_factor)))
                .unwrap_or(AvailableSpace::MaxContent),
            height: computed_height
                .map(|height| AvailableSpace::Definite(Pixels(height / scale_factor)))
                .unwrap_or(AvailableSpace::MaxContent),
        };
        let known_dimensions = Size {
            width: None,
            height: None,
        };

        let context: &mut dyn MeasureContext = &mut **sublayout;
        let measured = (leaf.measure.borrow_mut())(known_dimensions, available_space, context);
        let measured = (
            computed_width.unwrap_or(measured.width.0 * scale_factor),
            computed_height.unwrap_or(measured.height.0 * scale_factor),
        );

        leaf.memo.borrow_mut().push(MemoEntry {
            width: computed_width,
            height: computed_height,
            measured,
        });
        Some(measured)
    }
}

fn same_constraint(a: Option<f32>, b: Option<f32>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() < 0.01,
        (None, None) => true,
        _ => false,
    }
}

/// Resolves a tree root's extent against the space it was given.
///
/// morphorm reads the root's size with `to_px(0.0, 0.0)`, which is only
/// meaningful for a `Pixels` extent — `Auto` and `Stretch` come out as 0, and a
/// `Percentage` resolves against a parent the root does not have. Both are wrong
/// for a window root, whose percentages belong to the initial containing block
/// and whose `auto` size is the viewport. A `Pixels` extent is the author's own
/// size and is kept.
pub fn root_extent(extent: Units, available: Pixels, scale_factor: f32) -> Units {
    let available = available.0 * scale_factor;
    match extent {
        Units::Pixels(_) => extent,
        Units::Percentage(percent) => Units::Pixels(available * percent / 100.0),
        Units::Stretch(_) | Units::Auto => Units::Pixels(available),
    }
}

/// The morphorm-backed layout tree for a window.
pub struct MorphormLayoutEngine {
    store: Store,
    cache: LayoutCache,
    absolute_bounds: HashMap<LayoutId, Bounds<Pixels>>,
}

impl Default for MorphormLayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MorphormLayoutEngine {
    /// Creates an empty layout engine.
    pub fn new() -> Self {
        Self {
            store: Store::default(),
            cache: LayoutCache::default(),
            absolute_bounds: HashMap::new(),
        }
    }

    /// Builds the node data the morphorm tree owns.
    fn node(
        style: &EngineLayoutStyle,
        rem_size: Pixels,
        scale_factor: f32,
        measured: Option<MeasuredLeaf>,
    ) -> NodeData {
        let display_none = matches!(style.display, Display::None);
        let layout_type = match style.display {
            Display::Grid => LayoutType::Grid,
            Display::Flex => match style.flex_direction {
                FlexDirection::Column | FlexDirection::ColumnReverse => LayoutType::Column,
                FlexDirection::Row | FlexDirection::RowReverse => LayoutType::Row,
            },
            // morphorm's `Overlay` aligns every relative child independently in
            // the padded box; a single-axis stack is what block flow is.
            Display::Block | Display::None => LayoutType::Column,
        };
        let direction = match style.flex_direction {
            FlexDirection::RowReverse => Direction::RightToLeft,
            _ => Direction::LeftToRight,
        };
        let wrap = match style.flex_wrap {
            FlexWrap::NoWrap => LayoutWrap::NoWrap,
            // `WrapReverse` is not expressible; the children wrap forward.
            FlexWrap::Wrap | FlexWrap::WrapReverse => LayoutWrap::Wrap,
        };

        let mut node = NodeData {
            handle: MorphNode(0),
            parent: None,
            children: Vec::new(),
            measured,
            rem_size,
            scale_factor,
            visible: !display_none,
            layout_type,
            position_type: match style.position {
                Position::Absolute => PositionType::Absolute,
                Position::Relative => PositionType::Relative,
            },
            wrap,
            direction,
            alignment: alignment_of(layout_type, style.justify_content, style.align_items),
            size: style.size,
            min_size: style.min_size,
            max_size: style.max_size,
            aspect_ratio: style.aspect_ratio,
            padding: style.padding,
            border_widths: style.border_widths,
            scrollbar_width: style.scrollbar_width,
            scroll_gutter: (
                matches!(style.overflow.x, Overflow::Scroll),
                matches!(style.overflow.y, Overflow::Scroll),
            ),
            stretches_children: stretches_cross(style.align_items),
            margin: style.margin,
            inset: style.inset,
            gap: style.gap,
            flex_basis: style.flex_basis,
            flex_grow: style.flex_grow,
            grid_cols: style.grid_cols,
            grid_rows: style.grid_rows,
            grid_location: style.grid_location.clone(),
            parent_cross_extent_definite: false,
            width: Units::Auto,
            height: Units::Auto,
            min_width: Units::Pixels(0.0),
            min_height: Units::Pixels(0.0),
            max_width: Units::Pixels(f32::MAX),
            max_height: Units::Pixels(f32::MAX),
        };
        // Until a parent exists, a node resolves as the root: a row of one, with
        // the authored width and height as authored.
        node.resolve(LayoutType::Row, false);
        node
    }

    /// Appends a node and records its children as its own.
    ///
    /// The children are *not* resolved here: an axis depends on the parent's
    /// layout type and on whether the parent's cross extent is definite, and the
    /// root's own size can still change after the tree is built — the facade
    /// calls `stretch_auto_size_to_fill` on a window root once the tree exists.
    /// Resolution therefore happens in one pass from the root at
    /// `compute_layout` time, when everything upstream is settled.
    fn push(&mut self, mut node: NodeData, children: &[LayoutId]) -> LayoutId {
        let index = self.store.nodes.len();
        node.handle = MorphNode(index);
        node.children = children.iter().map(|id| MorphNode(id.0 as usize)).collect();
        self.store.nodes.push(node);
        for id in children {
            self.store.nodes[id.0 as usize].parent = Some(index);
        }
        LayoutId(index as u64)
    }

    /// Resolves every node's axes, top down, so the main axis, the cross axis and
    /// whether the cross axis may stretch are all known before morphorm runs.
    fn resolve_tree(&mut self, root: usize) {
        let mut stack = vec![root];
        while let Some(index) = stack.pop() {
            let layout_type = self.store.nodes[index].layout_type;
            let cross_extent_definite = self.store.nodes[index].cross_extent_definite();
            // CSS stretches to the line's cross size only when the container's is
            // definite; see `cross_extent_definite`.
            let stretches =
                self.store.nodes[index].stretches_children && cross_extent_definite;
            let children = self.store.nodes[index].children.clone();

            for child in children {
                let child_index = child.0;
                let node = &mut self.store.nodes[child_index];
                node.parent_cross_extent_definite = cross_extent_definite;
                node.resolve(layout_type, stretches);
                stack.push(child_index);
            }
        }
    }
}

impl LayoutEngine for MorphormLayoutEngine {
    fn clear(&mut self) {
        self.store.nodes.clear();
        self.cache.bounds.clear();
        self.absolute_bounds.clear();
    }

    fn request_layout(
        &mut self,
        style: &EngineLayoutStyle,
        rem_size: Pixels,
        scale_factor: f32,
        children: &[LayoutId],
    ) -> LayoutId {
        let node = Self::node(style, rem_size, scale_factor, None);
        self.push(node, children)
    }

    fn request_measured_layout(
        &mut self,
        style: &EngineLayoutStyle,
        rem_size: Pixels,
        scale_factor: f32,
        measure: BoxedMeasureFn,
    ) -> LayoutId {
        let measured = MeasuredLeaf {
            measure: RefCell::new(measure),
            memo: RefCell::new(Vec::new()),
        };
        let node = Self::node(style, rem_size, scale_factor, Some(measured));
        self.push(node, &[])
    }

    fn stretch_auto_size_to_fill(&mut self, id: LayoutId, size: Size<Pixels>, scale_factor: f32) {
        let node = &mut self.store.nodes[id.0 as usize];
        // Only `auto` axes stretch; an authored size is preserved. This is what
        // makes a window root behave like the root element on the web.
        if node.width == Units::Auto {
            node.width = Units::Pixels(size.width.0 * scale_factor);
        }
        if node.height == Units::Auto {
            node.height = Units::Pixels(size.height.0 * scale_factor);
        }
    }

    fn compute_layout(
        &mut self,
        id: LayoutId,
        available_space: Size<AvailableSpace>,
        scale_factor: f32,
        context: &mut dyn MeasureContext,
    ) {
        let root = id.0 as usize;
        // morphorm sizes the tree root from the root's own properties, and reads
        // them through `to_px(0.0, 0.0)`. For a root that is only meaningful for a
        // `Pixels` extent: `Auto` and `Stretch` become 0, and a `Percentage` is
        // resolved against a parent that does not exist, so it becomes 0 too.
        //
        // In CSS a root's percentages resolve against the initial containing
        // block — the viewport — and an `auto` root is the viewport's size, which
        // is what the facade's own `stretch_auto_size_to_fill` call is for. So a
        // root's extent is resolved against `available_space` here, and an
        // authored `Pixels` is left alone. A percentage root is the case that
        // matters in practice: a view that fills its window says `size_full`,
        // which is `100%` in both axes.
        if let AvailableSpace::Definite(width) = available_space.width {
            self.store.nodes[root].width =
                root_extent(self.store.nodes[root].width, width, scale_factor);
        }
        if let AvailableSpace::Definite(height) = available_space.height {
            self.store.nodes[root].height =
                root_extent(self.store.nodes[root].height, height, scale_factor);
        }

        // Only now, with the root's size settled, can the axes be resolved.
        self.resolve_tree(root);

        let node = self.store.nodes[root].handle;
        let mut sublayout: &mut dyn MeasureContext = context;
        node.layout(
            &mut self.cache,
            &self.store,
            &self.store,
            &mut sublayout,
        );
    }

    fn layout_bounds(&mut self, id: LayoutId, scale_factor: f32) -> Bounds<Pixels> {
        if let Some(bounds) = self.absolute_bounds.get(&id) {
            return *bounds;
        }

        // morphorm stores each node's position relative to its parent, so the
        // window-relative origin is the sum along the path to the root. Collected
        // into a chain and summed downwards rather than recursed, so a deep tree
        // cannot overflow the stack.
        let mut chain = Vec::new();
        let mut current = Some(id.0 as usize);
        while let Some(index) = current {
            chain.push(index);
            current = self.store.nodes[index].parent;
        }
        let mut origin = Point::new(0.0, 0.0);
        for index in chain.iter().rev() {
            let node = MorphNode(*index);
            origin.x += self.cache.posx(&node);
            origin.y += self.cache.posy(&node);
        }

        let node = MorphNode(id.0 as usize);
        let computed = Size {
            width: self.cache.width(&node),
            height: self.cache.height(&node),
        };
        let far = origin + Point::from(computed);

        // Snapping matches the default engine: round in device-pixel space so
        // painted edges land on physical pixel boundaries, and round midpoints
        // toward zero so a 1-logical-pixel line at 150% stays 1 device pixel.
        let snapped = Bounds::from_corners(
            origin.map(round_half_toward_zero),
            far.map(round_half_toward_zero),
        );
        let bounds = (snapped / scale_factor).map(Pixels);
        self.absolute_bounds.insert(id, bounds);
        bounds
    }
}
