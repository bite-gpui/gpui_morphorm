# gpui_morphorm

A second implementation of GPUI's `LayoutEngine`, built on
[morphorm](https://crates.io/crates/morphorm).

`gpui_engine::LayoutEngine` is the trait the facade swaps: an application picks
the solver that turns its element tree into laid-out boxes, and the default one is
taffy. This crate is the one that is not the default. It exists because the seam
claimed a solver could be replaced without the authoring layer noticing, and a
seam with nothing on the other side of it is a claim rather than a boundary.

The **package** is `bite-gp-morphorm` and the **library** is `gpui_morphorm`,
which is what code names. The two differ because the distribution's rule is that a
package identifier carries its namespace while a library target keeps the name the
code uses — so a consumer's `Cargo.toml` line changes and its
`use gpui_morphorm::…` does not.

## Using it

```toml
[dependencies]
bite-gpui = "1.21"
bite-gp-morphorm = "1.21"
```

```rust
use gpui::*;
use gpui_morphorm::MorphormLayoutEngine;

fn main() {
    application()
        .with_layout_engine(|| Box::new(MorphormLayoutEngine::new()))
        .run(|cx: &mut App| { /* … */ });
}
```

The package is `bite-gp-morphorm` and the library is `gpui_morphorm`, and **no
rename is needed for that to work**: cargo exposes a dependency under its
*library* name, so the key in `[dependencies]` can be the package name while
`use gpui_morphorm::…` compiles unchanged.

The factory is called once per window, before any of them exist. Nothing in the
view below it names an engine: the same `div()` tree lays out either way, which is
the point of a seam.

The requirement floats within the 1.21 line, whose amendments are `1.21.1`–
`1.21.99`; pin exactly if a build has to be reproducible against one.

## Is it the same layout?

That is the question a second engine has to answer before it is a second column
rather than a bug, so it is answered first and by measurement. `cargo test` lays
the same tree out with both engines and compares every node's bounds:

```
$ cargo test --test layout_engine -- --nocapture
scale 1:   worst delta 0.00 px, 0 of 12 nodes differ
scale 1.5: worst delta 0.00 px, 0 of 12 nodes differ
scale 2:   worst delta 0.00 px, 0 of 12 nodes differ
50 buttons, frame 0:   worst delta 0.00 px, 0 of 176 nodes differ
50 buttons, frame 7:   worst delta 0.00 px, 0 of 176 nodes differ
200 buttons, frame 3:  worst delta 0.00 px, 0 of 701 nodes differ
1000 buttons, frame 3: worst delta 0.00 px, 0 of 3501 nodes differ
```

Integral and fractional scale factors are both covered, because that is where
device-pixel snapping would expose a mismatch if the two resolved lengths
differently on the way in.

`src/sample.rs` and `src/buttons.rs` build the trees; `tests/layout_engine.rs`
lays each out with both engines; `benches/layout_engine.rs` times the same trees
through the same trait. The trees are those files, so the comparison and the
timings cannot drift apart.

They cover flex rows and columns, padding, gaps, explicit and percentage sizes,
`flex_grow` (in the twelve-node tree only, for the reason below), cross-axis
stretch, one measured leaf, and absolute positioning. They deliberately *exclude*
what the conversion cannot express, because a difference there would be a
documented limitation rather than a failure.

## Does a window come out of it?

Agreeing node for node is not the same as drawing a window, and the first version
of this engine painted **nothing** — a transparent window — while passing every
comparison above.

The window root is why. gpui's root is `size_full`, which is `100%` in both axes,
and morphorm reads a root's extent with `to_px(0.0, 0.0)`: a percentage resolves
against a parent the root does not have, so the root measured zero by zero and a
zero-size root paints nothing. Taffy lays the root out inside the available
space, as CSS resolves a root against the initial containing block.

The engine now resolves a root's extent against `available_space` first, and
`tests/facade.rs` is what found it and what keeps it fixed:

```
$ cargo test --features test-support --test facade -- --nocapture
root  0,0  1600x1200
row   16,16  1568x40
row   16,64  1568x40
```

It installs the engine with `set_layout_engine_factory`, draws a frame with
`TestAppContext`, and asserts on `painted_quads` — once with the default engine
and once with this one, comparing the geometry. That calibration test matters: it
is what says the harness is right when the swapped one fails. (The test platform
runs at a scale factor of 2, which is why the numbers are twice the 800×600 the
test resizes to.)

## Where the vocabularies differ

`EngineLayoutStyle` is a set of copies of taffy's style enums. morphorm reads its
own, and they are not the same shape, so most of `src/gpui_morphorm.rs` is not a
rename. The substance:

| construct | what happens |
| --- | --- |
| `flex_grow` | `Units::Stretch(factor)`, and **not** equivalent |
| `flex_shrink` | dropped — morphorm has no shrink factor |
| `align_items: Stretch` | `Units::Stretch(1.0)`, only when the parent's cross extent is definite |
| `justify_content` + `align_items` | collapse into one nine-way `Alignment` |
| `SpaceBetween`/`Evenly`/`Around`, `Baseline`, `align_self` | no equivalent; start of the axis |
| margin on an in-flow child | dropped — morphorm reads spacing only for absolute children |
| `border_widths`, scrollbar gutter | folded into `padding` |
| `ColumnReverse`, `WrapReverse` | no equivalent; laid out forward |
| `position: relative` + `inset` | dropped, as CSS ignores it at layout time |
| grid track `MinContent`/`MaxContent` | `Auto`; `GridTemplateMinSize::Zero` becomes `Stretch(1.0)` |
| grid item placement | not generated — every item lands in the first cell |
| the tree root | its extent is resolved against `available_space` |

The module docs in `src/gpui_morphorm.rs` carry the full list with the reasoning.
Four of them were found by testing rather than by reading, and three produce a
different *layout* rather than a crash: `flex_grow` (CSS grows from content,
morphorm's `Stretch` divides free space from zero — two `flex_grow: 1` buttons
with different labels came out 170/168 under taffy and 169/169 under morphorm), an
auto cross axis with unequal children, a measured leaf's single intrinsic-size
hook, and the percentage root above.

So the buttons tree deliberately uses none of the first three: explicit widths
instead of `flex_grow`, explicit row heights instead of an auto cross axis, and a
column of rows instead of a grid. What it measures is what both engines agree on;
the rest is listed rather than measured.

## Running it

```sh
cargo test                                              # the two-column comparison
cargo bench --bench layout_engine                       # both columns, both trees
cargo test --features test-support --test facade         # a frame through the facade
cargo run --features demo --example layout_demo          # the swap, in a window
BITE_BUTTONS=1000 cargo run --features demo --example layout_demo
BITE_LAYOUT_ENGINE=taffy cargo run --features demo --example layout_demo
```

`layout_demo` needs a display and a GPU, so it is not part of the automated
suite — but the frame it draws is, in `tests/facade.rs`. The numbers are in
[`benchmarks/2026-09-26-layout-engine-swap.md`](benchmarks/2026-09-26-layout-engine-swap.md),
with the command, toolchain, host and every result.

## Where this code came from

It was the `layout-engine` project of `bite-gpui`'s usage suite — the projects
that consume the published crates rather than publishing them — where it was the
one entry in the taxonomy's category 1 that could not be linked to a repository,
because there was not one. It is now here, and the usage suite's other projects
depend on the published crate like any other consumer.

Nothing about the engine changed on the way in. What changed is the packaging: the
manifest was written against published `bite-gp-*` versions instead of the
checkout it sat in, the library target was given the name the code already used,
and the demo became an example behind a feature. The numbers were then re-taken
under this repository's own toolchain rather than carried over, because a number
is the compiler's as much as the code's — the recorded run names both.

## Layout

- `src/gpui_morphorm.rs` — the engine: `MorphormLayoutEngine`, the style
  conversion, and the mapping notes.
- `src/sample.rs` — the twelve-node tree the comparison and the first two
  benchmark groups share.
- `src/buttons.rs` — the fifty-to-a-thousand-button tree.
- `tests/layout_engine.rs` — the conformance comparison, over both trees.
- `tests/facade.rs` — one frame drawn through the facade, twice, once per engine.
- `benches/layout_engine.rs` — the two columns.
- `examples/layout_demo.rs` — the swap at the facade, rendering the button tree
  live. Needs the `demo` feature.

## Licence

Apache-2.0 — see `LICENSE-APACHE`.

## Publishing

The tag **is** the release. `.github/workflows/release.yml` publishes to crates.io
and is `gpui_parley`'s publisher: a tag that has to agree with the manifest, a dry
run by default, a `crates-io` environment approval before the upload, and the
registry token checked before anything is built. Unlike that repository's, this
one's `package` job also builds the demo example, because the example is the
thing the README tells a reader to run and `cargo test` skips it — it declares
`required-features`.

```sh
# what will be released, without publishing it
gh workflow run release.yml -f tag=v1.21.2

# the release itself: push the tag, then approve the environment
git tag v1.21.2 && git push origin v1.21.2
```

Versioning is the distribution's, not a fresh count. `docs/contract.md` §6 in
`bite-gpui/distribution` numbers a release `major.minor.(patch * 100 + amendment)`:
`1.21.0` is a base and its amendments run `1.21.1`–`1.21.99`, with `1.21.100`
reserved for the next upstream patch. `1.21.1` is `bite-gp-parley`'s first
amendment, so `1.21.2` is this crate's slot on the same line.

A crates.io token's scope is a **name prefix glob**, and the token the layer stack
publishes with is scoped to `bite_*`, which does *not* match `bite-gp-morphorm`
because of the hyphen. An upload with it is refused as `403 Forbidden: this token
does not have the required permissions to perform this action`, after everything
has been built. The token that publishes this crate has to cover its name.
