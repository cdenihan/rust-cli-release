# rust-cli-release

`rust-cli-release` is an open-source distribution kit for Rust command-line
applications. It centralizes checksum-verified self-update, branded installers,
cross-platform CI, calendar or manifest releases, native artifacts, and GitHub
Release publication while each consumer owns its product commands.

## Runtime crate

Consumers pin an immutable toolkit tag in both dependency sections because the
same crate provides runtime and build-script helpers:

```toml
[dependencies]
rust-cli-release = { git = "https://github.com/cdenihan/rust-cli-release", tag = "v1.3.2" }

[build-dependencies]
rust-cli-release = { git = "https://github.com/cdenihan/rust-cli-release", tag = "v1.3.2" }
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

## Secure application data

`SecureDir` resolves a private, owner-only directory (`~/.<app_name>` by
default, or an explicit override for tests and `--config-dir`-style flags),
with atomic writes and 0600/0700 permissions on Unix:

```rust
use rust_cli_release::SecureDir;

let dir = SecureDir::discover("my-cli", None)?;
dir.write_private("identity.key", &secret_bytes)?;
```

`LockedJsonStore<T>` layers a lock-guarded, atomically-persisted JSON value on
top of a `SecureDir` for state that multiple invocations may touch
concurrently (peer lists, sync cursors, and similar small documents):

```rust
use rust_cli_release::{LockedJsonStore, SecureDir};

let store: LockedJsonStore<MyState> =
    LockedJsonStore::new(SecureDir::discover("my-cli", None)?, "state.json");
store.update(|state| {
    state.last_sync = now();
    Ok(())
})?;
```

`update` takes an exclusive advisory lock for the full load-modify-save cycle,
so concurrent processes serialize instead of racing.

When a whole operation needs to be serialized rather than a single document —
a sync, a migration, a multi-file rewrite — take the lock directly:

```rust
let dir = SecureDir::discover("my-cli", None)?;
match dir.try_lock_exclusive("sync.lock")? {
    Some(_guard) => { /* held until _guard drops */ }
    None => println!("another run is already in progress"),
}
```

`lock_exclusive` waits for the current holder; `try_lock_exclusive` reports
`None` immediately so a command can say a concurrent run is in progress instead
of appearing to hang. The operating system releases the lock if the process
exits or is killed, so an interrupted run never leaves a stale lock behind.

## Reusable workflows

Consumer repositories need three thin callers. Pin the same immutable tag used
by Cargo. Public consumers do not need credentials; the optional token remains
available for private forks or private Git dependencies.

```yaml
jobs:
  ci:
    uses: cdenihan/rust-cli-release/.github/workflows/rust-ci.yml@v1.3.2
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

Rust build caches use separate trust domains. Pull requests may restore the
read-only cache produced by the default branch, but cannot save cache entries.
Release builds use a distinct cache namespace and can access it only after a
preflight job verifies a default-branch dispatch, the immutable release tag,
and the exact tagged commit. Cached Cargo executables and failed builds are
never saved. Matrix targets are encoded directly in their shared cache keys so
different architectures cannot collide. Release dispatches run from the
default branch so this trusted cache remains reusable across version tags.

Generated POSIX installers use musl binaries by default on Linux to avoid a
dependency on the host's glibc version. Consumers that specifically need the
GNU build can pass `--libc gnu`; the branded `<PREFIX>_LIBC=gnu` environment
variable provides the same override. Both GNU and musl artifacts continue to
be built and published for x86_64 and ARM64.

## Optional private access

No token is needed to consume this public repository through Cargo, GitHub
Actions, or Dependabot. The `dependency_token` workflow secret is optional and
all credential setup steps are skipped when it is unset.

For a private fork or other private Git dependency, create a fine-grained token
restricted to the required repositories with read-only Contents permission.
Store it in the consumer as an Actions secret named
`RUST_CLI_RELEASE_TOKEN` and pass it as `dependency_token`. Temporary Git URL
credentials are removed after each job. If Dependabot needs the same private
access, store the token separately as a Dependabot secret and configure a `git`
registry in the consumer's Dependabot file.

Private reusable-workflow hosts must also enable access for their intended
private consumers in Settings > Actions > General > Access. GitHub does not
allow public repositories to call workflows stored in private repositories.

GitHub Packages does not provide a Cargo registry, so v1 deliberately uses a
tagged Git dependency. Private forks use the optional credential path described
above; public consumers fetch the tag anonymously.

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
python3 -m unittest tests/render-installers.py tests/release-version.py tests/workflow-cache.py
sh tests/install-sh.sh
```
