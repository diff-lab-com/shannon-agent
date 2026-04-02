use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Set build metadata
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".to_string());
    let git_hash = get_git_hash();

    println!("cargo:rustc-env=SHANNON_VERSION={}", version);
    println!("cargo:rustc-env=SHANNON_GIT_HASH={}", git_hash);

    // Output build info
    println!("cargo:warning=Building Shannon Code v{}", version);
}

fn get_git_hash() -> String {
    use std::process::Command;

    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "unknown".to_string(),
    }
}
