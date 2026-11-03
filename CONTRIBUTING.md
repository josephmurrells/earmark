# Contributing to Earmark

Thanks for helping. Bug reports, ideas and pull requests are all welcome.

## Reporting bugs

Use the [issue forms](https://github.com/josephmurrells/earmark/issues/new/choose). The most useful
things to include are:

- the Earmark version (in the release zip's name, or **Properties → Details** on `earmark.exe`);
- your headphones and how they're connected (wired, USB DAC, Bluetooth);
- which test you ran (one ear at a time or both together, and the length);
- if Earmark crashed, `%APPDATA%\Earmark\data\crash.log`.

Please don't send your hearing results unless you want to. They're personal, and most bugs can be
reproduced without them.

## Building and testing

You need Rust 1.95+ and the MSVC toolchain (Visual Studio Build Tools with the C++ workload). Before
opening a pull request, run:

```powershell
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same checks on `windows-latest`, plus a `cargo check` on the minimum supported Rust
version (1.95).

To check UI changes, render every screen in light and dark mode to `%TEMP%\earmark-*.png`:

```powershell
cargo test screenshots -- --ignored
```

The README images in `docs/screenshots` are the `test`, `tune` and `use` screens from that run.
Update them when a change alters those screens.

## Code style

- Idiomatic Rust, formatted with `rustfmt` (see `rustfmt.toml`), with no clippy warnings.
- Keep the UI simple: one main action per screen, advanced options folded away, and the look of
  the existing cards, buttons and charts in `src/widgets.rs`, `src/charts.rs` and `src/theme.rs`.
- Changes to the hearing test (`src/staircase.rs`, `src/hearing_test.rs`) or the EQ design
  (`src/eq_design.rs`) need tests, and a pull request that explains what changes for the listener,
  such as test length or EQ shape.
- Earmark isn't a medical device. Don't add wording or features that suggest it can diagnose
  hearing loss.

## Releasing

1. Bump `version` in `Cargo.toml`, run `cargo check` so `Cargo.lock` follows, and merge that
   through a pull request.
2. Tag the merged commit on master and push the tag: `git tag vX.Y.Z` then
   `git push origin vX.Y.Z`.
3. The Release workflow tests, builds and packages the app, then creates a **draft** release with
   the zip, `SHA256SUMS.txt` and build attestations.
4. Check the draft's files and notes, then publish it.

## Licensing of contributions

Earmark is licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at
your option. Unless you say otherwise, any contribution you submit for inclusion in Earmark is
dual-licensed in the same way, without any additional terms or conditions, and you confirm it's
your own work or that you have the right to submit it.
