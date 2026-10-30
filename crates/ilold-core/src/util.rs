// `Type(expr)` is a cast; a bare name is a call
pub fn is_type_cast(name: &str) -> bool {
    let name = name.trim();
    if let Some((head, _)) = name.split_once('(') {
        return is_elementary_type(head) || is_user_type_name(head);
    }
    is_elementary_type(name)
}

fn is_elementary_type(name: &str) -> bool {
    matches!(name, "address" | "bool" | "string")
        || is_sized_type(name, "uint")
        || is_sized_type(name, "int")
        || is_sized_type(name, "bytes")
}

fn is_sized_type(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.bytes().all(|b| b.is_ascii_digit()))
}

fn is_user_type_name(head: &str) -> bool {
    head.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && head.chars().all(|c| c.is_alphanumeric() || c == '_')
}

// Global functions that run no contract code (fallback when solc types are absent).
pub fn is_builtin_function(name: &str) -> bool {
    matches!(
        name,
        "keccak256" | "sha256" | "ripemd160" | "ecrecover" | "addmod" | "mulmod" | "gasleft"
            | "blockhash" | "blobhash" | "selfdestruct" | "require" | "assert" | "revert" | "type"
    )
}

// `abi.encode(...)`, `bytes.concat(...)`, `string.concat(...)`, `block.*`, `msg.*`, `tx.*`
pub fn is_builtin_namespace(name: &str) -> bool {
    matches!(name, "abi" | "bytes" | "string" | "block" | "msg" | "tx")
}

pub fn target_base_name(target: &str) -> &str {
    let base = target.split('[').next().unwrap_or(target);
    base.split('.').next().unwrap_or(base)
}
