use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use ilold_core::analysis::AnalysisParams;
use ilold_core::export::{export_graph, Build, Status};

/// The exit code: 0 complete, 3 complete with warnings, 4 no facts; errors exit 1 without writing `out`.
pub fn run(path: &Path, out: &Path) -> Result<i32> {
    let root = crate::foundry_root(path)?;
    let build = Build {
        git_commit: option_env!("ILOLD_GIT_COMMIT").map(str::to_string),
        dirty: option_env!("ILOLD_GIT_DIRTY").map(|dirty| dirty == "true"),
    };
    let export = export_graph(&root, &AnalysisParams::new(crate::SHARED_SEQ_DEPTH), &build).map_err(anyhow::Error::msg)?;
    write_atomically(out, export.jsonl.as_bytes())?;
    Ok(match export.status {
        Status::Complete => 0,
        Status::CompleteWithWarnings => 3,
        Status::NoFacts => 4,
    })
}

// a reader never sees a partial file: it is renamed into place only once complete
fn write_atomically(out: &Path, bytes: &[u8]) -> Result<()> {
    let name = out.file_name().context("the output path names no file")?.to_string_lossy();
    let temp = out.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let written = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, out)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written.with_context(|| format!("write {}", out.display()))
}
