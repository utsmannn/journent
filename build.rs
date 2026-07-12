// journent build script.
// - Embed migrations into the binary (sqlx::migrate!).
// - Embed templates/static/AGENT_ONBOARDING.md so the binary is self-contained, no runtime files needed.

use std::env;
use std::path::PathBuf;

fn main() {
    // Track AGENT_ONBOARDING.md so we rebuild when it changes.
    println!("cargo:rerun-if-changed=AGENT_ONBOARDING.md");
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=templates");
    println!("cargo:rerun-if-changed=static");

    // Ensure static folder exists so the builder does not fail on first run.
    let dirs = ["static", "templates", "migrations"];
    for d in dirs {
        let p: PathBuf = PathBuf::from(d);
        if !p.exists() {
            std::fs::create_dir_all(&p).ok();
        }
    }
    let _ = env::var("OUT_DIR").unwrap_or_else(|_| "target".into());
}
