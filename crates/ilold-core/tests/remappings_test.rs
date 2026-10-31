use std::path::PathBuf;

use ilold_core::parse::solc_frontend::SolcFrontend;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures")
        .join(name)
}

/// The nested `dep/Dep.sol` import only compiles through its context-scoped remapping.
#[test]
fn nested_library_remappings_compile() {
    let project = SolcFrontend
        .parse_project(&fixture_path("nested_remappings"))
        .expect("nested remappings fixture compiles");

    assert!(project.contracts.iter().any(|c| c.name == "App"));
    for source in [
        "lib/outer/lib/inner/contracts/Scoped.sol",
        "lib/outer/lib/inner/lib/dep/src/Dep.sol",
        "lib/outer/lib/dep/src/Dep.sol",
    ] {
        assert!(
            project.source_files.iter().any(|f| f.path.ends_with(source)),
            "missing {source}"
        );
    }
}
