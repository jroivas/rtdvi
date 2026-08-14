//! Build script: surface the resolved versions of the optional plugin
//! runtimes (`wasmtime` / `wasmi`) and `mlua` from `Cargo.lock` so the
//! `:version` command can report them. Each is exposed as a compile-time
//! env var (`RTDVI_<CRATE>_VERSION`) read via `option_env!`.

use std::fs;
use std::process::Command;

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
    if let Some(hash) = git_short_hash() {
        println!("cargo:rustc-env=RTDVI_GIT_HASH={hash}");
    }
}

/// Short git commit hash of the build, with `-dirty` appended when the
/// working tree has uncommitted changes — so `:version` can pin down exactly
/// which build is running. Returns `None` when git isn't available or this
/// isn't a checkout (e.g. building from a packaged tarball), in which case
/// `:version` just omits the hash.
fn git_short_hash() -> Option<String> {
    // Rebuild when HEAD moves (new commit / branch switch) or the index
    // changes (staging). Unstaged edits to tracked files won't retrigger the
    // build script, so `-dirty` can lag until the next rebuild — acceptable.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");

    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let hash = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if hash.is_empty() {
        return None;
    }
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    Some(if dirty { format!("{hash}-dirty") } else { hash })
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
