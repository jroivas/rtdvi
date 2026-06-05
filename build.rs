//! Build script: surface the resolved versions of the optional plugin
//! runtimes (`wasmtime` / `wasmi`) and `mlua` from `Cargo.lock` so the
//! `:version` command can report them. Each is exposed as a compile-time
//! env var (`RTDVI_<CRATE>_VERSION`) read via `option_env!`.

use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=Cargo.lock");
    let lock = fs::read_to_string("Cargo.lock").unwrap_or_default();
    for crate_name in ["wasmtime", "wasmi", "mlua"] {
        if let Some(v) = lock_version(&lock, crate_name) {
            println!(
                "cargo:rustc-env=RTDVI_{}_VERSION={v}",
                crate_name.to_uppercase()
            );
        }
    }
}

/// Return the `version = "…"` value from the `[[package]]` block whose
/// `name = "<crate_name>"`. Hand-parses the simple Cargo.lock format to
/// avoid pulling a TOML parser into build dependencies.
fn lock_version(lock: &str, crate_name: &str) -> Option<String> {
    let needle = format!("name = \"{crate_name}\"");
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim() == needle {
            for next in lines.by_ref() {
                let t = next.trim();
                if let Some(rest) = t.strip_prefix("version = \"") {
                    return rest.strip_suffix('"').map(str::to_string);
                }
                if t.starts_with("[[") {
                    break; // ran into the next package without a version
                }
            }
        }
    }
    None
}
