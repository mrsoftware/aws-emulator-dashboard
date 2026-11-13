// Dev-only harness: nothing in the UI reads the matrix, `cargo test -- --ignored` prints it.
#[cfg(test)]
pub mod capability;
pub mod client;
pub mod drivers;
pub mod manage;
pub mod services;
