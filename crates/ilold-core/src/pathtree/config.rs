#[derive(Debug, Clone)]
pub struct PruningConfig {
    pub max_depth: usize,
    pub max_loop_unroll: usize,
    pub max_paths: usize,
}

impl Default for PruningConfig {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_loop_unroll: 3,
            max_paths: 10_000,
        }
    }
}
