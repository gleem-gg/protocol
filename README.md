# gleem-protocol

The wire formats of Gleem Hosting, in one crate, so that a change to any of
them breaks the build on every side that speaks it instead of breaking at
runtime on somebody's machine.

| Module | Contract between |
|---|---|
| `enroll`, `heartbeat`, `command`, `specs`, `capability` | machine agent and gleem.gg |
| `signalling` | machine agent and signalling gateway |
| `ticket` | gleem.gg and signalling gateway: verifies the HMAC tickets `App\Services\GatewayTicketService` mints, byte for byte |

Field names follow the JSON of the Laravel application rather than Rust
naming, because every type here has a counterpart there. The ticket tests
include vectors minted by the PHP side (`cross_language`); keep them passing
when either side changes.

## Using it

The repository is private. Depend on a tag over SSH:

```toml
gleem-protocol = { git = "ssh://git@github.com/gleem-gg/protocol.git", tag = "v0.1.0" }
```

and let cargo use the system git, so your SSH setup applies
(`.cargo/config.toml`):

```toml
[net]
git-fetch-with-cli = true
```

CI and build hosts read it with a read-only deploy key.

## Releasing

Bump `version` in `Cargo.toml`, then create a signed tag `vX.Y.Z` and a
GitHub release. Dependants move by changing the tag they pin. A tag is never
moved once something depends on it.

```sh
cargo test
```
