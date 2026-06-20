// NOTE: The `.expanded.rs` snapshots embed an absolute `#[path = "..."]` that
// must resolve to real files during expansion, so the snapshot necessarily
// contains the absolute project path of whoever generated it (e.g.
// `/home/tristand/code/...`). That makes this test machine-specific: it only
// passes on the environment the snapshot was generated on. It's therefore
// `#[ignore]`d so a normal `cargo test` stays green everywhere; run it
// explicitly when (re)generating or verifying expansion output:
//
//   cargo test --test expand -- --ignored
//
// (delete the `.expanded.rs` files first to regenerate them for your machine).
#[test]
#[ignore = "snapshot embeds a machine-specific absolute path; run with --ignored"]
pub fn expand_snapshot_pass() {
    macrotest::expand_args("tests/expand/*.rs", &["--features", "nightly,debug,macrotest"]);
}
