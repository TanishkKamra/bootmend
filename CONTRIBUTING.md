# Contributing to Bootmend

Thanks for helping. Bootmend is an early version, so a clear bug report helps as much as code.

## Report a problem

Use **Report a problem** in Bootmend (on the notice it shows at launch, or under Help in Settings), or open the [bug form](https://github.com/TanishkKamra/bootmend/issues/new?template=bug_report.yml) directly. Everything on GitHub is public, so leave out anything private.

Security problems are different: report them privately, as [SECURITY.md](SECURITY.md) explains.

## Suggest an idea

Open an [issue](https://github.com/TanishkKamra/bootmend/issues/new) saying what you'd like Bootmend to do, and why.

## Send a fix

1. Fork the repository and make a branch from `main`.
2. Build and test it as the README describes. `cargo fmt`, `cargo clippy --all-targets` and `cargo test` should all pass with no warnings.
3. Keep each pull request to one change, and say what it fixes and how you checked it.
4. Bootmend is written for people who aren't technical. Anything people read should be short and plain; [PRODUCT.md](PRODUCT.md) and [DESIGN.md](DESIGN.md) explain the voice and the look.
5. Tests use sample data and temporary folders. Tests marked `#[ignore]` touch the real PC, so read one before you run it.

By sending a pull request, you agree that your contribution is licensed under the GNU GPL v3 or later, like the rest of Bootmend.
