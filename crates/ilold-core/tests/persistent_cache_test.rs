// Requires ILOLD_CACHE_TEST_SOLC_VERSION set to an installed SVM compiler; each parse runs in its own process.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use ilold_core::parse::solc_frontend::SolcFrontend;
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

fn fixture(version: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!("ilold-process-cache-{}-{}", std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("src")).unwrap();
    let cache = root.join("private-cache");
    fs::create_dir(&cache).unwrap();
    fs::write(root.join("foundry.toml"), format!("[profile.default]\nsolc = '{version}'\nauto_detect_remappings = false\n")).unwrap();
    fs::write(root.join("src/Main.sol"), "pragma solidity ^0.8.0; import './Helper.sol'; contract Main is Helper {}\n").unwrap();
    fs::write(root.join("src/Helper.sol"), "pragma solidity ^0.8.0; contract Helper { function ping() public pure returns (uint) { return 1; } }\n").unwrap();
    (root, cache)
}

fn child(root: &Path, cache: &Path, profile: &str) -> (bool, String) {
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("--exact").arg("cache_child").arg("--nocapture")
        .env("ILOLD_CACHE_TEST_CHILD", root).env("ILOLD_PROJECT_CACHE_DIR", cache)
        .env("FOUNDRY_PROFILE", profile).output().unwrap();
    (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())
}

#[test]
fn cache_child() {
    let Some(root) = std::env::var_os("ILOLD_CACHE_TEST_CHILD") else { return };
    let cache = std::env::var_os("ILOLD_PROJECT_CACHE_DIR")
        .expect("cache child requires an explicit isolated ILOLD_PROJECT_CACHE_DIR");
    assert!(!cache.is_empty(), "cache child requires a nonempty cache directory");
    let parsed = SolcFrontend.parse_project(Path::new(&root));
    assert!(parsed.is_ok(), "{parsed:?}");
    let project = parsed.unwrap();
    assert!(project.contract_index.contains_key("Main"));
    assert_eq!(project.find_contract(Some("Main")).unwrap().name, "Main");
}

#[test]
#[ignore = "requires ILOLD_CACHE_TEST_SOLC_VERSION set to an already installed SVM compiler"]
fn separate_process_hit_and_input_invalidation() {
    let version = std::env::var("ILOLD_CACHE_TEST_SOLC_VERSION")
        .expect("set ILOLD_CACHE_TEST_SOLC_VERSION to an already installed SVM compiler");
    let (root, cache) = fixture(&version);
    let (ok, cold) = child(&root, &cache, "default");
    assert!(ok && cold.contains("ilold project cache miss"), "{cold}");
    let (ok, warm) = child(&root, &cache, "default");
    assert!(ok && warm.contains("ilold project cache hit"), "{warm}");

    fs::write(root.join("src/Helper.sol"), "pragma solidity ^0.8.0; contract Helper { function ping() public pure returns (uint) { return 2; } }\n").unwrap();
    let (ok, changed) = child(&root, &cache, "default");
    assert!(ok && changed.contains("ilold project cache miss"), "{changed}");
    fs::write(root.join("src/New.sol"), "pragma solidity ^0.8.0; contract New {}\n").unwrap();
    let (ok, added) = child(&root, &cache, "default");
    assert!(ok && added.contains("ilold project cache miss"), "{added}");
    let external = root.join("vendor");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("External.sol"), "pragma solidity ^0.8.0; contract External {}\n").unwrap();
    fs::write(root.join("remappings.txt"), "ext/=vendor/\n").unwrap();
    fs::write(root.join("src/Main.sol"), "pragma solidity ^0.8.0; import './Helper.sol'; import 'ext/External.sol'; contract Main is Helper {}\n").unwrap();
    let (ok, remapped) = child(&root, &cache, "default");
    assert!(ok && remapped.contains("ilold project cache miss"), "{remapped}");
    let (ok, external_warm) = child(&root, &cache, "default");
    assert!(ok && external_warm.contains("ilold project cache hit"), "{external_warm}");
    fs::write(external.join("External.sol"), "pragma solidity ^0.8.0; contract External { uint public value; }\n").unwrap();
    let (ok, external_changed) = child(&root, &cache, "default");
    assert!(ok && external_changed.contains("ilold project cache miss"), "{external_changed}");
    let (ok, profile) = child(&root, &cache, "other");
    assert!(ok && profile.contains("ilold project cache miss"), "{profile}");
    fs::write(root.join("foundry.toml"), format!("[profile.default]\nsolc = '{version}'\nauto_detect_remappings = false\noptimizer = true\n")).unwrap();
    let (ok, config) = child(&root, &cache, "default");
    assert!(ok && config.contains("ilold project cache miss"), "{config}");

    // Imports resolved through a nested library's own remappings.txt stay cacheable.
    let outer = root.join("lib/outer");
    for inner in ["inner", "inner2"] {
        fs::create_dir_all(outer.join("lib").join(inner).join("src")).unwrap();
        fs::write(outer.join("lib").join(inner).join("src/Inner.sol"),
            format!("pragma solidity ^0.8.0; contract Inner {{ string public origin = '{inner}'; }}\n")).unwrap();
    }
    fs::write(outer.join("foundry.toml"), "[profile.default]\n").unwrap();
    fs::write(outer.join("remappings.txt"), "@nested/=lib/inner/src/\n").unwrap();
    fs::write(root.join("foundry.toml"), format!("[profile.default]\nsolc = '{version}'\noptimizer = true\n")).unwrap();
    fs::write(root.join("src/Main.sol"), "pragma solidity ^0.8.0; import './Helper.sol'; import '@nested/Inner.sol'; contract Main is Helper {}\n").unwrap();
    let (ok, nested) = child(&root, &cache, "default");
    assert!(ok && nested.contains("ilold project cache miss") && !nested.contains("bypass"), "{nested}");
    let (ok, nested_warm) = child(&root, &cache, "default");
    assert!(ok && nested_warm.contains("ilold project cache hit"), "{nested_warm}");
    fs::write(outer.join("lib/inner/src/Inner.sol"), "pragma solidity ^0.8.0; contract Inner { uint public changed; }\n").unwrap();
    let (ok, nested_source) = child(&root, &cache, "default");
    assert!(ok && nested_source.contains("ilold project cache miss"), "{nested_source}");
    fs::write(outer.join("remappings.txt"), "@nested/=lib/inner2/src/\n").unwrap();
    let (ok, nested_remap) = child(&root, &cache, "default");
    assert!(ok && nested_remap.contains("ilold project cache miss"), "{nested_remap}");

    fs::write(root.join("src/Helper.sol"), "pragma solidity ^0.8.0; contract Helper { function broken( }\n").unwrap();
    let (ok, failure) = child(&root, &cache, "default");
    assert!(!ok && !failure.contains("ilold project cache hit"), "{failure}");
}
