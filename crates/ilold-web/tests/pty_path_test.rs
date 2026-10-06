use std::path::{Path, PathBuf};

use ilold_web::contract_path_for;
use ilold_web::ws::pty::attach_path;

#[test]
fn current_dir_resolves_to_absolute_directory() {
    let path = contract_path_for(Path::new("."));
    assert!(!path.as_os_str().is_empty());
    assert!(path.is_absolute(), "{}", path.display());
    assert!(path.is_dir(), "{}", path.display());
    assert_eq!(path, std::env::current_dir().unwrap().canonicalize().unwrap());
}

#[test]
fn relative_directory_is_kept_not_its_parent() {
    let path = contract_path_for(Path::new("tests"));
    assert!(path.is_absolute(), "{}", path.display());
    assert_eq!(path, Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").canonicalize().unwrap());
}

#[test]
fn file_resolves_to_its_parent_directory() {
    let path = contract_path_for(Path::new("Cargo.toml"));
    assert!(path.is_absolute(), "{}", path.display());
    assert_eq!(path, Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap());
}

#[test]
fn attach_path_is_never_empty() {
    assert_eq!(attach_path(Path::new("")), PathBuf::from("."));
    assert_eq!(attach_path(Path::new("/project")), PathBuf::from("/project"));
}
