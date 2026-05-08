# path: iON/crates/iON_core/Cargo.toml
[package]
name = "iON_core"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1.0"
bitflags = "2.6"
chrono = { version = "0.4", features = ["serde"] }
hex = "0.4"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
sha2 = "0.10"
zeroize = "1.8"

# Optional signing verification (recommended).
ed25519-dalek = { version = "2.1", features = ["rand_core"] }
base64 = "0.22"

# path: iON/crates/iON_schemas/Cargo.toml
[package]
name = "iON_schemas"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = "1.0"
jsonschema = "0.18"
serde_json = "1.0"
regex = "1.10"
