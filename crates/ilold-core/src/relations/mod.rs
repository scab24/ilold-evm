//! Facts that relate code across contracts, computed once from the analysis; every surface reads them from here.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Relations {}
