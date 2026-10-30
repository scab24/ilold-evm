use std::path::PathBuf;
use std::process::Command;

fn cross() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/cross")
}

fn analyze(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ilold"))
        .arg("analyze")
        .arg(cross())
        .args(args)
        .output()
        .expect("run ilold analyze")
}

#[test]
fn unknown_contract_exits_with_error_naming_close_matches() {
    let out = analyze(&["--contract", "vault"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("Contract 'vault' not found"), "{stderr}");
    assert!(stderr.contains("Did you mean: Vault"), "{stderr}");
    assert!(out.stdout.is_empty() || !String::from_utf8_lossy(&out.stdout).contains("contract "), "printed analysis for an unknown contract");
}

#[test]
fn unknown_contract_without_close_match_lists_available() {
    let out = analyze(&["--contract", "Nope"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("Contract 'Nope' not found") && stderr.contains("Available:") && stderr.contains("LendingPool"), "{stderr}");
}

#[test]
fn deps_unknown_contract_suggests_too() {
    let out = analyze(&["--deps", "--contract", "LendingPol"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("Did you mean: LendingPool"), "{stderr}");
}

#[test]
fn known_contract_exits_zero() {
    let out = analyze(&["--contract", "Vault"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Vault"));
}

#[test]
fn ambiguous_contract_name_exits_with_error_listing_qualified_ids() {
    let model = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/model/project");
    let out = Command::new(env!("CARGO_BIN_EXE_ilold"))
        .arg("analyze")
        .arg(model)
        .args(["--contract", "IToken"])
        .output()
        .expect("run ilold analyze");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("ambiguous") && stderr.contains("src/ext/IToken.sol:IToken"), "{stderr}");
}
