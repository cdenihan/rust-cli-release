# rust-cli-release

`rust-cli-release` is a private distribution kit for Rust command-line
applications. It centralizes checksum-verified self-update, branded installers,
cross-platform CI, calendar or manifest releases, native artifacts, and GitHub
Release publication while each consumer owns its product commands.

## Runtime crate

Consumers pin an immutable toolkit tag in both dependency sections because the
same crate provides runtime and build-script helpers:

```toml
[dependencies]
rust-cli-release = { git = "https://github.com/cdenihan/rust-cli-release", tag = "v1.1.1" }

[build-dependencies]
rust-cli-release = { git = "https://github.com/cdenihan/rust-cli-release", tag = "v1.1.1" }
```

For a calendar-versioned application, `build.rs` becomes:

```rust
fn main() {
    rust_cli_release::emit_version_file(
        "VERSION",
        "MY_CLI_SOURCE_VERSION",
        rust_cli_release::VersionFormat::Calendar,
    )
    .expect("invalid release version");
}
```

The application defines its identity once and passes it to the updater:

```rust
use rust_cli_release::ReleaseSpec;

const RELEASE: ReleaseSpec = ReleaseSpec::new(
    "my-cli",
    "My CLI",
    "owner/my-cli",
    "MY_CLI",
    env!("MY_CLI_SOURCE_VERSION"),
);

let summary = rust_cli_release::update_current(&RELEASE, "latest", false)?;
```

See `examples/minimal.rs` for a complete Clap command.

## Reusable workflows

Private consumer repositories need three thin callers. Pin the same immutable
tag used by Cargo and pass the private dependency token explicitly.

```yaml
jobs:
  ci:
    uses: cdenihan/rust-cli-release/.github/workflows/rust-ci.yml@v1.1.1
    with:
      binary-name: my-cli
      display-name: My CLI
      environment-prefix: MY_CLI
    secrets:
      dependency_token: ${{ secrets.RUST_CLI_RELEASE_TOKEN }}
```

The prepare caller runs on the trigger chosen by the product repository and
calls `prepare-release.yml`. Calendar mode creates `YYYY.MM.DD.N`, updates the
Cargo package and `VERSION`, pushes a bot commit, moves the reserved tag, and
dispatches the product's thin `publish-release.yml` caller. Manifest mode tags
the existing Cargo package version without a version commit.

The publish caller exposes the dispatch inputs and passes them unchanged to
`publish-release.yml`. The reusable workflow builds eight Windows, macOS, GNU
Linux, and musl Linux artifacts, adds SHA-256 files and branded installers, and
publishes the release only after verifying tag, commit, and compiled version.

## Private access and credentials

In this repository's Settings > Actions > General > Access, enable access from
other private repositories owned by `cdenihan`.

Create a fine-grained personal access token restricted to this repository with
read-only Contents permission. Add it to every consumer as both an Actions
secret and a Dependabot secret named `RUST_CLI_RELEASE_TOKEN`. The workflow uses
it only while Cargo fetches the private Git dependency and removes the temporary
Git URL rewrite after each job.

GitHub Packages does not provide a Cargo registry, so v1 deliberately uses a
tagged private Git dependency. Local development uses the developer's normal
GitHub Git credential.

## Updating consumers

Toolkit releases use immutable SemVer tags. Dependabot monitors Cargo and
GitHub Actions separately and opens reviewable updates to the next tag. Update
both references together before merging when a release changes both runtime
and workflow behavior.

## Development

```console
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
python3 -m unittest tests/render-installers.py tests/release-version.py
sh tests/install-sh.sh
```
