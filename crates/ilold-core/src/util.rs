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

pub(crate) fn push_unique<T: PartialEq>(list: &mut Vec<T>, item: T) {
    if !list.contains(&item) {
        list.push(item);
    }
}

pub fn target_base_name(target: &str) -> &str {
    let base = target.split('[').next().unwrap_or(target);
    base.split('.').next().unwrap_or(base)
}

/// Window over a long list in an API response; `truncated` when items lie past it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Page {
    pub offset: usize,
    pub limit: usize,
    pub total: usize,
    pub truncated: bool,
}

impl Page {
    pub fn new(offset: usize, limit: usize, total: usize) -> Self {
        Self { offset, limit, total, truncated: offset.saturating_add(limit) < total }
    }

    pub fn range(&self) -> std::ops::Range<usize> {
        self.offset.min(self.total)..self.offset.saturating_add(self.limit).min(self.total)
    }
}

/// Items a page holds when a request names no limit, and at most.
pub const DEFAULT_PAGE_LIMIT: usize = 100;
pub const MAX_PAGE_LIMIT: usize = 1000;
