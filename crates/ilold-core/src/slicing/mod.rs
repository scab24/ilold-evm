// slicer v1: DEFs come from Assignment statements
pub mod backward;
pub mod extract;
pub mod flatten;
pub mod forward;
pub mod types;

use crate::model::contract::ContractDef;
use crate::model::function::FunctionDef;
use crate::model::project::Project;

pub use types::{SliceDirection, SliceEntry, SliceResult, StatementOrigin, StatementPath};

pub fn build_slice_result(
    project: &Project,
    contract: &ContractDef,
    function: &FunctionDef,
    variable: &str,
    direction: SliceDirection,
) -> SliceResult {
    let flat = flatten::flatten_function(project, contract, function);

    if flat.is_empty() {
        return SliceResult {
            function: function.name.clone(),
            variable: variable.to_string(),
            direction,
            backward: Vec::new(),
            forward: Vec::new(),
        };
    }

    let backward = if matches!(direction, SliceDirection::Backward | SliceDirection::Both) {
        backward::backward_slice(&flat, variable)
    } else {
        Vec::new()
    };

    let forward = if matches!(direction, SliceDirection::Forward | SliceDirection::Both) {
        forward::forward_slice(&flat, variable)
    } else {
        Vec::new()
    };

    SliceResult {
        function: function.name.clone(),
        variable: variable.to_string(),
        direction,
        backward: backward.into_iter().map(|p| entry_for(&flat, p)).collect(),
        forward: forward.into_iter().map(|p| entry_for(&flat, p)).collect(),
    }
}

fn entry_for(flat: &[flatten::FlatStatement<'_>], path: StatementPath) -> SliceEntry {
    let entry = flat.iter().find(|f| f.path == path);
    match entry {
        Some(f) => SliceEntry {
            path,
            span: Some(f.statement.span),
            text: extract::statement_text(f.statement),
            origin: f.origin.clone(),
        },
        None => SliceEntry {
            path,
            span: None,
            text: String::new(),
            origin: StatementOrigin::FunctionBody,
        },
    }
}
