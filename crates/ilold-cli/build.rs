use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("--no-optional-locks").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The build's commit and whether the build inputs differ from it; nothing outside a git checkout.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let Some(commit) = git(&["rev-parse", "--verify", "--quiet", "HEAD"]) else { return };
    let mut watched = vec!["HEAD".to_string(), "index".to_string(), "packed-refs".to_string()];
    watched.extend(git(&["symbolic-ref", "--quiet", "HEAD"]));
    for name in watched {
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", &name]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let Some(top) = git(&["rev-parse", "--show-toplevel"]) else { return };
    let inputs = ["crates", "Cargo.toml", "Cargo.lock"];
    // edits to tracked sources leave the index alone, so the sources themselves are watched
    for input in inputs.iter().map(|i| Path::new(&top).join(i)).filter(|p| p.exists()) {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    let mut status = vec!["-C", &top, "status", "--porcelain", "--untracked-files=all", "--"];
    status.extend(inputs);
    let Some(changes) = git(&status) else { return };
    println!("cargo:rustc-env=ILOLD_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=ILOLD_GIT_DIRTY={}", !changes.is_empty());
}
