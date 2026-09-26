# A second LayoutEngine: taffy against morphorm — 2026-09-26

The layout seam had no second implementation, which is what the site said about
it: *"taffy is the only published implementation of `LayoutEngine`, so the layout
swap has no second column until this exists."* This is that second column.

`Application::with_layout_engine` takes a factory, so an engine is chosen at
startup and the authoring layer never names it. `gpui_engine` says the same thing
from the other side — it has *"no dependency on `taffy`"*, and refers to a node
only through an opaque `LayoutId`. This project is what tests whether those two
statements are load-bearing: a layout solver written outside the tree, behind the
same trait, on the same tree.

**The run below is this repository's own.** The measurements were first taken
inside the zed checkout, where the project inherited that checkout's pin
(`1.95.0`); they were re-taken here under the toolchain this repository declares
(`1.98.1`) rather than carried over, because a number is the compiler's as much as
the code's. The source did not change between the two — only the import path, the
manifest and the compiler — and the result is the same claim with a different
compiler: **3.3–3.7×** there, **3.4–3.8×** here.

## Command

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_BENCH_DEBUG=0 \
  cargo bench --bench layout_engine
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test --test layout_engine -- --nocapture
# the frame the facade draws, which is a different question:
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test --features test-support --test facade -- --nocapture
```

| | |
| --- | --- |
| toolchain | `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1` — this repository's own pin, the same one `bite-gpui/gpui_parley` declares |
| host | Intel i7-8750H, 12 threads, Linux 7.0.0 x86_64 — **not a controlled benchmarking host**: a laptop CPU, no pinning, no fixed clocks |
| samples | criterion defaults, 100 measurements per benchmark |
| crates | `bite-gp-engine` `1.21.0` (the trait), `bite-gp-engine-default` `1.21.0` (the taffy column), `morphorm` `0.9.0`, `taffy` `0.13.0` (transitively, via the default engine), `criterion` `0.5.1` |

## The tree

One builder, `src/sample.rs`, drives both engines — so "the engines agree on this
tree" and "here is what each engine costs on this tree" are statements about the
same thing. Twelve nodes:

```
row 400×300, padding 10, gap 8
├── column          flex_grow 1
│   ├── 60×20
│   ├── 60×20
│   ├── column gap 4 ── 24×12, 24×12
│   └── measured leaf (auto/auto)
├── column width 100, gap 4
│   ├── 50% width, height 20
│   └── 40×40
└── absolute, right 5, top 5, 30×30
```

It exercises: flex rows and columns, padding, gaps, explicit and percentage
sizes, `flex_grow`, cross-axis stretch, one measured leaf, and absolute
positioning. It deliberately avoids the constructs the mapping cannot express
(see below), because a difference there would be a documented limitation rather
than a defect.

## Correctness first

Two engines that disagree are not two columns, they are one column and a bug, so
the bounds are compared before anything is timed:

```
scale 1:   worst delta 0.00 px, 0 of 12 nodes differ
scale 1.5: worst delta 0.00 px, 0 of 12 nodes differ
scale 2:   worst delta 0.00 px, 0 of 12 nodes differ
50 buttons, frame 0:   worst delta 0.00 px, 0 of 176 nodes differ
50 buttons, frame 7:   worst delta 0.00 px, 0 of 176 nodes differ
200 buttons, frame 3:  worst delta 0.00 px, 0 of 701 nodes differ
1000 buttons, frame 3: worst delta 0.00 px, 0 of 3501 nodes differ
```

Every node agrees to the printed precision at an integral scale *and* at two
fractional ones, which is where device-pixel snapping would expose a mismatch if
the two resolved lengths differently on the way in. The thousand-button case is
part of the committed test rather than a number reported from a one-off run,
because it is the upper bound the numbers below are quoted with.

## A transparent window, and what it took to see it

The comparison above passed while the engine painted nothing in a real window.
That is not a contradiction, it is two different questions: the tree here is
built by hand and laid out against an available space, while a facade builds a
window root, stretches it to the window, and reads bounds while painting. An
engine can match the default one node for node on a constructed tree and still
fill a window with nothing.

The window root is the reason. gpui's root is `size_full` — `100%` in both axes —
and morphorm reads a root's extent with `to_px(0.0, 0.0)`, which resolves a
percentage against a parent the root does not have. Zero by zero, and a zero-size
root paints nothing: the window is transparent. Taffy instead lays the root out
inside the available space, as CSS resolves a root against the initial containing
block, so the same tree drew fine.

`src/gpui_morphorm.rs` now resolves a root's extent against `available_space`
before laying out. `tests/facade.rs` is what found it, and is the reason it
cannot come back: it installs this engine with `set_layout_engine_factory`, draws
a frame with `TestAppContext`, and asserts on `painted_quads` — the same view,
drawn twice, once per engine, with the geometry compared. Both engines now paint:

```
root  0,0  1600x1200
row   16,16  1568x40
row   16,64  1568x40
```

(The test platform runs at a scale factor of 2, which is why the numbers are
twice the 800×600 the test resizes to.)

## Under load: changing buttons

A twelve-node tree finishes in tens of microseconds, so its two columns are too
close to the floor for the ratio to mean much. `src/buttons.rs` is the tree worth
timing: fifty to a thousand buttons in two columns, each with an icon whose width
and a row whose height change every frame, so nothing repeats and neither engine
can serve a stale layout.

| buttons | nodes | `taffy` | `morphorm` | ratio |
| --- | --- | --- | --- | --- |
| 50 | 176 | 329.5 µs | 96.7 µs | **3.4×** |
| 200 | 701 | 1.281 ms | 379.6 µs | **3.4×** |
| 1000 | 3501 | 7.305 ms | 2.002 ms | **3.7×** |

Intervals: 328.88–330.21 µs against 96.329–97.227; 1.2782–1.2833 ms against
375.96–384.29 µs; 7.0408–7.5900 ms against 1.9333–2.0816. The second run put the
three ratios at 3.48×, 3.49× and 3.82×, so the headline is a range:
**roughly 3.4–3.8×**, and the same tree is laid out the same way by both engines
(0.00 px across all 3501 nodes at 1000 buttons).

Notably the ratio *holds* as the tree grows rather than converging, which is what
makes the absolute numbers the interesting ones. On a 16.7 ms frame budget:

| buttons | `taffy` | `morphorm` |
| --- | --- | --- |
| 50 | 2.0% | 0.6% |
| 200 | 7.7% | 2.3% |
| 1000 | **44%** | 12% |

Which is the point of the exercise: at a thousand changing buttons, the default
engine spends nearly half the frame on layout and this one spends an eighth. The
demo in `examples/layout_demo.rs` renders exactly this tree and reports the
pipeline's layout phase, so the two can be compared on screen
(`BITE_LAYOUT_ENGINE=taffy cargo run --features demo --example layout_demo`
against the same without the variable, `BITE_BUTTONS=1000` to make it
unmissable).

## Results: the twelve-node tree

Mean estimates, with criterion's 95% interval. Run twice, because this host is
not a controlled one and the second run is the honest way to say by how much the
numbers move.

| group | run 1 `taffy` | run 1 `morphorm` | run 2 `taffy` | run 2 `morphorm` | ratio |
| --- | --- | --- | --- | --- | --- |
| `frame` — `clear` + build + `compute_layout` | 21.747 µs | 6.229 µs | 20.064 µs | 5.849 µs | **3.5×** / **3.4×** faster |
| `bounds` — `layout_bounds` for all 12 nodes | 2.287 µs | 2.218 µs | 2.328 µs | 1.919 µs | **1.03×** / **1.21×** faster |

Run-1 intervals: `frame` 21.386–22.039 µs against 6.2124–6.2482; `bounds`
2.0647–2.5012 against 2.1856–2.2460. Run-2 intervals: `frame` 19.780–20.292
against 5.7845–5.9361; `bounds` 2.1914–2.4576 against 1.8528–1.9828.

So the headline for this tree is **roughly 3.4× per frame**, and `bounds` is the
one figure here that is not stable: it was 1.03× and 1.21× in these two runs,
against 1.28× and 1.42× in the two taken on the older compiler, and run 1's
`taffy` interval spans 2.065–2.501 µs on its own. Read it as "comparable, both
sub-three-microseconds", not as a ratio. The bottleneck in `bounds` is
`layout_bounds`, called 12 times, and at that scale the call and the arithmetic
are the same order as the difference.

The `frame` ratio, by contrast, repeated: 3.49× and 3.43× here, 3.8× and 3.1× on
the older compiler. The correctness result did not move at all between any of the
runs; only the clocks did.

## What it says

**Two separate costs, and morphorm is cheaper at the one that matters per frame.**
`frame` is the cycle the facade starts every frame with; `bounds` is what painting
asks for, once per node.

**Why `bounds` is close at all is structural.** taffy hands back a node's
parent-relative location and the default engine accumulates it while walking
parents, caching each result. morphorm's cache stores parent-relative positions
too, and this engine accumulates the same way — so the two are doing comparable
work, and any gap is per-node bookkeeping rather than a different idea.

**What is being measured is the whole engine, not the algorithm.** `frame`
includes each engine's node storage: this one keeps `NodeData` in a `Vec` and
indexes it, taffy keeps a `TaffyTree` over a slotmap. Both cache measured sizes
(taffy internally, this one with a per-leaf memo) so that the measured leaf does
not tilt the result. The ratio is a claim about these two engines as written, not
about taffy and morphorm in the abstract.

**taffy implements more of CSS.** It has block flow, wrapping modes, grid track
sizing with `minmax` and `fr`, `flex_shrink`, and independent `justify_content`
and `align_items`. morphorm has fewer concepts, which is part of why it is
cheaper here, and the mapping below is what that costs.

## What it does not say

- **Not a general claim.** One tree, twelve nodes, one host, and 20–25%
  run-to-run spread on the timings.
- **Not attributable to the algorithm alone.** See above: storage and caching are
  inside the measurement.
- **The conformance test is not a proof of equivalence.** It is a comparison on
  two trees built from the subset the mapping claims. It says the mapping is right
  where it claims to be right; it says nothing about the cases it declines.

## Where the mapping ends

The vocabularies are not the same shape. `EngineLayoutStyle` is a set of copies
of taffy's style enums; morphorm reads its own. `src/gpui_morphorm.rs` records
every choice, and the substance is:

| construct | what happens |
| --- | --- |
| `flex_grow` | `Units::Stretch(factor)`, and **not** equivalent — see below |
| `flex_shrink` | **dropped** — morphorm has no shrink factor |
| `flex_basis` | a definite basis wins over `size`, which wins over `flex_grow` |
| `align_items: Stretch` | `Units::Stretch(1.0)` on an `auto` cross axis, **only when the parent's cross extent is definite** |
| a measured leaf | both axes come from one hook, so a resolved axis is echoed back |
| the tree root | its extent is resolved against `available_space`; morphorm's own root sizing reads `Percentage` and `Auto` as zero |
| `justify_content` + `align_items` | collapse into morphorm's single nine-way `Alignment` |
| `SpaceBetween`/`SpaceEvenly`/`SpaceAround`, `Baseline`, `align_self` | **no equivalent** — fall back to the start of the axis |
| margin on an in-flow child | **dropped** — morphorm reads spacing only for absolute children |
| `border_widths`, scrollbar gutter | folded into `padding`, which is the same inset |
| `ColumnReverse`, `WrapReverse` | **no equivalent** — laid out as their forward forms |
| `position: relative` + `inset` | **dropped**, as CSS ignores it at layout time |
| grid track `MinContent`/`MaxContent` | `Auto`; `GridTemplateMinSize::Zero` becomes `Stretch(1.0)` |
| grid item placement | **not generated** — morphorm reads each child's `column_start`/`row_start` and defaults both to 0, so `index % columns` placement has to be synthesised or every item lands in the first cell |

Four of those were found by testing rather than by reading, and three of them
produce a *different layout* rather than a crash, which is why they are written
down here and not left implicit:

- **`flex_grow` is not equivalent.** CSS grows an item *beyond its own content*;
  morphorm's `Stretch` divides the free space by factor *from zero*. One grown
  item and the two agree, which is why the twelve-node tree can use it. Two grown
  items carrying different content and they do not: two `flex_grow: 1` buttons
  whose labels differ came out 170 and 168 px wide under taffy and 169 and 169
  under morphorm.
- **An auto cross axis with unequal children.** CSS stretches the auto-cross
  children of a row to the tallest sibling; morphorm gives each its own content
  height. `align_items: Stretch` is therefore only mapped where the container's
  cross extent is definite, because stretching against an auto extent hands the
  child zero and collapses the parent with it.
- **A measured leaf is sized from one hook.** morphorm assigns *both* axes from
  the single `content_size` answer, where taffy resolves each axis on its own, so
  a stretched width with a measured height — the ordinary case for a run of text
  in a column — only works if the hook echoes back the axis the layout already
  resolved.
- **A percentage root paints nothing.** Covered above, and the only one of the
  four that was invisible to the conformance comparison.

So the buttons tree deliberately uses none of the first three: explicit widths
instead of `flex_grow`, explicit row heights instead of an auto cross axis, and a
column of rows instead of a grid. What it measures is what both engines agree on;
the rest is listed rather than measured.

## Files

- `src/gpui_morphorm.rs` — the engine: `MorphormLayoutEngine`, the style
  conversion, and the mapping notes.
- `src/sample.rs` — the twelve-node tree, used by the comparison and the first two
  benchmark groups.
- `src/buttons.rs` — the fifty-to-a-thousand-button tree behind the third.
- `tests/layout_engine.rs` — the two-column comparison, over both trees.
- `tests/facade.rs` — one frame drawn through the facade, twice, once per engine.
  The transparent window lives here now.
- `benches/layout_engine.rs` — the timings above.
- `examples/layout_demo.rs` — the swap, at the facade, rendering the button tree
  live.
