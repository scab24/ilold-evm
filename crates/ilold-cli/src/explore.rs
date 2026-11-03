use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use colored::Colorize;
use reedline::{
    default_emacs_keybindings, ColumnarMenu, Completer, DefaultHinter, Emacs, FileBackedHistory,
    KeyCode, KeyModifiers, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, Signal, Span, Suggestion,
};

use ilold_core::classify::entry_points::AccessLevel;
use ilold_core::exploration::commands::CommandResult;
use ilold_core::model::project::ContractLookupError;
use ilold_core::util::{compact, push_unique};
use ilold_evm_mcp::{check_segments, encode_component as enc};

use crate::colors::*;
use crate::fmt;

pub async fn run(
    paths: Vec<PathBuf>,
    addr: std::net::SocketAddr,
    max_seq_depth: usize,
    attach: Option<String>,
    contract: Option<String>,
) -> Result<()> {
    if let Some(url) = attach {
        let client = reqwest::Client::new();
        let resp = client.get(format!("{url}/api/project"))
            .send().await
            .map_err(|e| anyhow::anyhow!("Cannot reach server at {url}: {e}"))?;
        if !resp.status().is_success() {
            anyhow::bail!("Server at {url} returned {}", resp.status());
        }
        let project_info: serde_json::Value = resp.json().await?;

        let contracts_arr = project_info["contracts"].as_array();
        let contract_name = start_contract(&project_info, contract.as_deref())?;

        let mut names_by_contract = HashMap::<String, ContractNames>::new();
        let contract_names: Vec<String> = contracts_arr
            .map(|arr| arr.iter().filter_map(|c| c["name"].as_str().map(String::from)).collect())
            .unwrap_or_default();
        for cname in &contract_names {
            if let Ok(resp) = client.get(format!("{url}/api/contract/{}", enc(cname))).send().await {
                if let Ok(detail) = resp.json::<serde_json::Value>().await {
                    names_by_contract.insert(cname.clone(), ContractNames::from_detail(&detail));
                }
            }
        }

        let func_count = names_by_contract.get(&contract_name).map_or(0, |n| n.functions.len());

        let banner = fmt::header_box(&[
            &format!("ilold explore — {} (attached)", contract_name),
            &format!("{} functions | Type ? for help", func_count),
            &format!("Server: {}", url),
        ]);
        println!("{}\n", banner);

        let base_url = url;
        let handle = tokio::runtime::Handle::current();
        let repl_thread = std::thread::spawn(move || {
            repl_loop(handle, contract_name, contract_names, names_by_contract, None, base_url);
        });
        tokio::task::spawn_blocking(move || repl_thread.join())
            .await?
            .map_err(|_| anyhow::anyhow!("REPL thread panicked"))?;
        return Ok(());
    }

    for root in &paths {
        println!("Analyzing {}...", root.display());
    }
    let (state, _, server) = ilold_web::start_server_with_handle(paths, addr, max_seq_depth).await?;
    let base_url = ilold_web::local_url(state.addr);

    let contract_name = match contract.as_deref() {
        Some(name) => state.project.find_contract(Some(name))
            .map(|c| c.key().to_string())
            .map_err(|e| anyhow::anyhow!(e))?,
        None => state.project.default_contract()
            .map(|c| c.key().to_string())
            .ok_or_else(|| anyhow::anyhow!("No contracts found"))?,
    };

    let names_by_contract = local_names(&state.project);
    let contract_names: Vec<String> = state.project.listed_contracts()
        .map(|c| c.key().to_string())
        .filter(|n| !n.is_empty())
        .collect();

    let func_count = names_by_contract.get(&contract_name).map_or(0, |n| n.functions.len());

    let banner = fmt::header_box(&[
        &format!("ilold explore — {}", contract_name),
        &format!(
            "{} file(s), {} contract(s) | {} functions in {}",
            state.project.source_files.len(), contract_names.len(), func_count, contract_name,
        ),
        &format!("Web UI: {base_url} | Type ? for help"),
    ]);
    println!("{}\n", banner);

    let mut by_folder: std::collections::BTreeMap<String, Vec<&str>> = std::collections::BTreeMap::new();
    for c in state.project.listed_contracts() {
        by_folder.entry(state.project.contract_folder(c)).or_default().push(c.key());
    }
    println!("{}", "Contracts:".bold());
    for (folder, mut names) in by_folder {
        names.sort();
        let label = if folder.is_empty() { "(root)" } else { folder.as_str() };
        println!("  {} {}", format!("{label}/").dimmed(), names.join(", "));
    }
    println!();

    let handle = tokio::runtime::Handle::current();
    let state_for_thread = state.clone();
    let repl_thread = std::thread::spawn(move || {
        repl_loop(handle, contract_name, contract_names, names_by_contract, Some(state_for_thread), base_url);
    });

    // a current-thread runtime must keep serving API requests while Reedline waits for input
    let repl = tokio::task::spawn_blocking(move || {
        repl_thread.join().map_err(|_| anyhow::anyhow!("REPL thread panicked"))
    });
    tokio::select! {
        result = repl => result??,
        result = server => {
            result??;
            anyhow::bail!("Explore server stopped while the REPL was running");
        }
    }
    Ok(())
}

fn start_contract(project_info: &serde_json::Value, explicit: Option<&str>) -> Result<String> {
    let names: Vec<&str> = project_info["contracts"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|c| c["name"].as_str()).filter(|n| !n.is_empty()).collect())
        .unwrap_or_default();
    match explicit {
        Some(name) if names.contains(&name) => Ok(name.to_string()),
        Some(name) => anyhow::bail!("Contract '{name}' not found. Available: {}", names.join(", ")),
        None => project_info["default_contract"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("No contracts found")),
    }
}

fn open_browser(url: &str) -> Result<()> {
    let status = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).status()?
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).status()?
    } else {
        std::process::Command::new("xdg-open").arg(url).status()?
    };
    if !status.success() { anyhow::bail!("browser launcher exited with {status}"); }
    Ok(())
}

fn repl_loop(
    handle: tokio::runtime::Handle,
    mut contract: String,
    contract_names: Vec<String>,
    names_by_contract: HashMap<String, ContractNames>,
    state: Option<std::sync::Arc<ilold_web::state::AppState>>,
    base_url: String,
) {
    let history_path = dirs::home_dir()
        .map(|h| h.join(".ilold").join("history"))
        .unwrap_or_else(|| PathBuf::from(".ilold_history"));

    if let Some(parent) = history_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let history = Box::new(
        FileBackedHistory::with_file(500, history_path).expect("Failed to create history"),
    );

    let names_for = |name: &str| names_by_contract.get(name).cloned().unwrap_or_default();
    let mut names = names_for(&contract);
    let completer = std::sync::Arc::new(std::sync::Mutex::new(IloldCompleter {
        functions: names.functions.clone(),
        state_vars: names.state_vars.clone(),
        contracts: contract_names.clone(),
        scenarios: vec!["main".to_string()],
    }));

    let mut keybindings = default_emacs_keybindings();
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu(COMPLETION_MENU.to_string()),
            ReedlineEvent::MenuNext,
        ]),
    );
    keybindings.add_binding(KeyModifiers::SHIFT, KeyCode::BackTab, ReedlineEvent::MenuPrevious);

    let mut editor = Reedline::create()
        .with_history(history)
        .with_completer(Box::new(CompleterWrapper(completer.clone())))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(
            ColumnarMenu::default().with_name(COMPLETION_MENU),
        )))
        .with_edit_mode(Box::new(Emacs::new(keybindings)))
        .with_quick_completions(true)
        .with_partial_completions(true)
        .with_hinter(Box::new(DefaultHinter::default().with_style(
            nu_ansi_term::Style::new().fg(nu_ansi_term::Color::DarkGray),
        )));

    let client = reqwest::Client::new();
    let mut steps: Vec<String> = Vec::new();
    let mut scenario_name: String = "main".into();

    let mut prompt = IloldPrompt {
        contract: contract.clone(),
        steps: Vec::new(),
        scenario: scenario_name.clone(),
    };

    loop {
        // other clients share the server session: refresh it before every prompt
        if let Some(snapshot) = sync_session(&handle, &client, &base_url, &contract) {
            steps = snapshot.steps;
            scenario_name = snapshot.active;
            prompt.steps = steps.clone();
            prompt.scenario = scenario_name.clone();
            if let Ok(mut comp) = completer.lock() {
                comp.scenarios = snapshot.scenarios;
            }
        }

        match editor.read_line(&prompt) {
            Ok(Signal::Success(line)) => {
                let line = line.trim();
                if line.is_empty() { continue; }

                let ctx = ReplContext {
                    handle: &handle,
                    client: &client,
                    base_url: &base_url,
                    contract: &contract,
                    names: &names,
                    contract_names: &contract_names,
                    state: &state,
                };
                match handle_input(line, &ctx, &mut steps, &mut scenario_name, &completer) {
                    InputResult::Continue => {}
                    InputResult::Quit => break,
                    InputResult::UpdatePrompt => {
                        prompt.steps = steps.clone();
                        prompt.scenario = scenario_name.clone();
                    }
                    InputResult::SwitchContract(new_name) => {
                        contract = new_name;
                        steps.clear();
                        scenario_name = "main".into();
                        names = names_for(&contract);
                        if let Ok(mut comp) = completer.lock() {
                            comp.functions = names.functions.clone();
                            comp.state_vars = names.state_vars.clone();
                            comp.scenarios = vec!["main".to_string()];
                        }
                        prompt.contract = contract.clone();
                        prompt.steps = Vec::new();
                        prompt.scenario = scenario_name.clone();
                    }
                }
            }
            Ok(Signal::CtrlC) => continue,
            Ok(Signal::CtrlD) => break,
            Err(e) => {
                eprintln!("Error: {e}");
                break;
            }
        }
    }
}

const COMPLETION_MENU: &str = "completion_menu";

#[derive(Debug, Clone, Default, PartialEq)]
struct ContractNames {
    functions: Vec<String>,
    state_vars: Vec<String>,
}

impl ContractNames {
    fn from_detail(detail: &serde_json::Value) -> Self {
        let items = |keys: &[&str]| -> Vec<serde_json::Value> {
            keys.iter().flat_map(|key| detail[*key].as_array().cloned().unwrap_or_default()).collect()
        };
        let functions = items(&["functions", "inherited_functions"])
            .iter()
            .filter_map(|f| {
                let name = f["name"].as_str()?;
                Some((name.to_string(), f["signature"].as_str().unwrap_or(name).to_string()))
            })
            .collect::<Vec<_>>();
        let mut state_vars: Vec<String> = Vec::new();
        for v in items(&["state_vars", "inherited_state_vars"]) {
            if let Some(name) = v["name"].as_str().filter(|n| !n.is_empty()) {
                push_unique(&mut state_vars, name.to_string());
            }
        }
        ContractNames { functions: function_names(&functions), state_vars }
    }
}

/// Overloads of a shared name are listed by signature without spaces.
fn function_names(functions: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, signature) in functions.iter().filter(|(n, _)| !n.is_empty()) {
        let overloaded = functions.iter().any(|(n, sig)| n == name && sig != signature);
        let entry = if overloaded { compact(signature) } else { name.clone() };
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    out
}

fn local_names(project: &ilold_core::model::project::Project) -> HashMap<String, ContractNames> {
    let mut out = HashMap::new();
    for c in project.contracts.iter().filter(|c| !c.name.is_empty()) {
        let functions: Vec<(String, String)> = project.accessible_functions(c).iter()
            .map(|af| (af.function.name.clone(), af.function.match_signature().to_string()))
            .collect();
        let functions = function_names(&functions);
        let mut state_vars: Vec<String> = Vec::new();
        for v in project.inherited_state_vars(c) {
            push_unique(&mut state_vars, v.name);
        }
        out.insert(c.key().to_string(), ContractNames { functions, state_vars });
    }
    out
}

struct ReplContext<'a> {
    handle: &'a tokio::runtime::Handle,
    client: &'a reqwest::Client,
    base_url: &'a str,
    contract: &'a str,
    names: &'a ContractNames,
    contract_names: &'a [String],
    state: &'a Option<std::sync::Arc<ilold_web::state::AppState>>,
}

enum InputResult {
    Continue,
    Quit,
    UpdatePrompt,
    SwitchContract(String),
}

struct CompleterWrapper(std::sync::Arc<std::sync::Mutex<IloldCompleter>>);

impl Completer for CompleterWrapper {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        self.0.lock().map(|mut c| c.complete(line, pos)).unwrap_or_default()
    }
}

fn handle_input(
    line: &str,
    ctx: &ReplContext,
    steps: &mut Vec<String>,
    scenario_name: &mut String,
    completer: &std::sync::Arc<std::sync::Mutex<IloldCompleter>>,
) -> InputResult {
    let ReplContext { handle, client, base_url, contract, names, contract_names, state } = *ctx;
    let normalized = split_numeric_suffix(line);
    let parts: Vec<&str> = normalized.splitn(2, ' ').collect();
    let cmd = parts[0].to_lowercase();
    let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");

    if cmd.ends_with('?') && cmd.len() > 1 {
        let base = &cmd[..cmd.len() - 1];
        print_inline_help(base);
        return InputResult::Continue;
    }

    match cmd.as_str() {
        "?" | "h" | "help" => { print_help(); InputResult::Continue }
        "q" | "quit" | "exit" => InputResult::Quit,
        "browser" => {
            let available = handle.block_on(async {
                client.get(base_url).send().await
            });
            match available {
                Ok(response) if response.status().is_success()
                    && response.headers().get(reqwest::header::CONTENT_TYPE)
                        .is_some_and(|v| v.to_str().unwrap_or("").starts_with("text/html")) => {
                    if let Err(error) = open_browser(base_url) {
                        eprintln!("  Cannot open browser: {error}. Open {base_url} manually.");
                    }
                }
                Ok(response) => eprintln!("  Web UI unavailable at {base_url}: {}", response.status()),
                Err(error) => eprintln!("  Web UI unavailable at {base_url}: {error}"),
            }
            InputResult::Continue
        }
        "sc" | "scen" | "scenario" => {
            let sub_parts: Vec<&str> = arg.splitn(2, ' ').collect();
            let sub = sub_parts.first().copied().unwrap_or("").trim();
            let name_arg = sub_parts.get(1).map(|s| s.trim()).unwrap_or("");

            use ilold_core::exploration::commands::ScenarioAction;

            let parse_fork = |raw: &str| -> Result<ScenarioAction, String> {
                let parts: Vec<&str> = raw.split_whitespace().collect();
                match parts.as_slice() {
                    [name] => Ok(ScenarioAction::Fork {
                        name: name.to_string(),
                        at_step: None,
                    }),
                    [name, "at", n_str] => n_str
                        .parse::<usize>()
                        .map(|n| ScenarioAction::Fork {
                            name: name.to_string(),
                            at_step: Some(n),
                        })
                        .map_err(|_| format!(
                            "Invalid at-step: '{n_str}' is not a non-negative integer"
                        )),
                    _ => Err("Usage: scenario fork <name> [at <N>]".to_string()),
                }
            };

            let action: Option<ScenarioAction> = match sub {
                "new" if !name_arg.is_empty() => Some(ScenarioAction::New { name: name_arg.to_string() }),
                "list" | "ls" => Some(ScenarioAction::List),
                "switch" if !name_arg.is_empty() => Some(ScenarioAction::Switch { name: name_arg.to_string() }),
                "fork" if !name_arg.is_empty() => match parse_fork(name_arg) {
                    Ok(a) => Some(a),
                    Err(msg) => {
                        eprintln!("  {}", c_danger(&msg));
                        return InputResult::Continue;
                    }
                },
                "delete" | "rm" if !name_arg.is_empty() => Some(ScenarioAction::Delete { name: name_arg.to_string() }),
                _ => None,
            };

            let Some(action) = action else {
                println!("  Usage: scenario new|list|switch|fork|delete <name>");
                return InputResult::Continue;
            };

            let body = serde_json::json!({
                "contract": contract,
                "command": { "Scenario": { "sub": action } }
            });
            match send_command(handle, client, base_url, &body) {
                Ok(result) => {
                    let mut did_update_scenario = false;
                    match &result {
                        CommandResult::ScenarioCreated { name } => {
                            if let Ok(mut comp) = completer.lock() {
                                if !comp.scenarios.iter().any(|s| s == name) {
                                    comp.scenarios.push(name.clone());
                                }
                            }
                        }
                        CommandResult::ScenarioForked { to, .. } => {
                            if let Ok(mut comp) = completer.lock() {
                                if !comp.scenarios.iter().any(|s| s == to) {
                                    comp.scenarios.push(to.clone());
                                }
                            }
                        }
                        CommandResult::ScenarioSwitched { from, to } if from != to => {
                            *scenario_name = to.clone();
                            did_update_scenario = true;
                        }
                        CommandResult::ScenarioDeleted { name } => {
                            if let Ok(mut comp) = completer.lock() {
                                comp.scenarios.retain(|s| s != name);
                            }
                        }
                        CommandResult::ScenarioList { items } => {
                            if let Ok(mut comp) = completer.lock() {
                                comp.scenarios = items.iter().map(|i| i.name.clone()).collect();
                            }
                        }
                        _ => {}
                    }
                    print_result(&result, steps);
                    if did_update_scenario {
                        return InputResult::UpdatePrompt;
                    }
                }
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }

        "ct" | "contracts" => {
            let all = arg == "all";
            if let Some(ref state) = state {
                println!("{}", render_contracts(&local_rows(&state.project, all), contract, state.project.hidden_contracts(), all));
            } else {
                match send_get(handle, client, &format!("{base_url}/api/project?include_dependencies={all}")) {
                    Ok(info) => {
                        let hidden = info["hidden_contracts"].as_u64().unwrap_or(0) as usize;
                        println!("{}", render_contracts(&contract_rows(&info), contract, hidden, all));
                    }
                    Err(e) => eprintln!("  {}", c_danger(&format!("Failed to fetch contracts: {e}"))),
                }
            }
            InputResult::Continue
        }
        "deps" | "dep" => {
            let target = if arg.is_empty() { contract } else { arg };
            show_deps(handle, client, base_url, state, target, false);
            InputResult::Continue
        }
        "usedby" | "rdeps" => {
            let target = if arg.is_empty() { contract } else { arg };
            show_deps(handle, client, base_url, state, target, true);
            InputResult::Continue
        }
        "use" => {
            if arg.is_empty() {
                println!("  Usage: use <contract>");
                return InputResult::Continue;
            }
            if let Some(extra) = extra_args(arg) {
                eprintln!("  {}", c_danger(&format!("use takes one contract name; unexpected: '{extra}'")));
                return InputResult::Continue;
            }
            if let Some(ref state) = state {
                match state.project.lookup_contract(arg) {
                    Ok(c) => {
                        let name = c.key().to_string();
                        if name == contract {
                            println!("  Already using {}", c_accent(&name));
                            return InputResult::Continue;
                        }
                        println!("  {} Now using: {}", c_ok("✓"), c_accent(&name));
                        if !steps.is_empty() {
                            println!("  {}", c_muted(&format!("{contract} keeps its {} step(s); `use {contract}` returns to them", steps.len())));
                        }
                        InputResult::SwitchContract(name)
                    }
                    Err(ContractLookupError::Ambiguous(ids)) => {
                        eprintln!("  {}", c_danger(&format!(
                            "Contract name '{arg}' is ambiguous; use one of: {}", ids.join(", "))));
                        InputResult::Continue
                    }
                    Err(ContractLookupError::NotFound) => {
                        eprintln!("  {}", c_danger(&crate::analyze::contract_not_found(
                            arg, contract_names.iter().map(String::as_str)).to_string()));
                        InputResult::Continue
                    }
                }
            } else {
                let name = arg.to_string();
                if !contract_names.contains(&name) {
                    eprintln!("  {}", c_danger(&crate::analyze::contract_not_found(
                        &name, contract_names.iter().map(String::as_str)).to_string()));
                    return InputResult::Continue;
                }
                if name == contract {
                    println!("  Already using {}", c_accent(&name));
                    return InputResult::Continue;
                }
                println!("  {} Now using: {}", c_ok("✓"), c_accent(&name));
                if !steps.is_empty() {
                    println!("  {}", c_muted(&format!("{contract} keeps its {} step(s); `use {contract}` returns to them", steps.len())));
                }
                InputResult::SwitchContract(name)
            }
        }

        "c" | "call" => {
            if arg.is_empty() {
                println!("  Usage: call <function>");
                return InputResult::Continue;
            }
            if let Some(extra) = extra_args(arg) {
                eprintln!("  {}", c_danger(&format!(
                    "call takes one function name (no arguments); unexpected: '{extra}'. Usage: call <function>")));
                return InputResult::Continue;
            }
            let body = serde_json::json!({
                "contract": contract,
                "command": { "Call": { "func": arg } }
            });
            match send_command(handle, client, base_url, &body) {
                Ok(result) => {
                    if let CommandResult::StepAdded { function, .. } = &result {
                        steps.push(function.clone());
                    }
                    print_result(&result, steps);
                    if matches!(&result, CommandResult::StepAdded { .. }) {
                        return InputResult::UpdatePrompt;
                    }
                }
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "b" | "back" => {
            let body = serde_json::json!({
                "contract": contract,
                "command": "Back"
            });
            match send_command(handle, client, base_url, &body) {
                Ok(result) => {
                    if matches!(&result, CommandResult::StepRemoved { .. }) {
                        steps.pop();
                    }
                    print_result(&result, steps);
                    if matches!(&result, CommandResult::StepRemoved { .. }) {
                        return InputResult::UpdatePrompt;
                    }
                }
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "cl" | "clear" => {
            if !steps.is_empty() {
                print!("  Clear {} steps? (y/n) ", steps.len());
                if !read_answer().is_some_and(|answer| answer.trim().eq_ignore_ascii_case("y")) {
                    println!("  Cancelled.");
                    return InputResult::Continue;
                }
            }
            let body = serde_json::json!({
                "contract": contract,
                "command": "Clear"
            });
            match send_command(handle, client, base_url, &body) {
                Ok(result) => {
                    print_result(&result, steps);
                    // an Error keeps the local view
                    if matches!(result, CommandResult::Cleared) {
                        steps.clear();
                        return InputResult::UpdatePrompt;
                    }
                }
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "s" | "state" => {
            send_and_print(handle, client, base_url, contract, "State", steps);
            InputResult::Continue
        }
        "f" | "functions" => {
            send_and_print(handle, client, base_url, contract, "Functions", steps);
            InputResult::Continue
        }
        "fa" | "funcs-all" => {
            send_and_print(handle, client, base_url, contract, "FunctionsAll", steps);
            InputResult::Continue
        }
        "va" | "vars-all" => {
            send_and_print(handle, client, base_url, contract, "StateVarsAll", steps);
            InputResult::Continue
        }
        "ss" | "session" => {
            send_and_print(handle, client, base_url, contract, "Session", steps);
            InputResult::Continue
        }
        "n" | "note" => {
            if arg.is_empty() {
                println!("  Usage: note <text>");
                return InputResult::Continue;
            }
            let body = serde_json::json!({
                "contract": contract,
                "command": { "Note": { "text": arg } }
            });
            match send_command(handle, client, base_url, &body) {
                Ok(result) => print_annotation_result(&result, steps),
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "fi" | "finding" => {
            if arg.is_empty() {
                handle_finding_interactive(handle, client, base_url, contract, steps);
            } else {
                let finding_parts: Vec<&str> = arg.splitn(2, ' ').collect();
                if finding_parts.len() < 2 {
                    println!("  Usage: fi <severity> <title>");
                    println!("  Or just: fi (interactive mode)");
                } else {
                    let severity_input = finding_parts[0];
                    let rest = finding_parts[1];
                    match normalize_severity(severity_input) {
                        Some(severity) => {
                            let body = serde_json::json!({
                                "contract": contract,
                                "command": {
                                    "Finding": {
                                        "severity": severity,
                                        "title": rest,
                                        "description": ""
                                    }
                                }
                            });
                            match send_command(handle, client, base_url, &body) {
                                Ok(result) => print_annotation_result(&result, steps),
                                Err(e) => eprintln!("  {}", c_danger(&e)),
                            }
                        }
                        None => {
                            println!("  {}", c_danger("Invalid severity. Valid: critical, high, medium, low, info"));
                        }
                    }
                }
            }
            InputResult::Continue
        }
        "status" => {
            let status_parts: Vec<&str> = arg.splitn(2, ' ').collect();
            if status_parts.len() < 2 {
                println!("  Usage: status <function> <reviewed|suspicious|vulnerable|clean|inprogress|notreviewed>");
                return InputResult::Continue;
            }
            let normalized = match normalize_status(status_parts[1]) {
                Some(s) => s,
                None => {
                    println!("  {}", c_danger("Invalid status. Valid: reviewed, suspicious, vulnerable, clean, inprogress, notreviewed"));
                    return InputResult::Continue;
                }
            };
            let body = serde_json::json!({
                "contract": contract,
                "command": { "Status": { "func": status_parts[0], "status": normalized } }
            });
            match send_command(handle, client, base_url, &body) {
                Ok(result) => print_result(&result, steps),
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "w" | "who" => {
            if arg.is_empty() {
                println!("  Usage: who <variable>");
                return InputResult::Continue;
            }
            if let Err(e) = check_state_var(arg, contract, names) {
                eprintln!("  {}", c_danger(&e));
                return InputResult::Continue;
            }
            let body = serde_json::json!({
                "contract": contract,
                "command": { "Who": { "variable": arg } }
            });
            match send_command(handle, client, base_url, &body) {
                Ok(result) => print_result(&result, steps),
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }

        "v" | "vars" => {
            match send_get(handle, client, &format!("{base_url}/api/contract/{}", enc(contract))) {
                Ok(val) => print_vars(&val),
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }

        "i" | "info" => {
            if arg.is_empty() {
                println!("  Usage: info <function>");
                return InputResult::Continue;
            }
            if let Some(extra) = extra_args(arg) {
                eprintln!("  {}", c_danger(&format!("info takes one function name; unexpected: '{extra}'")));
                return InputResult::Continue;
            }
            match send_get(handle, client, &format!("{base_url}/api/session/function/{}/{}", enc(contract), enc(arg))) {
                Ok(val) => {
                    highlight(handle, client, base_url, serde_json::json!({ "contract": contract, "function": arg }));
                    print_narrative(&val);
                }
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "tr" | "trace" => {
            if arg.is_empty() {
                println!("  Usage: {TRACE_USAGE}");
                return InputResult::Continue;
            }
            let parsed = match parse_trace_args(arg) {
                Ok(parsed) => parsed,
                Err(e) => {
                    eprintln!("  {}", c_danger(&e));
                    println!("  Usage: {TRACE_USAGE}");
                    return InputResult::Continue;
                }
            };
            let (url, target) = match &parsed.target {
                TraceTarget::Function(func_name) => (
                    function_trace_url(base_url, contract, func_name, &parsed),
                    serde_json::json!({ "contract": contract, "function": func_name }),
                ),
                TraceTarget::SessionStep(idx) => {
                    if let Err(e) = check_step_index(*idx, steps) {
                        eprintln!("  {}", c_danger(&e));
                        return InputResult::Continue;
                    }
                    if parsed.has_render_flags() {
                        // the persisted trace has a fixed shape, so flags rebuild it from the function
                        let func_name = &steps[*idx];
                        println!("  {}", c_muted(&format!("step {idx} ({func_name}) rebuilt with the given flags")));
                        (
                            function_trace_url(base_url, contract, func_name, &parsed),
                            serde_json::json!({ "contract": contract, "function": func_name }),
                        )
                    } else {
                        (
                            format!("{base_url}/api/session/step/{idx}/trace?contract={}", enc(contract)),
                            serde_json::json!({ "contract": contract, "step": idx }),
                        )
                    }
                }
            };
            match send_get(handle, client, &url) {
                Ok(val) => match serde_json::from_value::<ilold_core::narrative::trace::FlowTree>(val) {
                    Ok(tree) => {
                        highlight(handle, client, base_url, target);
                        if parsed.interactive {
                            if let Err(e) = crate::interactive::run_trace_viewer(tree) {
                                eprintln!("  {} interactive viewer: {}", c_danger("✗"), e);
                            }
                        } else {
                            print!("{}", fmt::render_flow_tree(&tree));
                        }
                    }
                    Err(e) => eprintln!("  {} Parse FlowTree: {}", c_danger("✗"), e),
                },
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "seq" | "sequence" => {
            match send_get(handle, client, &format!("{base_url}/api/session/sequence?contract={}", enc(contract))) {
                Ok(val) => print_sequence_narrative(&val),
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "tl" | "timeline" => {
            if arg.is_empty() {
                println!("  Usage: timeline <variable>");
                return InputResult::Continue;
            }
            if let Err(e) = check_state_var(arg, contract, names) {
                eprintln!("  {}", c_danger(&e));
                return InputResult::Continue;
            }
            match send_get(handle, client, &format!("{base_url}/api/session/timeline/{}?contract={}", enc(arg), enc(contract))) {
                Ok(val) => match serde_json::from_value::<ilold_core::exploration::timeline::VariableTimeline>(val) {
                    Ok(tl) => print!("{}", fmt::render_variable_timeline(&tl)),
                    Err(e) => eprintln!("  {} Parse VariableTimeline: {}", c_danger("✗"), e),
                },
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "sl" | "slice" => {
            let parts: Vec<&str> = arg.split_whitespace().collect();
            let mut positionals: Vec<&str> = Vec::new();
            let mut direction: Option<&str> = None;
            for part in &parts {
                match *part {
                    "--backward" | "-b" => direction = Some("backward"),
                    "--forward" | "-f" => direction = Some("forward"),
                    "--both" => direction = Some("both"),
                    _ => positionals.push(part),
                }
            }
            if let Some(flag) = positionals.iter().find(|p| p.starts_with('-')) {
                eprintln!("  {}", c_danger(&format!("Unknown flag '{flag}'. Valid: --backward/-b, --forward/-f, --both")));
                return InputResult::Continue;
            }
            if positionals.len() != 2 {
                println!("  Usage: slice <function> <variable> [--backward|--forward|--both]");
                return InputResult::Continue;
            }
            let func_name = positionals[0];
            let var_name = positionals[1];
            if let Err(e) = check_function(func_name, contract, names)
                .and_then(|_| check_state_var(var_name, contract, names))
            {
                eprintln!("  {}", c_danger(&e));
                return InputResult::Continue;
            }
            let mut url = format!(
                "{base_url}/api/session/slice/{}/{}?contract={}", enc(func_name), enc(var_name), enc(contract),
            );
            if let Some(d) = direction {
                url.push_str(&format!("&direction={d}"));
            }
            match send_get(handle, client, &url) {
                Ok(val) => match serde_json::from_value::<ilold_core::slicing::SliceResult>(val) {
                    Ok(res) => print!("{}", fmt::render_slice_result(&res)),
                    Err(e) => eprintln!("  {} Parse SliceResult: {}", c_danger("✗"), e),
                },
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "st" | "step" => {
            if arg.is_empty() {
                println!("  Usage: step <index>");
                return InputResult::Continue;
            }
            let idx = match parse_step_index(arg).and_then(|idx| check_step_index(idx, steps).map(|_| idx)) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("  {}", c_danger(&e));
                    return InputResult::Continue;
                }
            };
            match send_get(handle, client, &format!("{base_url}/api/session/step/{idx}/narrative?contract={}", enc(contract))) {
                Ok(val) => {
                    highlight(handle, client, base_url, serde_json::json!({ "contract": contract, "step": idx }));
                    print_narrative(&val);
                }
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }

        "fl" | "findings" => {
            print_findings_list(handle, client, base_url, contract);
            InputResult::Continue
        }
        "ex" | "export" => {
            let body = serde_json::json!({ "contract": contract, "command": "Export" });
            match send_command(handle, client, base_url, &body) {
                Ok(CommandResult::Exported { markdown }) => {
                    let filename = format!("ilold-report-{}.md", contract);
                    match std::fs::write(&filename, &markdown) {
                        Ok(_) => println!("  {} Exported to {}", c_ok("✓"), c_accent(&filename)),
                        Err(e) => eprintln!("  {} Failed to write: {}", c_danger("✗"), e),
                    }
                }
                Ok(other) => print_result(&other, steps),
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }

        "save" => {
            if arg.is_empty() {
                println!("  Usage: save <name>");
                return InputResult::Continue;
            }
            let path = match session_file(arg) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!("  {}", c_danger(&e));
                    return InputResult::Continue;
                }
            };
            let body = serde_json::json!({ "contract": contract, "command": "SaveSession" });
            match send_command(handle, client, base_url, &body) {
                Ok(CommandResult::SessionSaved { json }) => {
                    if let Some(dir) = path.parent() {
                        std::fs::create_dir_all(dir).ok();
                    }
                    if let Err(e) = check_session_path(&path) {
                        eprintln!("  {}", c_danger(&e));
                        return InputResult::Continue;
                    }
                    match std::fs::write(&path, &json) {
                        Ok(_) => println!("  {} Saved to {}", c_ok("✓"), c_accent(&path.display().to_string())),
                        Err(e) => eprintln!("  {} Write failed: {}", c_danger("✗"), e),
                    }
                }
                Ok(other) => print_result(&other, steps),
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "load" => {
            if arg.is_empty() {
                println!("  Usage: load <name>");
                return InputResult::Continue;
            }
            let path = match session_file(arg) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!("  {}", c_danger(&e));
                    return InputResult::Continue;
                }
            };
            if let Err(e) = check_session_path(&path) {
                eprintln!("  {}", c_danger(&e));
                return InputResult::Continue;
            }
            let json = match std::fs::read_to_string(&path) {
                Ok(j) => j,
                Err(e) => {
                    eprintln!("  {} File not found: {} ({})", c_danger("✗"), path.display(), e);
                    return InputResult::Continue;
                }
            };
            let body = serde_json::json!({ "contract": contract, "command": { "LoadSession": { "json": json } } });
            match send_command(handle, client, base_url, &body) {
                Ok(CommandResult::SessionLoaded { contract: loaded, steps: loaded_steps }) => {
                    println!("  {} Session loaded: {} ({} steps)", c_ok("✓"), c_accent(&loaded), loaded_steps.len());
                    if loaded != contract {
                        return InputResult::SwitchContract(loaded);
                    }
                    *steps = loaded_steps;
                    return InputResult::UpdatePrompt;
                }
                Ok(other) => print_result(&other, steps),
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }

        _ => {
            println!("  Unknown command: {}. Type {} for help.", c_danger(cmd.as_str()), c_accent("?"));
            InputResult::Continue
        }
    }
}

#[derive(Debug, PartialEq)]
struct SessionSnapshot {
    active: String,
    steps: Vec<String>,
    scenarios: Vec<String>,
}

/// Read without changing which contract the server considers current.
fn sync_session(
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    base_url: &str,
    contract: &str,
) -> Option<SessionSnapshot> {
    let all = send_get(handle, client, &format!("{base_url}/api/scenarios/all?contract={}", enc(contract))).ok()?;
    let active = all["active"].as_str()?.to_string();
    let scenarios = all["scenarios"].as_array()?;
    let steps = scenarios
        .iter()
        .find(|s| s["name"].as_str() == Some(active.as_str()))?["steps"]
        .as_array()?
        .iter()
        .filter_map(|s| s["function"].as_str().map(String::from))
        .collect();
    let scenarios = scenarios.iter().filter_map(|s| s["name"].as_str().map(String::from)).collect();
    Some(SessionSnapshot { active, steps, scenarios })
}

fn send_command(
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    base_url: &str,
    body: &serde_json::Value,
) -> Result<CommandResult, String> {
    let value = handle.block_on(json_body(client.post(format!("{base_url}/api/cmd")).json(body)))?;
    serde_json::from_value(value).map_err(|e| format!("Parse failed: {e}"))
}

// the answer is already shown; selecting it on open canvases is best effort
fn highlight(handle: &tokio::runtime::Handle, client: &reqwest::Client, base_url: &str, target: serde_json::Value) {
    handle.block_on(json_body(client.post(format!("{base_url}/api/session/highlight")).json(&target))).ok();
}

fn send_get(
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    url: &str,
) -> Result<serde_json::Value, String> {
    check_segments(url)?;
    handle.block_on(json_body(client.get(url)))
}

async fn json_body(request: reqwest::RequestBuilder) -> Result<serde_json::Value, String> {
    let resp = request.send().await.map_err(|e| format!("Request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(if body.is_empty() { format!("Server error: {status}") } else { body });
    }
    resp.json().await.map_err(|e| format!("Parse failed: {e}"))
}

fn send_and_print(
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    base_url: &str,
    contract: &str,
    command: &str,
    steps: &[String],
) {
    let body = serde_json::json!({ "contract": contract, "command": command });
    match send_command(handle, client, base_url, &body) {
        Ok(result) => print_result(&result, steps),
        Err(e) => eprintln!("  {}", c_danger(&e)),
    }
}

fn split_numeric_suffix(line: &str) -> String {
    let prefixes = ["st", "step"];
    let first_space = line.find(' ').unwrap_or(line.len());
    let first_word = &line[..first_space];

    for p in prefixes {
        if first_word.len() > p.len()
            && first_word.to_lowercase().starts_with(p)
            && first_word[p.len()..].chars().all(|c| c.is_ascii_digit())
        {
            let (cmd, num) = first_word.split_at(p.len());
            let rest = &line[first_space..];
            return format!("{} {}{}", cmd, num, rest);
        }
    }
    line.to_string()
}

const TRACE_USAGE: &str = "trace <function> [--depth N] [--reverts] [+N...] [-i] | trace step <N> [-i]";

#[derive(Debug, PartialEq)]
enum TraceTarget {
    Function(String),
    SessionStep(usize),
}

#[derive(Debug, PartialEq)]
struct TraceArgs {
    target: TraceTarget,
    depth: Option<usize>,
    reverts: bool,
    expand: Vec<usize>,
    interactive: bool,
}

impl TraceArgs {
    /// Flags that change what the trace contains, not just how it is shown.
    fn has_render_flags(&self) -> bool {
        self.depth.is_some() || self.reverts || !self.expand.is_empty()
    }
}

fn parse_trace_args(arg: &str) -> Result<TraceArgs, String> {
    let tokens: Vec<&str> = arg.split_whitespace().collect();
    let mut target: Option<TraceTarget> = None;
    let mut depth: Option<usize> = None;
    let mut reverts = false;
    let mut expand: Vec<usize> = Vec::new();
    let mut interactive = false;
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        match t {
            "--depth" | "-d" => {
                let value = tokens.get(i + 1).ok_or("--depth needs a value: --depth N")?;
                depth = Some(value.parse::<usize>().map_err(|_| {
                    format!("--depth needs a non-negative integer, got '{value}'")
                })?);
                i += 2;
                continue;
            }
            "--reverts" | "-r" => reverts = true,
            "-i" | "--interactive" => interactive = true,
            _ if t.starts_with('+') => {
                let id = t[1..].parse::<usize>()
                    .map_err(|_| format!("'{t}' is not a node to expand: use +N with a numeric node id"))?;
                expand.push(id);
            }
            _ if t.starts_with('-') => {
                return Err(format!("Unknown flag '{t}'. Valid: --depth N, --reverts, +N, -i"));
            }
            "step" if target.is_none() => {
                let n = tokens.get(i + 1).ok_or("trace step needs a step index: trace step <N>")?;
                target = Some(TraceTarget::SessionStep(parse_step_index(n)?));
                i += 2;
                continue;
            }
            _ if target.is_none() => target = Some(TraceTarget::Function(t.to_string())),
            _ => return Err(format!("Unexpected argument '{t}': trace takes one function")),
        }
        i += 1;
    }
    let target = target.ok_or("trace needs a function or `step <N>`")?;
    Ok(TraceArgs { target, depth, reverts, expand, interactive })
}

fn function_trace_url(base_url: &str, contract: &str, func_name: &str, parsed: &TraceArgs) -> String {
    let mut url = format!("{base_url}/api/session/trace/{}/{}", enc(contract), enc(func_name));
    let mut params: Vec<String> = Vec::new();
    if let Some(d) = parsed.depth.or(if parsed.interactive { Some(4) } else { None }) {
        params.push(format!("depth={d}"));
    }
    if parsed.reverts {
        params.push("reverts=true".into());
    }
    if !parsed.expand.is_empty() {
        let csv = parsed.expand.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",");
        params.push(format!("expand={csv}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    url
}

fn extra_args(arg: &str) -> Option<&str> {
    arg.split_once(char::is_whitespace).map(|(_, rest)| rest.trim()).filter(|r| !r.is_empty())
}

fn parse_step_index(raw: &str) -> Result<usize, String> {
    raw.parse::<usize>()
        .map_err(|_| format!("Step index must be a non-negative integer, got '{raw}'"))
}

fn check_step_index(idx: usize, steps: &[String]) -> Result<(), String> {
    match steps.len() {
        0 => Err("No steps yet: add one with `c <function>`".into()),
        n if idx >= n => Err(format!("Step {idx} does not exist: this scenario has {n} step(s), 0-{}", n - 1)),
        _ => Ok(()),
    }
}

fn unknown_name(kind: &str, name: &str, contract: &str, known: &[String]) -> String {
    let candidates: Vec<&str> = known.iter().map(String::as_str).collect();
    let close = crate::analyze::close_matches(name, &candidates);
    if close.is_empty() {
        format!("Unknown {kind} '{name}' in {contract}")
    } else {
        format!("Unknown {kind} '{name}' in {contract}. Did you mean: {}?", close.join(", "))
    }
}

/// An empty name list means the contract's names are unknown, so nothing is rejected.
fn check_state_var(name: &str, contract: &str, names: &ContractNames) -> Result<(), String> {
    if let Some(extra) = extra_args(name) {
        return Err(format!("Expected one state variable; unexpected: '{extra}'"));
    }
    let base = name.split(['[', '.']).next().unwrap_or(name);
    if names.state_vars.is_empty() || names.state_vars.iter().any(|v| v == base) {
        Ok(())
    } else {
        Err(unknown_name("state variable", name, contract, &names.state_vars))
    }
}

/// A bare name also matches its overloads; the server picks the first.
fn check_function(name: &str, contract: &str, names: &ContractNames) -> Result<(), String> {
    let query = compact(name);
    let known = names.functions.iter()
        .any(|f| *f == query || f.split('(').next() == Some(query.as_str()));
    if names.functions.is_empty() || known {
        Ok(())
    } else {
        Err(unknown_name("function", name, contract, &names.functions))
    }
}

fn sessions_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".ilold").join("sessions"))
        .unwrap_or_else(|| PathBuf::from(".ilold/sessions"))
}

fn session_file(name: &str) -> Result<PathBuf, String> {
    Ok(sessions_dir().join(format!("{}.json", session_stem(name)?)))
}

fn session_stem(name: &str) -> Result<&str, String> {
    let stem = name.strip_suffix(".json").unwrap_or(name);
    let valid = !stem.is_empty()
        && stem.len() <= 64
        && !stem.starts_with('.')
        && !stem.ends_with('.')
        && stem.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !valid {
        return Err(format!(
            "Invalid session name '{name}': use letters, digits, '-', '_' or '.' (no paths, no leading or trailing '.'); \
             sessions live in {}",
            sessions_dir().display()
        ));
    }
    // Windows reserves these device names, with any extension
    let device = stem.split('.').next().unwrap_or(stem).to_ascii_uppercase();
    let reserved = matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|p| device.strip_prefix(p).is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9'])));
    if reserved {
        return Err(format!("Invalid session name '{name}': '{device}' is a reserved device name"));
    }
    Ok(stem)
}

/// Saving or loading through a symbolic link would reach outside the sessions directory.
fn check_session_path(path: &std::path::Path) -> Result<(), String> {
    for p in [path.parent(), Some(path)].into_iter().flatten() {
        if std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(format!("Refusing {}: it is a symbolic link", p.display()));
        }
    }
    Ok(())
}

fn normalize_severity(input: &str) -> Option<&'static str> {
    match input.to_lowercase().as_str() {
        "critical" => Some("Critical"),
        "high" => Some("High"),
        "medium" => Some("Medium"),
        "low" => Some("Low"),
        "informational" | "info" => Some("Informational"),
        _ => None,
    }
}

fn normalize_status(input: &str) -> Option<&'static str> {
    match input.to_lowercase().as_str() {
        "reviewed" => Some("Reviewed"),
        "suspicious" => Some("Suspicious"),
        "vulnerable" => Some("Vulnerable"),
        "clean" => Some("Clean"),
        "inprogress" => Some("InProgress"),
        "notreviewed" => Some("NotReviewed"),
        _ => None,
    }
}

fn read_prompt(label: &str) -> Option<String> {
    println!("  {} {}", label, c_muted("(empty to cancel)"));
    print!("  > ");
    let input = read_answer()?;
    let trimmed = input.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

/// Reads through crossterm: keys typed right after the command may already sit in its queue, out of reach of stdin.
fn read_answer() -> Option<String> {
    use std::io::Write;
    let mut out = std::io::stdout();
    out.flush().ok();
    crossterm::terminal::enable_raw_mode().ok()?;
    let answer = collect_answer(std::iter::from_fn(|| Some(crossterm::event::read())), &mut out);
    let _ = crossterm::terminal::disable_raw_mode();
    println!();
    answer
}

/// Enter accepts the line; Esc, Ctrl-C, Ctrl-D and read errors cancel.
fn collect_answer(
    events: impl Iterator<Item = std::io::Result<crossterm::event::Event>>,
    echo: &mut impl std::io::Write,
) -> Option<String> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
    let mut line = String::new();
    for event in events {
        let Event::Key(key) = event.ok()? else { continue };
        if key.kind == KeyEventKind::Release { continue; }
        match key.code {
            KeyCode::Enter => return Some(line),
            KeyCode::Esc => return None,
            KeyCode::Char('c' | 'd') if key.modifiers.contains(KeyModifiers::CONTROL) => return None,
            KeyCode::Char(c) => {
                line.push(c);
                let _ = write!(echo, "{c}");
            }
            KeyCode::Backspace if line.pop().is_some() => {
                let _ = write!(echo, "\x08 \x08");
            }
            _ => {}
        }
        let _ = echo.flush();
    }
    None
}

fn handle_finding_interactive(
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    base_url: &str,
    contract: &str,
    steps: &[String],
) {
    let severity_input = match read_prompt("Severity (critical/high/medium/low/info):") {
        Some(s) => s,
        None => { println!("  {}", c_muted("Cancelled.")); return; }
    };
    let severity = match normalize_severity(&severity_input) {
        Some(s) => s.to_string(),
        None => {
            println!("  {}", c_danger("Invalid severity. Valid: critical, high, medium, low, info"));
            return;
        }
    };

    let title = match read_prompt("Title:") {
        Some(t) => t,
        None => { println!("  {}", c_muted("Cancelled.")); return; }
    };

    let description = read_prompt("Description (optional):").unwrap_or_default();

    let body = serde_json::json!({
        "contract": contract,
        "command": {
            "Finding": {
                "severity": severity,
                "title": title,
                "description": description
            }
        }
    });
    match send_command(handle, client, base_url, &body) {
        Ok(result) => print_annotation_result(&result, steps),
        Err(e) => eprintln!("  {}", c_danger(&e)),
    }
}

fn print_annotation_result(result: &CommandResult, steps: &[String]) {
    let scope = match steps.last() {
        Some(last) => format!("attached to step {} ({last})", steps.len() - 1),
        None => "session-level: no step yet, not attached to a step".to_string(),
    };
    match result {
        CommandResult::NoteAdded => println!("  {} Note added {}", c_ok("✓"), c_muted(&format!("({scope})"))),
        CommandResult::FindingAdded { id } => {
            println!("  {} Finding {} added {}", c_ok("✓"), c_accent(id), c_muted(&format!("({scope})")))
        }
        other => print_result(other, steps),
    }
}

struct ContractRow {
    key: String,
    kind: ilold_core::model::contract::ContractKind,
    functions: usize,
    state_vars: usize,
    inherits: Vec<String>,
    is_dependency: bool,
}

fn local_rows(project: &ilold_core::model::project::Project, include_dependencies: bool) -> Vec<ContractRow> {
    project.contracts.iter()
        .filter(|c| !c.name.is_empty() && (include_dependencies || c.is_listed()))
        .map(|c| ContractRow {
            key: c.key().to_string(),
            kind: c.kind,
            functions: c.functions.iter().filter(|f| !f.name.is_empty()).count(),
            state_vars: c.state_vars.len(),
            inherits: c.inherits.clone(),
            is_dependency: c.is_dependency,
        })
        .collect()
}

fn contract_rows(info: &serde_json::Value) -> Vec<ContractRow> {
    info["contracts"].as_array().into_iter().flatten()
        .filter_map(|c| Some(ContractRow {
            key: c["name"].as_str().filter(|n| !n.is_empty())?.to_string(),
            kind: serde_json::from_value(c["kind"].clone()).ok()?,
            functions: c["functions"].as_u64().unwrap_or(0) as usize,
            state_vars: c["state_vars"].as_u64().unwrap_or(0) as usize,
            inherits: serde_json::from_value(c["inherits"].clone()).unwrap_or_default(),
            is_dependency: c["is_dependency"].as_bool().unwrap_or(false),
        }))
        .collect()
}

fn render_contracts(rows: &[ContractRow], current: &str, hidden: usize, include_dependencies: bool) -> String {
    use ilold_core::model::contract::ContractKind;
    use std::fmt::Write;
    let mut out = String::from("\n");
    let max_name = rows.iter().map(|c| c.key.chars().count()).max().unwrap_or(0);
    for c in rows {
        let badge = match c.kind {
            ContractKind::Contract => c_accent("[C]"),
            ContractKind::Interface => c_muted("[I]"),
            ContractKind::Library => c_muted("[L]"),
            ContractKind::Abstract => c_warn("[A]"),
        };
        let marker = if c.key == current { c_ok(" ← current").to_string() } else { String::new() };
        let details = format!("{} functions, {} state vars", c.functions, c.state_vars);
        let inherits = if c.inherits.is_empty() { String::new() } else { format!(", inherits {}", c.inherits.join(", ")) };
        let dependency = if c.is_dependency { " (dependency)" } else { "" };
        writeln!(
            out, "  {} {}  {}{}{}{}",
            badge, c_accent(&fmt::pad_right(&c.key, max_name)), c_muted(&details), c_muted(&inherits), c_muted(dependency), marker,
        ).ok();
    }
    if !include_dependencies && hidden > 0 {
        writeln!(out, "  {}", c_muted(&format!("{hidden} dependency or free-function contract(s) hidden; `ct all` lists them"))).ok();
    }
    out
}

fn show_deps(
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    base_url: &str,
    state: &Option<std::sync::Arc<ilold_web::state::AppState>>,
    name: &str,
    reverse: bool,
) {
    if let Some(state) = state {
        let name = match state.project.find_contract(Some(name)) {
            Ok(c) => c.key(),
            Err(e) => {
                eprintln!("  {}", c_danger(&e));
                return;
            }
        };
        let rows = crate::deps_view::dep_rows(&state.dep_graph, name, reverse);
        crate::deps_view::print_direction(name, reverse, rows);
        return;
    }

    let url = format!("{base_url}/api/contract/{}/depgraph", enc(name));
    if let Err(e) = check_segments(&url) {
        eprintln!("  {}", c_danger(&e));
        return;
    }
    let result = handle.block_on(async {
        let resp = client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Ok::<Option<serde_json::Value>, reqwest::Error>(None);
        }
        Ok(Some(resp.json::<serde_json::Value>().await?))
    });
    match result {
        Ok(Some(json)) => {
            let rows = crate::deps_view::rows_from_depgraph_json(&json, name, reverse);
            crate::deps_view::print_direction(name, reverse, Some(rows));
        }
        Ok(None) => crate::deps_view::print_direction(name, reverse, None),
        Err(e) => eprintln!("  {}", c_danger(&format!("Failed to fetch depgraph: {e}"))),
    }
}

fn print_findings_list(
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    base_url: &str,
    contract: &str,
) {
    let body = serde_json::json!({ "contract": contract, "command": "SaveSession" });
    match send_command(handle, client, base_url, &body) {
        Ok(CommandResult::SessionSaved { json }) => match active_findings(&json) {
            Some((scenario, findings)) => print!("{}", render_findings(contract, &scenario, &findings)),
            None => eprintln!("  {}", c_danger("Could not read findings from the session")),
        },
        Ok(CommandResult::Error { message }) => eprintln!("  {}", c_danger(&message)),
        Ok(_) => eprintln!("  {}", c_danger("Could not retrieve findings")),
        Err(e) => eprintln!("  {}", c_danger(&e)),
    }
}

fn active_findings(json: &str) -> Option<(String, Vec<ilold_core::journal::types::Finding>)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let active = value["active"].as_str()?.to_string();
    let findings = serde_json::from_value(value["scenarios"][&active]["journal"]["findings"].clone()).ok()?;
    Some((active, findings))
}

fn render_findings(contract: &str, scenario: &str, findings: &[ilold_core::journal::types::Finding]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    if findings.is_empty() {
        writeln!(out, "  No findings recorded in {contract}/{scenario}. Add one with {}.", c_accent("fi <severity> <title>")).ok();
        return out;
    }
    writeln!(out).ok();
    writeln!(out, "  {} finding(s) in {contract}/{scenario}:", findings.len()).ok();
    for f in findings {
        let target = match (&f.affected_sequence, f.affected_function.is_empty()) {
            (Some(seq), _) => seq.join(" → "),
            (None, false) => f.affected_function.clone(),
            (None, true) => "session-level".to_string(),
        };
        writeln!(out, "  {} {} {}  {}", c_accent(&f.id), severity_colored(&f.severity.to_string()), c_bright(&f.title), c_muted(&target)).ok();
    }
    writeln!(out, "  {}", c_muted("→ export writes them to a markdown report")).ok();
    writeln!(out).ok();
    out
}

fn severity_colored(sev: &str) -> String {
    let tag = format!("[{sev}]");
    match sev {
        "Critical" | "High" => c_danger(&tag).to_string(),
        "Medium" => c_warn(&tag).to_string(),
        _ => c_muted(&tag).to_string(),
    }
}


fn print_result(result: &CommandResult, steps: &[String]) {
    match result {
        CommandResult::StepAdded { step_index, function, access, state_changed } => {
            let badge = access_colored(access);
            println!();
            println!("  {} Step {}: {} {} {}", c_ok("+"), step_index, c_bright(function), badge, format_access_detail(access));
            if !state_changed.is_empty() {
                println!("    {}:", c_muted("State writes"));
                for var in state_changed {
                    println!("      {} {}", c_muted("·"), c_warn(var));
                }
            }
            let seq_str = steps.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" → ");
            println!("    {}: {}", c_muted("Sequence"), c_accent(&seq_str));
            println!();
        }
        CommandResult::StepRemoved { remaining } => {
            println!();
            println!("  {} Step removed. {} remaining.", c_warn("-"), remaining);
            if !steps.is_empty() {
                let seq_str = steps.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" → ");
                println!("    {}: {}", c_muted("Sequence"), c_accent(&seq_str));
            }
            println!();
        }
        CommandResult::Cleared => {
            println!("  {}", c_ok("Session cleared."));
        }
        CommandResult::StateView { summary } => {
            if summary.is_empty() {
                println!("  No state changes yet. Use {} to add steps.", c_accent("call <func>"));
                return;
            }
            println!();
            println!("{}", fmt::separator("STATE"));
            for var in summary {
                println!("  {} {}", c_bright(&var.variable), "");
                for change in &var.changes {
                    println!("    {}", c_muted(change));
                }
            }
            println!();
        }
        CommandResult::FunctionList { functions } => {
            println!();
            let max_name = functions.iter()
                .filter(|f| !f.name.is_empty())
                .map(|f| f.name.chars().count())
                .max().unwrap_or(0);
            for entry in functions {
                if entry.name.is_empty() { continue; }
                let badge = access_colored(&entry.access);
                let padded_name = fmt::pad_right(&entry.name, max_name);
                let mut tags: Vec<&str> = Vec::new();
                if entry.writes_state { tags.push("writes state"); }
                if entry.has_external_calls { tags.push("external calls"); }
                if entry.is_read_only { tags.push("view"); }
                let tag_str = if tags.is_empty() {
                    String::new()
                } else {
                    format!("  {}", c_muted(&tags.join(", ")))
                };
                println!("  {} {}{}{}", badge, c_accent(&padded_name), tag_str, guard_str(&entry.guards));
            }
            println!();
        }
        CommandResult::FunctionListAll { functions } => {
            println!();
            let max_name = functions.iter()
                .filter(|f| !f.name.is_empty())
                .map(|f| f.name.chars().count())
                .max().unwrap_or(0);

            let own: Vec<_> = functions.iter().filter(|f| !f.is_inherited).collect();
            let inherited: Vec<_> = functions.iter().filter(|f| f.is_inherited).collect();

            for entry in &own {
                if entry.name.is_empty() { continue; }
                let badge = access_colored(&entry.access);
                let padded_name = fmt::pad_right(&entry.name, max_name);
                let mut tags: Vec<&str> = Vec::new();
                if entry.writes_state { tags.push("writes state"); }
                if entry.has_external_calls { tags.push("external calls"); }
                if entry.is_read_only { tags.push("view"); }
                let tag_str = if tags.is_empty() {
                    String::new()
                } else {
                    format!("  {}", c_muted(&tags.join(", ")))
                };
                println!("  {} {}{}{}", badge, c_accent(&padded_name), tag_str, guard_str(&entry.guards));
            }

            if !inherited.is_empty() {
                println!();
                println!("  {}", c_muted("inherited:"));
                for entry in &inherited {
                    let badge = access_colored(&entry.access);
                    let padded_name = fmt::pad_right(&entry.name, max_name);
                    let origin = format!("from {}", entry.origin);
                    println!("  {} {}  {}{}", badge, c_muted(&padded_name), c_muted(&origin), guard_str(&entry.guards));
                }
            }
            println!();
        }
        CommandResult::StateVarListAll { state_vars } => {
            println!();
            let max_name = state_vars.iter()
                .map(|v| v.name.chars().count())
                .max().unwrap_or(0);
            let max_tag = 9;

            let own: Vec<_> = state_vars.iter().filter(|v| !v.is_inherited).collect();
            let inherited: Vec<_> = state_vars.iter().filter(|v| v.is_inherited).collect();

            let render_tag = |is_const: bool, is_immut: bool| -> String {
                let text = if is_const { "const" }
                    else if is_immut { "immutable" }
                    else { "mutable" };
                let padded = fmt::pad_right(text, max_tag);
                if is_const || is_immut {
                    c_muted(&padded).to_string()
                } else {
                    c_warn(&padded).to_string()
                }
            };

            for entry in &own {
                let tag = render_tag(entry.is_constant, entry.is_immutable);
                let padded_name = fmt::pad_right(&entry.name, max_name);
                println!("  {} {}  {}", tag, c_accent(&padded_name), c_muted(&entry.type_name));
            }

            if !inherited.is_empty() {
                println!();
                println!("  {}", c_muted("inherited:"));
                for entry in &inherited {
                    let tag = render_tag(entry.is_constant, entry.is_immutable);
                    let padded_name = fmt::pad_right(&entry.name, max_name);
                    let origin = format!("from {}", entry.origin);
                    println!("  {} {}  {}  {}",
                        tag,
                        c_muted(&padded_name),
                        c_muted(&entry.type_name),
                        c_muted(&origin),
                    );
                }
            }
            println!();
        }
        CommandResult::FindingAdded { id } => {
            println!("  {} Finding {} added", c_ok("✓"), c_accent(id));
        }
        CommandResult::NoteAdded => {
            println!("  {} Note added", c_ok("✓"));
        }
        CommandResult::StatusUpdated => {
            println!("  {} Status updated", c_ok("✓"));
        }
        CommandResult::SessionView { contract, steps: session_steps, findings_count } => {
            println!();
            println!("  Contract: {}", c_bright(contract));
            println!("  Steps:    {}", if session_steps.is_empty() {
                c_muted("(empty)").to_string()
            } else {
                session_steps.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" → ")
            });
            println!("  Findings: {}", findings_count);
            println!();
        }
        CommandResult::VariableInfo { variable, writers, readers } => {
            println!();
            println!("  {} {}", c_bright("who:"), c_accent(variable));
            if !writers.is_empty() {
                println!("    {}:", c_warn("Writers"));
                for (name, access) in writers {
                    println!("      {} {}", access_colored(access), c_accent(name));
                }
            }
            if !readers.is_empty() {
                println!("    {}:", c_muted("Readers"));
                for (name, access) in readers {
                    println!("      {} {}", access_colored(access), c_muted(name));
                }
            }
            if !writers.is_empty() {
                let slice_hints: Vec<String> = writers.iter()
                    .take(4)
                    .map(|(name, _)| format!("sl {} {}", name, variable))
                    .collect();
                let suffix = if writers.len() > 4 {
                    format!(" (+{})", writers.len() - 4)
                } else {
                    String::new()
                };
                println!("  {}{}", c_muted(&format!("→ {}", slice_hints.join(", "))), c_muted(&suffix));
            }
            println!("  {}", c_muted(&format!("→ tl {}", variable)));
            println!();
        }
        CommandResult::Exported { markdown } => {
            println!("  {} chars exported", markdown.len());
        }
        CommandResult::SessionSaved { json } => {
            println!("  Session serialized ({} bytes)", json.len());
        }
        CommandResult::SessionLoaded { contract, steps: loaded_steps } => {
            println!("  Session loaded: {} ({} steps)", contract, loaded_steps.len());
        }
        CommandResult::Error { message } => {
            println!("  {}", c_danger(message));
        }
        CommandResult::ScenarioList { items } => {
            print!("{}", fmt::render_scenario_list(items));
        }
        CommandResult::ScenarioCreated { name } => {
            println!("{}", fmt::render_scenario_created(name));
        }
        CommandResult::ScenarioSwitched { from, to } => {
            println!("{}", fmt::render_scenario_switched(from, to));
        }
        CommandResult::ScenarioForked { from, to, at_step } => {
            println!("{}", fmt::render_scenario_forked(from, to, *at_step));
        }
        CommandResult::ScenarioDeleted { name } => {
            println!("{}", fmt::render_scenario_deleted(name));
        }
    }
}

fn guard_str(guards: &[ilold_core::classify::guards::GuardFact]) -> String {
    if guards.is_empty() {
        String::new()
    } else {
        format!("  {}", c_warn(&guard_badges(guards)))
    }
}

fn format_access_detail(access: &AccessLevel) -> String {
    match access {
        AccessLevel::Public => "external".truecolor(110, 120, 140).to_string(),
        AccessLevel::Restricted { role } => format!("{}", c_warn(&format!("restricted({role})"))),
        AccessLevel::Internal => "internal".truecolor(110, 120, 140).to_string(),
        AccessLevel::Special { kind } => format!("{}", c_muted(&format!("special({kind})"))),
    }
}

fn print_vars(val: &serde_json::Value) {
    let vars = match val.get("state_vars").and_then(|v| v.as_array()) {
        Some(v) => v,
        None => { println!("  No state variables found."); return; }
    };
    let max_name = vars.iter()
        .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
        .map(|n| n.chars().count())
        .max().unwrap_or(0);
    let max_tag = 9; // "immutable" is the longest
    println!();
    for v in vars {
        let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("?");
        let type_name = v.get("type_name").and_then(|n| n.as_str()).unwrap_or("?");
        let is_const = v.get("is_constant").and_then(|n| n.as_bool()).unwrap_or(false);
        let is_immut = v.get("is_immutable").and_then(|n| n.as_bool()).unwrap_or(false);

        let tag_text = if is_const { "const" }
            else if is_immut { "immutable" }
            else { "mutable" };
        let padded_tag = fmt::pad_right(tag_text, max_tag);
        let tag = if is_const || is_immut {
            c_muted(&padded_tag).to_string()
        } else {
            c_warn(&padded_tag).to_string()
        };

        let padded_name = fmt::pad_right(name, max_name);
        println!("  {} {}  {}", tag, c_accent(&padded_name), c_muted(type_name));
    }
    println!();
}

fn print_narrative(val: &serde_json::Value) {
    println!();
    if let Some(name) = val.get("name").and_then(|v| v.as_str()) {
        let access = match val.get("access").map(|v| serde_json::from_value::<AccessLevel>(v.clone())) {
            Some(Ok(a)) => a.to_string(),
            _ => String::new(),
        };
        let mods = val.get("modifiers").and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|m| m.as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        let mod_str = if mods.is_empty() { String::new() } else { format!(" — {}", c_muted(&mods)) };
        println!("  {} [{}]{}", c_bright(name), c_accent(&access), mod_str);
    }

    #[derive(Default)]
    struct TransitiveGroup {
        writes: Vec<String>,
        reads: Vec<String>,
        external: Vec<String>,
        events: Vec<String>,
    }

    enum Section<'a> {
        Paths { total: u64, happy: u64, revert: u64, note: Option<String> },
        StringList { label: &'a str, label_color: SectionColor, items: Vec<String> },
        Transitive(Vec<(String, TransitiveGroup)>),
        Observations(Vec<String>),
    }
    enum SectionColor { Muted, Danger, Warn, Accent }

    let color = |c: &SectionColor, s: &str| -> String {
        match c {
            SectionColor::Muted => c_muted(s).to_string(),
            SectionColor::Danger => c_danger(s).to_string(),
            SectionColor::Warn => c_warn(s).to_string(),
            SectionColor::Accent => c_accent(s).to_string(),
        }
    };

    let mut sections: Vec<Section> = Vec::new();

    if let Some(total) = val.get("total_paths").and_then(|v| v.as_u64()) {
        let happy = val.get("happy_paths").and_then(|v| v.as_u64()).unwrap_or(0);
        let revert = val.get("revert_paths").and_then(|v| v.as_u64()).unwrap_or(0);
        let note = val.get("path_stats")
            .and_then(|v| serde_json::from_value::<ilold_core::pathtree::types::PathTreeStats>(v.clone()).ok())
            .and_then(|stats| stats.truncation_note());
        sections.push(Section::Paths { total, happy, revert, note });
    }

    let collect_strs = |key: &str| -> Vec<String> {
        val.get(key)
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default()
    };

    let guards: Vec<String> = val.get("guards")
        .and_then(|v| serde_json::from_value::<Vec<ilold_core::classify::guards::GuardFact>>(v.clone()).ok())
        .unwrap_or_default()
        .iter()
        .map(|g| {
            let via = if g.via.is_empty() { String::new() } else { format!(" (via {})", g.via.join(" → ")) };
            let conditional = if g.scope == ilold_core::classify::guards::GuardScope::Conditional { " (some paths only)" } else { "" };
            format!("{}{}: {}{}{conditional}", g.kind.label(), if g.heuristic { "?" } else { "" }, g.condition, via)
        })
        .collect();
    if !guards.is_empty() {
        sections.push(Section::StringList { label: "Guards", label_color: SectionColor::Warn, items: guards });
    }

    let reads = collect_strs("state_reads");
    if !reads.is_empty() {
        sections.push(Section::StringList { label: "State reads", label_color: SectionColor::Muted, items: reads });
    }
    let writes = collect_strs("state_writes");
    if !writes.is_empty() {
        sections.push(Section::StringList { label: "State writes", label_color: SectionColor::Danger, items: writes });
    }
    let internal = collect_strs("internal_calls");
    if !internal.is_empty() {
        sections.push(Section::StringList { label: "Internal calls", label_color: SectionColor::Accent, items: internal });
    }
    let externals = collect_strs("external_calls");
    if !externals.is_empty() {
        sections.push(Section::StringList { label: "External calls", label_color: SectionColor::Warn, items: externals });
    }
    let events = collect_strs("events");
    if !events.is_empty() {
        sections.push(Section::StringList { label: "Events", label_color: SectionColor::Accent, items: events });
    }

    let collect_transitive = |key: &str| -> Vec<(Vec<String>, String)> {
        val.get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| {
                        let via = e.get("via")?.as_array()?
                            .iter().filter_map(|s| s.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>();
                        let item = e.get("item")?.as_str()?.to_string();
                        Some((via, item))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };

    let t_writes = collect_transitive("transitive_state_writes");
    let t_reads = collect_transitive("transitive_state_reads");
    let t_external = collect_transitive("transitive_external_calls");
    let t_events = collect_transitive("transitive_events");

    if !t_writes.is_empty() || !t_reads.is_empty() || !t_external.is_empty() || !t_events.is_empty() {
        use std::collections::BTreeMap;
        let mut groups: BTreeMap<String, TransitiveGroup> = BTreeMap::new();
        let join_chain = |via: &[String]| via.join(" → ");
        for (via, item) in t_writes { groups.entry(join_chain(&via)).or_default().writes.push(item); }
        for (via, item) in t_reads { groups.entry(join_chain(&via)).or_default().reads.push(item); }
        for (via, item) in t_external { groups.entry(join_chain(&via)).or_default().external.push(item); }
        for (via, item) in t_events { groups.entry(join_chain(&via)).or_default().events.push(item); }
        let ordered: Vec<(String, TransitiveGroup)> = groups.into_iter().collect();
        sections.push(Section::Transitive(ordered));
    }

    let obs_items: Vec<String> = val
        .get("observations")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|o| o.get("description").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if !obs_items.is_empty() {
        sections.push(Section::Observations(obs_items));
    }

    let total = sections.len();
    for (i, section) in sections.iter().enumerate() {
        let last = i == total - 1;
        let branch = if last { "└──" } else { "├──" };
        let cont = if last { "   " } else { "│  " };
        match section {
            Section::Paths { total, happy, revert, note } => {
                println!(
                    "  {} {} path(s): {} happy, {} revert",
                    c_muted(branch), total, c_ok(&happy.to_string()), c_danger(&revert.to_string())
                );
                if let Some(note) = note {
                    println!("  {}    {}", c_muted(cont), c_warn(note));
                }
            }
            Section::StringList { label, label_color, items } => {
                println!("  {} {}:", c_muted(branch), color(label_color, label));
                for (j, item) in items.iter().enumerate() {
                    let leaf = if j == items.len() - 1 { "└──" } else { "├──" };
                    println!("  {}    {} {}", c_muted(cont), c_muted(leaf), c_muted(item));
                }
            }
            Section::Transitive(groups) => {
                println!("  {} {}:", c_muted(branch), c_warn("Transitive effects"));
                let gtotal = groups.len();
                for (gi, (chain, g)) in groups.iter().enumerate() {
                    let glast = gi == gtotal - 1;
                    let gbranch = if glast { "└──" } else { "├──" };
                    let gcont = if glast { "   " } else { "│  " };
                    println!("  {}    {} {} {}", c_muted(cont), c_muted(gbranch), c_muted("via"), c_muted(chain));

                    let mut parts: Vec<(&str, &Vec<String>)> = Vec::new();
                    if !g.writes.is_empty() { parts.push(("writes", &g.writes)); }
                    if !g.reads.is_empty() { parts.push(("reads", &g.reads)); }
                    if !g.external.is_empty() { parts.push(("external", &g.external)); }
                    if !g.events.is_empty() { parts.push(("emits", &g.events)); }
                    let ptotal = parts.len();
                    for (pi, (plabel, pitems)) in parts.iter().enumerate() {
                        let plast = pi == ptotal - 1;
                        let pbranch = if plast { "└──" } else { "├──" };
                        println!(
                            "  {}    {}    {} {}: {}",
                            c_muted(cont), c_muted(gcont), c_muted(pbranch),
                            c_muted(plabel), c_muted(&pitems.join(", "))
                        );
                    }
                }
            }
            Section::Observations(items) => {
                println!("  {} {}:", c_muted(branch), c_danger("Observations"));
                for (j, item) in items.iter().enumerate() {
                    let leaf = if j == items.len() - 1 { "└──" } else { "├──" };
                    println!("  {}    {} {}", c_muted(cont), c_muted(leaf), c_danger(item));
                }
            }
        }
    }
    if let Some(name) = val.get("name").and_then(|v| v.as_str()) {
        println!("  {}", c_muted(&format!("→ c {} | tr {}", name, name)));
    }
    println!();
}

fn print_sequence_narrative(val: &serde_json::Value) {
    println!();
    if let Some(steps) = val.get("steps").and_then(|v| v.as_array()) {
        let names: Vec<&str> = steps.iter()
            .filter_map(|s| s.get("function").and_then(|f| f.as_str()))
            .collect();
        if !names.is_empty() {
            println!("  {}", c_bright(&names.join(" → ")));
        }
    }
    if let Some(deps) = val.get("dependencies").and_then(|v| v.as_array()) {
        if !deps.is_empty() {
            println!("  {}:", c_warn("Dependencies"));
            for dep in deps {
                if let Some(desc) = dep.get("description").and_then(|v| v.as_str()) {
                    println!("    • {}", c_muted(desc));
                }
            }
        }
    }
    if let Some(obs) = val.get("observations").and_then(|v| v.as_array()) {
        if !obs.is_empty() {
            println!("  {}:", c_danger("Observations"));
            for o in obs {
                if let Some(desc) = o.get("description").and_then(|v| v.as_str()) {
                    println!("    ! {}", c_danger(desc));
                } else if let Some(desc) = o.as_str() {
                    println!("    ! {}", c_danger(desc));
                }
            }
        }
    }
    if let Some(steps) = val.get("steps").and_then(|v| v.as_array()) {
        let any_summary = steps.iter().any(|s| !s.get("flow_summary").map(|v| v.is_null()).unwrap_or(true));
        if any_summary {
            println!("  {}:", c_warn("Flow summaries"));
            for (i, step) in steps.iter().enumerate() {
                let summary = match step.get("flow_summary") {
                    Some(s) if !s.is_null() => s,
                    _ => continue,
                };
                let func = step.get("function").and_then(|v| v.as_str()).unwrap_or("?");
                let total = summary.get("total_steps").and_then(|v| v.as_u64()).unwrap_or(0);
                let muts = summary.get("mutation_count").and_then(|v| v.as_u64()).unwrap_or(0);
                let ext = summary.get("external_call_count").and_then(|v| v.as_u64()).unwrap_or(0);
                let int_n = summary.get("internal_call_count").and_then(|v| v.as_u64()).unwrap_or(0);
                let dl = summary.get("depth_limited_count").and_then(|v| v.as_u64()).unwrap_or(0);
                println!(
                    "    {} {}  {} ops | {} mutations | {} ext calls | {} internal{}",
                    c_accent(&format!("step {}", i)),
                    c_bright(func),
                    total, muts, ext, int_n,
                    if dl > 0 { format!(" ({} depth-limited)", dl) } else { String::new() },
                );
            }
        }
    }
    println!();
}

// (shortcut, usage, description)
type HelpEntry = (&'static str, &'static str, &'static str);

/// Every entry has inline help (`cmd?`).
const HELP_GROUPS: &[(&str, &[HelpEntry])] = &[
    ("Session", &[
        ("c",      "call <func>",      "Add function to sequence"),
        ("b",      "back",             "Remove last step"),
        ("cl",     "clear",            "Reset sequence (asks first)"),
        ("s",      "state",            "Show accumulated state"),
        ("seq",    "sequence",         "Sequence narrative with dependencies"),
        ("st",     "step <index>",     "Re-inspect a specific step"),
        ("ss",     "session",          "Full session state"),
        ("sc",     "scenario new|list|switch|fork|delete <name>", "Manage scenarios"),
    ]),
    ("Analysis", &[
        ("w",      "who <var>",        "Who reads/writes a variable"),
        ("i",      "info <func>",      "Function detail"),
        ("tr",     "trace <func>",     "Execution flow tree (inlined)"),
        ("",       "trace <func> -i",  "Interactive trace viewer"),
        ("",       "trace step <N>",   "Re-render persisted step trace"),
        ("tl",     "timeline <var>",   "Cross-step variable mutation history"),
        ("sl",     "slice <fn> <var>", "Dataflow slice for a variable"),
    ]),
    ("Contract", &[
        ("f",      "functions",        "List callable functions"),
        ("fa",     "funcs-all",        "List all accessible (incl. inherited)"),
        ("v",      "vars",             "List state variables"),
        ("va",     "vars-all",         "List all accessible (incl. inherited)"),
        ("ct",     "contracts [all]",  "List project contracts (`all`: with dependencies)"),
        ("",       "deps [contract]",  "What this contract depends on"),
        ("",       "usedby [contract]","What depends on it (blast radius)"),
        ("",       "use <contract>",   "Switch active contract (its steps are kept)"),
    ]),
    ("Findings", &[
        ("fi",     "finding [sev] [t]","Record a finding"),
        ("n",      "note <text>",      "Add note to current step"),
        ("",       "status <f> <s>",   "Change review status"),
        ("fl",     "findings",         "List findings (id, severity, title)"),
        ("ex",     "export",           "Export findings as markdown"),
    ]),
    ("Workspace", &[
        ("",       "save <name>",      "Save session to ~/.ilold/sessions"),
        ("",       "load <name>",      "Load session from ~/.ilold/sessions"),
        ("",       "browser",          "Open web UI"),
        ("?",      "help",             "This help"),
        ("q",      "quit/exit",        "Exit (also Ctrl-D)"),
    ]),
];

const INLINE_HELP: &[(&[&str], &str, &str)] = &[
    (&["c", "call"],      "call <func>",                     "Add a public/external function call to the session. Example: c deposit"),
    (&["b", "back"],      "back",                            "Remove last step from the session sequence."),
    (&["cl", "clear"],    "clear",                           "Reset the active scenario (all steps removed); asks for confirmation."),
    (&["s", "state"],     "state",                           "Show accumulated state mutations across all steps."),
    (&["f", "functions"], "functions",                       "List callable functions in the active contract."),
    (&["v", "vars"],      "vars",                            "List state variables of the active contract."),
    (&["w", "who"],       "who <variable>",                  "Show which functions read/write a state variable. Example: who totalStaked"),
    (&["i", "info"],      "info <func>",                     "Function detail: paths, reads, writes, calls. Example: i deposit"),
    (&["tr", "trace"],    TRACE_USAGE,                       "Execution flow tree. -i opens the TUI; flags after `trace step N` rebuild that step's function. Example: tr swap --depth 3"),
    (&["seq", "sequence"],"sequence",                        "Show the narrative of the current call sequence (needs 2+ steps)."),
    (&["tl", "timeline"], "timeline <variable>",             "Cross-step mutation history of a state variable. Example: tl totalStaked"),
    (&["sl", "slice"],    "slice <func> <var> [--backward|--forward|--both]", "Dataflow slice. Example: sl deposit totalStaked --backward"),
    (&["st", "step"],     "step <index>",                    "Re-inspect a session step (0-based). Example: st 0"),
    (&["ss", "session"],  "session",                         "Full session state with all steps."),
    (&["fi", "finding"],  "finding [severity] [title]",      "Record a finding on the current step; with no steps it is session-level. No arguments: interactive."),
    (&["n", "note"],      "note <text>",                     "Attach a note to the current step; with no steps it is session-level."),
    (&["sc", "scen", "scenario"], "scenario new <name> | list | switch <name> | fork <name> [at N] | delete <name>",
                                                             "Manage scenarios of the active contract. Example: sc fork attack at 2"),
    (&["fl", "findings"], "findings",                        "List findings of the active scenario with id, severity and title."),
    (&["ex", "export"],   "export",                          "Export findings, notes and coverage as ilold-report-<contract>.md."),
    (&["fa", "funcs-all"],"funcs-all",                       "List all accessible functions including inherited."),
    (&["va", "vars-all"], "vars-all",                        "List all accessible state variables including inherited."),
    (&["ct", "contracts"],"contracts [all]",                 "List the project contracts; `all` adds dependency contracts (lib/)."),
    (&["deps", "dep"],    "deps [contract]",                 "Contracts the current (or named) contract depends on. Example: deps Vault"),
    (&["usedby", "rdeps"],"usedby [contract]",               "Contracts that depend on it — the blast radius. Example: usedby IPool"),
    (&["use"],            "use <contract>",                  "Switch the active contract; each contract keeps its own steps. Example: use Staking"),
    (&["status"],         "status <func> <status>",          "Change review status (reviewed, suspicious, vulnerable, clean, inprogress, notreviewed). Example: status deposit reviewed"),
    (&["save"],           "save <name>",                     "Save the contract's scenarios to ~/.ilold/sessions/<name>.json. Names are plain (letters, digits, - _ .). Example: save my-audit"),
    (&["load"],           "load <name>",                     "Load ~/.ilold/sessions/<name>.json; switches to the saved contract. Example: load my-audit"),
    (&["browser"],        "browser",                         "Open the web UI in a browser."),
    (&["?", "h", "help"], "help",                            "List commands. Append ? to any command for its inline help (e.g. sl?)."),
    (&["q", "quit", "exit"], "quit",                         "Exit the REPL (also Ctrl-D). Ctrl-C only clears the current line."),
];

fn print_help() {
    println!();
    println!("  {}  {}", c_bright("ilold explore"), c_muted("— append ? to any command for inline help (e.g. sl?)"));
    println!("  {}", c_muted("Tab completes commands and names · Ctrl-C clears the line · Ctrl-D or q exits"));
    println!();
    for (group_name, cmds) in HELP_GROUPS {
        println!("  {}", c_warn(group_name));
        for (shortcut, name, desc) in *cmds {
            let sc = if shortcut.is_empty() {
                format!("  {}  ", fmt::pad_right("", 3))
            } else {
                format!("  {} {}", c_accent(&fmt::pad_right(shortcut, 3)), c_muted("|"))
            };
            println!("  {} {}  {}", sc, c_accent(&fmt::pad_right(name, 22)), c_muted(desc));
        }
        println!();
    }
}

fn inline_help(cmd: &str) -> Option<(&'static str, &'static str)> {
    INLINE_HELP.iter()
        .find(|(aliases, _, _)| aliases.contains(&cmd))
        .map(|(_, usage, desc)| (*usage, *desc))
}

fn print_inline_help(cmd: &str) {
    match inline_help(cmd) {
        Some((usage, desc)) => println!("  {} {}", c_accent(usage), c_muted(desc)),
        None => println!("  {} unknown command: {}. Type {} for help.", c_danger("✗"), cmd, c_accent("?")),
    }
}


struct IloldPrompt {
    contract: String,
    steps: Vec<String>,
    scenario: String,
}

impl IloldPrompt {
    fn label(&self) -> String {
        if self.scenario == "main" {
            self.contract.clone()
        } else {
            format!("{}/{}", self.contract, self.scenario)
        }
    }
}

impl Prompt for IloldPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        let label = self.label();
        if self.steps.is_empty() {
            Cow::Owned(format!("ilold[{}]", label))
        } else if self.steps.len() <= 3 {
            let path = self.steps.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(" → ");
            Cow::Owned(format!("ilold[{} → {}]", label, path))
        } else {
            let skipped = self.steps.len() - 2;
            Cow::Owned(format!(
                "ilold[{} → {} → ...{} more → {}]",
                label, self.steps[0], skipped, self.steps.last().unwrap()
            ))
        }
    }

    fn render_prompt_right(&self) -> Cow<'_, str> { Cow::Borrowed("") }
    fn render_prompt_indicator(&self, _: PromptEditMode) -> Cow<'_, str> { Cow::Borrowed("> ") }
    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> { Cow::Borrowed(".. ") }
    fn render_prompt_history_search_indicator(&self, search: PromptHistorySearch) -> Cow<'_, str> {
        match search.status {
            PromptHistorySearchStatus::Passing => Cow::Borrowed("(search) "),
            PromptHistorySearchStatus::Failing => Cow::Borrowed("(search failed) "),
        }
    }
}


struct IloldCompleter {
    functions: Vec<String>,
    state_vars: Vec<String>,
    contracts: Vec<String>,
    scenarios: Vec<String>,
}

const COMMAND_WORDS: &[&str] = &[
    "back", "browser", "call", "clear", "contracts", "deps", "exit", "export", "finding", "findings",
    "funcs-all", "functions", "help", "info", "load", "note", "quit", "save", "scenario", "sequence",
    "session", "slice", "state", "status", "step", "timeline", "trace", "use", "usedby", "vars",
    "vars-all", "who",
];
const SCENARIO_SUBCOMMANDS: &[&str] = &["new", "list", "switch", "fork", "delete"];
const SEVERITIES: &[&str] = &["critical", "high", "medium", "low", "info"];
const STATUSES: &[&str] = &["reviewed", "suspicious", "vulnerable", "clean", "inprogress", "notreviewed"];

impl IloldCompleter {
    fn candidates(&self, before: &str) -> (usize, Vec<String>) {
        let word_start = before.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
        let partial = &before[word_start..];
        let words: Vec<&str> = before[..word_start].split_whitespace().collect();
        let owned = |list: &[String]| list.to_vec();
        let fixed = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        let pool: Vec<String> = match words.as_slice() {
            [] => fixed(COMMAND_WORDS),
            [cmd, rest @ ..] => match (cmd.to_lowercase().as_str(), rest) {
                ("c" | "call" | "i" | "info", []) => owned(&self.functions),
                ("status", []) => owned(&self.functions),
                ("status", [_]) => fixed(STATUSES),
                ("tr" | "trace", []) => {
                    let mut v = owned(&self.functions);
                    v.push("step".into());
                    v
                }
                ("w" | "who" | "tl" | "timeline", []) => owned(&self.state_vars),
                ("sl" | "slice", []) => owned(&self.functions),
                ("sl" | "slice", [_]) => owned(&self.state_vars),
                ("use" | "deps" | "dep" | "usedby" | "rdeps", []) => owned(&self.contracts),
                ("sc" | "scen" | "scenario", []) => fixed(SCENARIO_SUBCOMMANDS),
                ("sc" | "scen" | "scenario", [sub]) if matches!(*sub, "switch" | "fork" | "delete" | "rm") => {
                    owned(&self.scenarios)
                }
                ("fi" | "finding", []) => fixed(SEVERITIES),
                _ => Vec::new(),
            },
        };
        let needle = partial.to_lowercase();
        let mut out: Vec<String> = pool.into_iter().filter(|c| c.to_lowercase().starts_with(&needle)).collect();
        out.dedup();
        (word_start, out)
    }
}

impl Completer for IloldCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let (start, values) = self.candidates(&line[..pos]);
        values
            .into_iter()
            .map(|value| Suggestion {
                value,
                display_override: None,
                description: None,
                style: None,
                extra: None,
                span: Span::new(start, pos),
                append_whitespace: true,
                match_indices: None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_from_queued_key_events() {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
        let key = |code| Ok(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        let answer = |events: Vec<std::io::Result<Event>>| {
            let mut echo = Vec::new();
            (collect_answer(events.into_iter(), &mut echo), String::from_utf8(echo).unwrap())
        };

        assert_eq!(answer(vec![key(KeyCode::Char('y')), key(KeyCode::Enter), key(KeyCode::Char('x'))]), (Some("y".into()), "y".into()));
        let typo = vec![key(KeyCode::Char('n')), key(KeyCode::Backspace), key(KeyCode::Backspace), key(KeyCode::Char('y')), key(KeyCode::Enter)];
        assert_eq!(answer(typo).0.as_deref(), Some("y"));
        let mut released = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE);
        released.kind = KeyEventKind::Release;
        assert_eq!(answer(vec![Ok(Event::Key(released)), key(KeyCode::Enter)]).0.as_deref(), Some(""));
        assert_eq!(answer(vec![Ok(Event::FocusGained), key(KeyCode::Char('y')), key(KeyCode::Esc)]).0, None);
        let ctrl_c = Ok(Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        assert_eq!(answer(vec![ctrl_c]).0, None);
        assert_eq!(answer(vec![Err(std::io::Error::other("closed"))]).0, None);
        assert_eq!(answer(vec![key(KeyCode::Char('y'))]).0, None, "input ended before Enter");
    }

    fn cross() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/cross")
    }

    fn server() -> (tokio::runtime::Runtime, std::sync::Arc<ilold_web::state::AppState>, String) {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let (state, port) = rt.block_on(ilold_web::start_server(vec![cross()], 0, 2)).unwrap();
        (rt, state, format!("http://127.0.0.1:{port}"))
    }

    #[test]
    fn send_command_surfaces_server_error_text() {
        let (rt, _state, url) = server();
        let client = reqwest::Client::new();
        let body = serde_json::json!({ "contract": "Nope", "command": "Session" });
        let err = send_command(rt.handle(), &client, &url, &body).unwrap_err();
        assert_eq!(err, "Contract 'Nope' not found");
    }

    #[test]
    fn start_contract_matches_server_default_and_validates_explicit_names() {
        let (rt, state, url) = server();
        let client = reqwest::Client::new();
        let info = send_get(rt.handle(), &client, &format!("{url}/api/project")).unwrap();

        let attached = start_contract(&info, None).unwrap();
        let local = state.project.default_contract().unwrap().name.clone();
        assert_eq!(attached, local);
        assert_eq!(attached, state.scenarios.read().unwrap().current_contract());
        assert_ne!(attached, "unknown");

        assert_eq!(start_contract(&info, Some("Vault")).unwrap(), "Vault");
        let err = start_contract(&info, Some("Nope")).unwrap_err().to_string();
        assert!(err.contains("Contract 'Nope' not found") && err.contains("Vault"), "{err}");
    }

    #[test]
    fn sync_session_reflects_changes_made_by_other_clients() {
        let (rt, state, url) = server();
        let client = reqwest::Client::new();
        assert_eq!(
            sync_session(rt.handle(), &client, &url, "Vault"),
            Some(SessionSnapshot { active: "main".into(), steps: vec![], scenarios: vec!["main".into()] }),
        );

        let call = serde_json::json!({ "contract": "Vault", "command": { "Call": { "func": "depositVia" } } });
        send_command(rt.handle(), &client, &url, &call).unwrap();
        let other = serde_json::json!({ "contract": "LendingPool", "command": "Session" });
        send_command(rt.handle(), &client, &url, &other).unwrap();

        assert_eq!(
            sync_session(rt.handle(), &client, &url, "Vault"),
            Some(SessionSnapshot { active: "main".into(), steps: vec!["depositVia".to_string()], scenarios: vec!["main".into()] }),
        );
        // syncing is a read: it does not take the session over
        assert_eq!(state.scenarios.read().unwrap().current_contract(), "LendingPool");
    }

    #[test]
    fn sync_session_follows_scenario_switch_and_fork() {
        let (rt, _state, url) = server();
        let client = reqwest::Client::new();
        let cmd = |command: serde_json::Value| {
            send_command(rt.handle(), &client, &url, &serde_json::json!({ "contract": "Vault", "command": command })).unwrap()
        };
        cmd(serde_json::json!({ "Call": { "func": "depositVia" } }));
        cmd(serde_json::json!({ "Scenario": { "sub": { "Fork": { "name": "alt" } } } }));
        cmd(serde_json::json!({ "Scenario": { "sub": { "Switch": { "name": "alt" } } } }));
        let snap = sync_session(rt.handle(), &client, &url, "Vault").unwrap();
        assert_eq!(snap.active, "alt");
        assert_eq!(snap.steps, vec!["depositVia".to_string()]);
        assert_eq!(snap.scenarios, vec!["main".to_string(), "alt".to_string()]);
    }

    fn trace(arg: &str) -> Result<TraceArgs, String> {
        parse_trace_args(arg)
    }

    #[test]
    fn parse_trace_args_accepts_valid_forms() {
        let t = trace("deposit --depth 3 --reverts +4 +7 -i").unwrap();
        assert_eq!(t, TraceArgs {
            target: TraceTarget::Function("deposit".into()),
            depth: Some(3), reverts: true, expand: vec![4, 7], interactive: true,
        });
        let t = trace("step 2 -i --depth 5").unwrap();
        assert_eq!(t.target, TraceTarget::SessionStep(2));
        assert!(t.interactive);
        assert_eq!(t.depth, Some(5), "flags after `trace step N` apply");
        assert!(t.has_render_flags());
        assert!(!trace("step 0 -i").unwrap().has_render_flags());
    }

    #[test]
    fn parse_trace_args_rejects_bad_input() {
        let err = trace("f --bogus").unwrap_err();
        assert!(err.contains("Unknown flag '--bogus'"), "{err}");
        let err = trace("f --depth x").unwrap_err();
        assert!(err.contains("--depth needs a non-negative integer, got 'x'"), "{err}");
        assert!(trace("f --depth").unwrap_err().contains("--depth needs a value"));
        assert!(trace("step").unwrap_err().contains("trace step needs a step index"));
        assert!(trace("step two").unwrap_err().contains("Step index must be a non-negative integer"));
        assert!(trace("f g").unwrap_err().contains("Unexpected argument 'g'"));
        assert!(trace("f +x").unwrap_err().contains("+N"));
        assert!(trace("--reverts").unwrap_err().contains("trace needs a function"));
    }

    #[test]
    fn function_trace_url_encodes_flags() {
        let t = trace("f --depth 3 --reverts +1 +2").unwrap();
        assert_eq!(function_trace_url("http://h", "C", "f", &t), "http://h/api/session/trace/C/f?depth=3&reverts=true&expand=1,2");
        let t = trace("f -i").unwrap();
        assert_eq!(function_trace_url("http://h", "C", "f", &t), "http://h/api/session/trace/C/f?depth=4");
        let t = trace("f").unwrap();
        assert_eq!(function_trace_url("http://h", "C", "f", &t), "http://h/api/session/trace/C/f");
        assert_eq!(
            function_trace_url("http://h", "src/a b.sol:A", "f(uint256[] storage)", &t),
            "http://h/api/session/trace/src%2Fa%20b.sol:A/f(uint256%5B%5D%20storage)",
        );
    }

    #[test]
    fn dot_segments_fail_before_the_request() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let t = trace("..").unwrap();
        let err = send_get(rt.handle(), &client, &function_trace_url("http://127.0.0.1:1", "C", "..", &t)).unwrap_err();
        assert_eq!(err, "'..' is not a valid path segment");
        let url = format!("http://127.0.0.1:1/api/session/timeline/{}?contract=C", enc("."));
        assert_eq!(send_get(rt.handle(), &client, &url).unwrap_err(), "'.' is not a valid path segment");
    }

    #[test]
    fn session_names_cannot_escape_the_sessions_dir() {
        for bad in ["../x", "../../escape-t2", "/etc/passwd", "a/b", "a\\b", "..", ".hidden", "", "a b", "~/x"] {
            assert!(session_stem(bad).is_err(), "{bad:?} accepted");
        }
        assert_eq!(session_stem("my-audit").unwrap(), "my-audit");
        assert_eq!(session_stem("pool_v3.1").unwrap(), "pool_v3.1");
        assert_eq!(session_stem("my-audit.json").unwrap(), "my-audit");
        let path = session_file("my-audit").unwrap();
        assert_eq!(path.parent().unwrap(), sessions_dir());
        assert_eq!(path.file_name().unwrap(), "my-audit.json");
        for reserved in ["con", "NUL", "aux.v1", "Com1", "lpt9.json", "trailing."] {
            assert!(session_stem(reserved).is_err(), "{reserved:?} accepted");
        }
        assert!(session_stem("console").is_ok() && session_stem("com10").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_sessions_are_refused() {
        let root = std::env::temp_dir().join(format!("ilold-session-links-{}", std::process::id()));
        let real = root.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("target.json"), "{}").unwrap();
        std::os::unix::fs::symlink(&real, root.join("linked")).unwrap();
        std::os::unix::fs::symlink(real.join("target.json"), real.join("alias.json")).unwrap();

        assert!(check_session_path(&real.join("new.json")).is_ok());
        assert!(check_session_path(&real.join("target.json")).is_ok());
        let err = check_session_path(&root.join("linked").join("new.json")).unwrap_err();
        assert!(err.contains("symbolic link"), "{err}");
        assert!(check_session_path(&real.join("alias.json")).unwrap_err().contains("symbolic link"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn attached_contract_list_shows_each_kind() {
        let (rt, state, url) = server();
        let client = reqwest::Client::new();
        let info = send_get(rt.handle(), &client, &format!("{url}/api/project")).unwrap();
        colored::control::set_override(false);
        let attached = render_contracts(&contract_rows(&info), "Vault", info["hidden_contracts"].as_u64().unwrap() as usize, false);
        let local = render_contracts(&local_rows(&state.project, false), "Vault", state.project.hidden_contracts(), false);
        for line in ["[I] IPool", "[L] SafeMath", "[A] BasePool", "[C] LendingPool", "[C] Vault"] {
            assert!(attached.contains(line), "{line} in {attached}");
        }
        assert!(attached.contains("← current"));
        assert_eq!(attached, local);
    }

    #[test]
    fn attached_and_local_footers_count_the_same_hidden_contracts() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/model/project");
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let (state, port) = rt.block_on(ilold_web::start_server(vec![root], 0, 2)).unwrap();
        let info = send_get(rt.handle(), &reqwest::Client::new(), &format!("http://127.0.0.1:{port}/api/project")).unwrap();
        colored::control::set_override(false);
        let hidden = info["hidden_contracts"].as_u64().unwrap() as usize;
        assert!(hidden > 0);
        let attached = render_contracts(&contract_rows(&info), "Vault", hidden, false);
        let local = render_contracts(&local_rows(&state.project, false), "Vault", state.project.hidden_contracts(), false);
        assert!(local.contains(&format!("{hidden} dependency or free-function contract(s) hidden")), "{local}");
        assert_eq!(attached, local);
    }

    #[test]
    fn every_help_entry_and_command_word_has_inline_help() {
        for (_, cmds) in HELP_GROUPS {
            for (shortcut, usage, _) in *cmds {
                if !shortcut.is_empty() {
                    assert!(inline_help(shortcut).is_some(), "no inline help for {shortcut}");
                }
                let first = usage.split_whitespace().next().unwrap();
                for word in first.split('/') {
                    assert!(inline_help(word).is_some(), "no inline help for {word} ({usage})");
                }
            }
        }
        for word in COMMAND_WORDS {
            assert!(inline_help(word).is_some(), "no inline help for {word}");
        }
        assert!(inline_help("q").is_some());
        assert!(inline_help("nope").is_none());
        let (usage, _) = inline_help("sc").unwrap();
        assert!(usage.contains("new <name>") && usage.contains("fork <name> [at N]") && usage.contains("delete <name>"), "{usage}");
    }

    fn completer() -> IloldCompleter {
        IloldCompleter {
            functions: vec!["deposit".into(), "depositVia".into(), "_accrue".into(), "withdraw".into()],
            state_vars: vec!["totalSupplied".into(), "balances".into()],
            contracts: vec!["Vault".into(), "LendingPool".into()],
            scenarios: vec!["main".into(), "attack".into()],
        }
    }

    fn values(before: &str) -> Vec<String> {
        completer().candidates(before).1
    }

    #[test]
    fn completer_offers_names_by_argument_position() {
        assert_eq!(values("dep"), vec!["deps".to_string()]);
        assert_eq!(values("c dep"), vec!["deposit".to_string(), "depositVia".to_string()]);
        assert_eq!(values("tr _"), vec!["_accrue".to_string()], "inherited functions complete");
        assert!(values("tr ").contains(&"step".to_string()));
        assert_eq!(values("who tot"), vec!["totalSupplied".to_string()]);
        assert_eq!(values("tl b"), vec!["balances".to_string()]);
        assert_eq!(values("w d"), Vec::<String>::new(), "who completes state variables, not functions");
        assert_eq!(values("sl w"), vec!["withdraw".to_string()]);
        assert_eq!(values("sl withdraw bal"), vec!["balances".to_string()]);
        assert_eq!(values("use L"), vec!["LendingPool".to_string()]);
        assert_eq!(values("sc s"), vec!["switch".to_string()]);
        assert_eq!(values("sc switch a"), vec!["attack".to_string()]);
        assert_eq!(values("scenario delete "), vec!["main".to_string(), "attack".to_string()]);
        assert_eq!(values("sc fork m"), vec!["main".to_string()]);
        assert_eq!(values("sc new a"), Vec::<String>::new());
        assert_eq!(values("status deposit re"), vec!["reviewed".to_string()]);
        assert_eq!(values("C DEP").len(), 2, "case-insensitive");
        assert_eq!(completer().candidates("c dep").0, 2);
    }

    #[test]
    fn argument_checks_name_close_matches() {
        let names = ContractNames { functions: vec!["supply".into()], state_vars: vec!["balances".into()] };
        assert!(check_state_var("balances", "C", &names).is_ok());
        assert!(check_state_var("balances[msg.sender]", "C", &names).is_ok());
        let err = check_state_var("balance", "C", &names).unwrap_err();
        assert!(err.contains("Unknown state variable 'balance' in C. Did you mean: balances?"), "{err}");
        assert!(check_state_var("nope", "C", &names).unwrap_err().contains("Unknown state variable 'nope'"));
        assert!(check_state_var("balances extra", "C", &names).unwrap_err().contains("unexpected: 'extra'"));
        assert!(check_function("supply", "C", &names).is_ok());
        assert!(check_function("suply", "C", &names).unwrap_err().contains("Did you mean: supply?"));
        assert!(check_state_var("anything", "C", &ContractNames::default()).is_ok());
        assert_eq!(extra_args("supply extra args"), Some("extra args"));
        assert_eq!(extra_args("supply"), None);
    }

    fn parse_fixture(name: &str) -> ilold_core::model::project::Project {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc").join(name);
        ilold_core::parse::solc_frontend::SolcFrontend.parse_project(&root).expect("parse fixture")
    }

    #[test]
    fn overloaded_functions_complete_by_signature() {
        let project = parse_fixture("call_resolution");
        let names = &local_names(&project)["Over"];
        for sig in ["put(uint256)", "put(address)", "put(uint256,bool)"] {
            assert!(names.functions.contains(&sig.to_string()), "{names:?}");
        }
        assert!(!names.functions.contains(&"put".to_string()), "bare overloaded name: {names:?}");
        assert!(check_function("put", "Over", names).is_ok(), "bare name resolves leniently");
        assert!(check_function("put(address)", "Over", names).is_ok());
        assert!(check_function("pu", "Over", names).is_err());
        assert_eq!(
            function_names(&[("f".into(), "f(uint256[] storage)".into()), ("f".into(), "f(uint256[] memory)".into()), ("g".into(), "g()".into())]),
            vec!["f(uint256[]storage)".to_string(), "f(uint256[]memory)".to_string(), "g".to_string()],
        );
    }

    #[test]
    fn duplicate_contract_names_are_keyed_by_qualified_id() {
        let project = parse_fixture("model/project");
        let names = local_names(&project);
        assert!(names.contains_key("Vault"), "{:?}", names.keys());
        assert!(names.contains_key("src/ext/IToken.sol:IToken") && !names.contains_key("IToken"), "{:?}", names.keys());
        let contracts: Vec<String> = project.project_contracts().map(|c| c.key().to_string()).collect();
        let comp = IloldCompleter { contracts, ..completer() };
        let tokens = comp.candidates("use src/ext/I").1;
        assert_eq!(tokens, vec!["src/ext/IToken.sol:IToken".to_string()]);
    }

    #[test]
    fn step_index_errors_are_friendly() {
        let steps = vec!["a".to_string(), "b".to_string()];
        assert!(parse_step_index("abc").unwrap_err().contains("non-negative integer, got 'abc'"));
        assert!(check_step_index(1, &steps).is_ok());
        assert!(check_step_index(5, &steps).unwrap_err().contains("has 2 step(s), 0-1"));
        assert!(check_step_index(0, &[]).unwrap_err().contains("No steps yet"));
    }

    #[test]
    fn local_and_attached_names_include_inherited_members() {
        let (rt, state, url) = server();
        let client = reqwest::Client::new();
        let local = local_names(&state.project);
        let detail = send_get(rt.handle(), &client, &format!("{url}/api/contract/LendingPool")).unwrap();
        let attached = ContractNames::from_detail(&detail);
        for names in [&local["LendingPool"], &attached] {
            assert!(names.functions.contains(&"supply".to_string()), "{names:?}");
            assert!(names.state_vars.contains(&"balances".to_string()), "{names:?}");
            assert!(names.state_vars.contains(&"totalSupplied".to_string()), "inherited var: {names:?}");
        }
    }

    #[test]
    fn findings_list_shows_severity_and_title() {
        let (rt, _state, url) = server();
        let client = reqwest::Client::new();
        let finding = serde_json::json!({ "contract": "Vault", "command": {
            "Finding": { "severity": "High", "title": "Unchecked pool return", "description": "" } } });
        send_command(rt.handle(), &client, &url, &finding).unwrap();
        let saved = send_command(rt.handle(), &client, &url, &serde_json::json!({ "contract": "Vault", "command": "SaveSession" })).unwrap();
        let CommandResult::SessionSaved { json } = saved else { panic!("{saved:?}") };
        let (scenario, findings) = active_findings(&json).unwrap();
        assert_eq!(scenario, "main");
        colored::control::set_override(false);
        let out = render_findings("Vault", &scenario, &findings);
        assert!(out.contains("1 finding(s) in Vault/main"), "{out}");
        assert!(out.contains("F-01 [High] Unchecked pool return  session-level"), "{out}");
    }
}
