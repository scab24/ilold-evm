use std::{env, fs, path::{Path, PathBuf}};

fn collect(dir: &Path, root: &Path, entries: &mut Vec<String>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            collect(&path, root, entries)?;
        } else if kind.is_file() {
            let relative = path.strip_prefix(root).unwrap().to_str().expect("UTF-8 frontend path");
            println!("cargo:rerun-if-changed={}", path.display());
            entries.push(format!(
                "({:?}, include_bytes!({:?}).as_slice()),\n",
                relative,
                path.canonicalize()?.to_str().expect("UTF-8 frontend path")
            ));
        }
    }
    Ok(())
}

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("frontend/build");
    println!("cargo:rerun-if-changed={}", root.display());
    if !root.join("index.html").is_file() {
        panic!("Missing Svelte UI assets at {}. Run `npm --prefix crates/ilold-web/frontend ci` then `npm --prefix crates/ilold-web/frontend run build` before cargo build/test; no downloads are performed by build.rs.", root.display());
    }
    let mut entries = Vec::new();
    collect(&root, &root, &mut entries).expect("Cannot read built Svelte UI assets");
    entries.sort();
    let output = format!("pub static ASSETS: &[(&str, &[u8])] = &[\n{}];\n", entries.concat());
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("ui_assets.rs"), output)
        .expect("Cannot write embedded UI manifest");
}
