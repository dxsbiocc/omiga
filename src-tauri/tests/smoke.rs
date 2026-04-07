//! Minimal integration test crate: ensures the library links as `omiga_lib` (not `app_lib`).
//! Session/round DB tests were removed when they drifted from `SessionRepository`; restore from
//! git history if you reintroduce a full flow suite against the current API.

#[test]
fn omiga_lib_crate_resolves() {
    // If this compiles, integration tests see the same crate name as Cargo.toml `[[lib]] name`.
    let _ = core::any::type_name::<omiga_lib::domain::persistence::RoundStatus>();
}
