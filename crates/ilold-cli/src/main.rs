use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Parser;

mod analyze;
mod colors;
mod context;
mod deps_view;
mod explore;
mod fmt;
mod interactive;

#[derive(Parser)]
#[command(
    name = "ilold",
    version,
    about = "Smart contract execution path analyzer",
    after_help = "Exit codes:\n  0  success\n  1  error: unknown --contract (with close matches), not a Foundry project, compile or server failure\n  2  invalid command-line usage"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    Analyze {
        path: PathBuf,
        #[arg(long)]
        contract: Option<String>,
        #[arg(long, default_value = "3", value_parser = seq_depth)]
        max_seq_depth: usize,
        #[arg(long)]
        verbose: bool,
        #[arg(long)]
        deps: bool,
        /// Also list dependency contracts (libraries, vendored code) loaded for resolution
        #[arg(long)]
        include_dependencies: bool,
    },
    Context {
        path: PathBuf,
        #[arg(long)]
        contract: Option<String>,
        #[arg(long)]
        function: Option<String>,
        #[arg(long)]
        sequence: Option<String>,
        #[arg(long)]
        list: bool,
    },
    Serve {
        path: PathBuf,
        #[arg(long, default_value = "8080")]
        port: u16,
        /// Address to listen on; 0.0.0.0 exposes the server (and its terminal) to the network
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        #[arg(long, default_value = "3", value_parser = seq_depth)]
        max_seq_depth: usize,
    },
    Explore {
        path: PathBuf,
        #[arg(long, default_value = "0")]
        port: u16,
        /// Address to listen on; 0.0.0.0 exposes the server (and its terminal) to the network
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        #[arg(long, default_value = "3", value_parser = seq_depth)]
        max_seq_depth: usize,
        /// Attach to an existing ilold server URL instead of starting one
        #[arg(long)]
        attach: Option<String>,
        /// Contract to start on; defaults to the most derived deployable contract that is not a test, script or mock
        #[arg(long)]
        contract: Option<String>,
    },
    EvmMcp {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        server_url: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Analyze { path, contract, max_seq_depth, verbose, deps, include_dependencies } => {
            if deps {
                analyze::run_deps(&path, contract.as_deref(), include_dependencies)
            } else {
                analyze::run(&path, contract.as_deref(), max_seq_depth, verbose, include_dependencies)
            }
        }
        Commands::Context { path, contract, function, sequence, list } => {
            context::run(&path, contract.as_deref(), function.as_deref(), sequence.as_deref(), list)
        }
        Commands::Serve { path, port, host, max_seq_depth } => {
            let root = foundry_root(&path)?;
            ilold_web::serve(vec![root], SocketAddr::new(host, port), max_seq_depth).await
        }
        Commands::Explore { path, port, host, max_seq_depth, attach, contract } => {
            let addr = SocketAddr::new(host, port);
            if attach.is_some() {
                explore::run(Vec::new(), addr, max_seq_depth, attach, contract).await
            } else {
                let root = foundry_root(&path)?;
                explore::run(vec![root], addr, max_seq_depth, attach, contract).await
            }
        }
        Commands::EvmMcp { server_url } => {
            ilold_evm_mcp::run(ilold_evm_mcp::Config { server_url }).await
        }
    }
}

fn seq_depth(raw: &str) -> Result<usize, String> {
    use ilold_web::api::contract::{valid_sequence_depth, MAX_SEQUENCE_DEPTH};
    match raw.parse::<usize>() {
        Ok(depth) if valid_sequence_depth(depth) => Ok(depth),
        _ => Err(format!("--max-seq-depth must be between 1 and {MAX_SEQUENCE_DEPTH}")),
    }
}

/// Canonical, so `.` and other relative inputs never yield an empty path.
pub fn foundry_root(path: &Path) -> Result<PathBuf> {
    let Ok(start) = path.canonicalize() else {
        anyhow::bail!("Path does not exist: {}", path.display());
    };
    let mut dir = if start.is_dir() {
        start
    } else if start.is_file() {
        start.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        anyhow::bail!("Path does not exist: {}", path.display());
    };

    loop {
        if dir.join("foundry.toml").is_file() {
            return Ok(dir);
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => break,
        }
    }

    anyhow::bail!(
        "Not a Foundry project: no foundry.toml found from {}. The Solidity backend requires a compilable Foundry project.",
        path.display()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foundry_root_is_canonical_for_relative_inputs() {
        let expected = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/staking").canonicalize().unwrap();
        for input in ["../../tests/fixtures/staking", "../../tests/fixtures/staking/src/staking.sol"] {
            let root = foundry_root(Path::new(input)).unwrap();
            assert!(root.is_absolute(), "{input}: {}", root.display());
            assert_eq!(root, expected, "{input}");
        }
        assert!(foundry_root(Path::new("../../tests/fixtures/no-such-dir")).is_err());
    }

    #[test]
    fn serve_and_explore_default_to_loopback() {
        for command in ["serve", "explore"] {
            let host = |extra: &[&str]| match Cli::try_parse_from([&["ilold", command, "."], extra].concat()).unwrap().command {
                Commands::Serve { host, .. } | Commands::Explore { host, .. } => host,
                _ => unreachable!(),
            };
            assert_eq!(host(&[]), IpAddr::from([127, 0, 0, 1]), "{command}");
            assert_eq!(host(&["--host", "0.0.0.0"]), IpAddr::from([0, 0, 0, 0]), "{command}");
            assert!(Cli::try_parse_from(["ilold", command, ".", "--host", "nope"]).is_err(), "{command}");
        }
    }
}
