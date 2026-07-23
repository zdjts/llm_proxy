fn main() {
    let runbook = std::fs::read_to_string("RUNBOOK.md").unwrap_or_default();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    std::fs::write(
        format!("{out_dir}/runbook.rs"),
        format!("pub const RUNBOOK: &str = r###\"{runbook}\"###;"),
    )
    .unwrap();
    println!("cargo:rerun-if-changed=RUNBOOK.md");
}
