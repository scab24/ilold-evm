use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use colored::Colorize;
use reedline::{
    Completer, DefaultHinter, FileBackedHistory, Prompt, PromptEditMode,
    PromptHistorySearch, PromptHistorySearchStatus, Reedline, Signal, Span, Suggestion,
};

use ilold_core::classify::entry_points::AccessLevel;
use ilold_core::exploration::commands::CommandResult;

use crate::colors::*;
use crate::fmt;

pub async fn run(paths: Vec<PathBuf>, port: u16, max_seq_depth: usize, attach: Option<String>) -> Result<()> {
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
        let contract_name = contracts_arr
            .and_then(|arr| arr.iter().rev().find(|c| c["kind"].as_str() == Some("Contract")))
            .or_else(|| contracts_arr.and_then(|arr| arr.last()))
            .and_then(|c| c["name"].as_str())
            .unwrap_or("unknown")
            .to_string();

        let mut functions_by_contract = HashMap::<String, Vec<String>>::new();
        let contract_names_raw: Vec<String> = contracts_arr
            .map(|arr| arr.iter().filter_map(|c| c["name"].as_str().map(String::from)).collect())
            .unwrap_or_default();
        for cname in &contract_names_raw {
            if let Ok(resp) = client.get(format!("{url}/api/contract/{cname}")).send().await {
                if let Ok(detail) = resp.json::<serde_json::Value>().await {
                    let funcs: Vec<String> = detail["functions"].as_array()
                        .map(|fs| fs.iter().filter_map(|f| f["name"].as_str().map(String::from)).collect())
                        .unwrap_or_default();
                    functions_by_contract.insert(cname.clone(), funcs);
                }
            }
        }

        let function_names = functions_by_contract.get(&contract_name).cloned().unwrap_or_default();
        let contract_names: Vec<String> = functions_by_contract.keys().cloned().collect();
        let func_count = function_names.len();

        let banner = fmt::header_box(&[
            &format!("ilold explore — {} (attached)", contract_name),
            &format!("{} functions | Type ? for help", func_count),
            &format!("Server: {}", url),
        ]);
        println!("{}\n", banner);

        let base_url = url;
        let handle = tokio::runtime::Handle::current();
        let repl_thread = std::thread::spawn(move || {
            repl_loop(handle, contract_name, function_names, contract_names, None, base_url, Some(functions_by_contract));
        });
        repl_thread.join().map_err(|_| anyhow::anyhow!("REPL thread panicked"))?;
        return Ok(());
    }

    println!("Analyzing {} file(s)...", paths.len());
    let (state, actual_port) = ilold_web::start_server(paths, port, max_seq_depth).await?;

    let contract_name = state.project.find_contract(None)
        .map(|c| c.name.clone())
        .unwrap_or_else(|_| "unknown".into());

    let function_names: Vec<String> = state.project.contracts.iter()
        .find(|c| c.name == contract_name)
        .map(|c| {
            state.project
                .accessible_functions(c)
                .iter()
                .map(|af| af.function.name.clone())
                .collect()
        })
        .unwrap_or_default();

    let contract_names: Vec<String> = state.project.contracts.iter()
        .map(|c| c.name.clone())
        .filter(|n| !n.is_empty())
        .collect();

    let func_count = function_names.len();

    let banner = fmt::header_box(&[
        &format!("ilold explore — {}", contract_name),
        &format!("{} functions | Type ? for help", func_count),
        &format!("Web UI: http://localhost:{}", actual_port),
    ]);
    println!("{}\n", banner);

    let mut by_folder: std::collections::BTreeMap<String, Vec<&str>> = std::collections::BTreeMap::new();
    for c in &state.project.contracts {
        by_folder.entry(state.project.contract_folder(c)).or_default().push(c.name.as_str());
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
    let base_url = format!("http://127.0.0.1:{}", actual_port);
    let repl_thread = std::thread::spawn(move || {
        repl_loop(handle, contract_name, function_names, contract_names, Some(state_for_thread), base_url, None);
    });

    repl_thread.join().map_err(|_| anyhow::anyhow!("REPL thread panicked"))?;
    Ok(())
}

fn repl_loop(
    handle: tokio::runtime::Handle,
    mut contract: String,
    mut functions: Vec<String>,
    contract_names: Vec<String>,
    state: Option<std::sync::Arc<ilold_web::state::AppState>>,
    base_url: String,
    functions_by_contract: Option<HashMap<String, Vec<String>>>,
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

    let completer = std::sync::Arc::new(std::sync::Mutex::new(IloldCompleter {
        functions: functions.clone(),
        contracts: contract_names.clone(),
        scenarios: vec!["main".to_string()],
    }));

    let mut editor = Reedline::create()
        .with_history(history)
        .with_completer(Box::new(CompleterWrapper(completer.clone())))
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

    if state.is_none() {
        if let Some(server_steps) = sync_steps(&handle, &client, &base_url, &contract) {
            steps = server_steps;
            prompt.steps = steps.clone();
        }
    }

    loop {
        if state.is_none() {
            if let Some(server_steps) = sync_steps(&handle, &client, &base_url, &contract) {
                if server_steps != steps {
                    steps = server_steps;
                    prompt.steps = steps.clone();
                }
            }
        }

        match editor.read_line(&prompt) {
            Ok(Signal::Success(line)) => {
                let line = line.trim();
                if line.is_empty() { continue; }

                match handle_input(
                    line, &handle, &client, &base_url, &contract,
                    &mut steps, &mut scenario_name, &completer, &state,
                ) {
                    InputResult::Continue => {}
                    InputResult::Quit => break,
                    InputResult::UpdatePrompt => {
                        prompt.steps = steps.clone();
                        prompt.scenario = scenario_name.clone();
                    }
                    InputResult::SwitchContract(new_name) => {
                        contract = new_name.clone();
                        steps.clear();
                        scenario_name = "main".into();
                        if let Some(ref state) = state {
                            if let Some(c) = state.project.contracts.iter().find(|c| c.name == new_name) {
                                functions = state.project
                                    .accessible_functions(c)
                                    .iter()
                                    .map(|af| af.function.name.clone())
                                    .collect();
                                if let Ok(mut comp) = completer.lock() {
                                    comp.functions = functions.clone();
                                }
                            }
                        } else if let Some(ref fbc) = functions_by_contract {
                            if let Some(funcs) = fbc.get(&new_name) {
                                functions = funcs.clone();
                                if let Ok(mut comp) = completer.lock() {
                                    comp.functions = functions.clone();
                                }
                            }
                        }
                        prompt.contract = contract.clone();
                        prompt.steps = Vec::new();
                        prompt.scenario = scenario_name.clone();
                        if let Ok(mut comp) = completer.lock() {
                            comp.scenarios = vec!["main".to_string()];
                        }
                    }
                }
            }
            Ok(Signal::CtrlD) | Ok(Signal::CtrlC) => break,
            Err(e) => {
                eprintln!("Error: {e}");
                break;
            }
        }
    }
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
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    base_url: &str,
    contract: &str,
    steps: &mut Vec<String>,
    scenario_name: &mut String,
    completer: &std::sync::Arc<std::sync::Mutex<IloldCompleter>>,
    state: &Option<std::sync::Arc<ilold_web::state::AppState>>,
) -> InputResult {
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
            println!("  {}", c_muted("Web UI not yet available in explore mode."));
            println!("  API running at {base_url}/api/");
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
            if let Some(ref state) = state {
                print_contracts(state, contract);
            } else {
                match handle.block_on(async {
                    let resp = client.get(format!("{base_url}/api/project")).send().await?;
                    resp.json::<serde_json::Value>().await
                }) {
                    Ok(info) => {
                        if let Some(arr) = info["contracts"].as_array() {
                            println!();
                            for c in arr {
                                let name = c["name"].as_str().unwrap_or("?");
                                let marker = if name == contract { c_ok(" ← current").to_string() } else { String::new() };
                                println!("  {} {}{}", c_accent("[C]"), name, marker);
                            }
                            println!();
                        }
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
            if let Some(ref state) = state {
                match state.project.find_contract(Some(arg)) {
                    Ok(c) => {
                        let name = c.name.clone();
                        if name == contract {
                            println!("  Already using {}", c_accent(&name));
                            return InputResult::Continue;
                        }
                        println!("  {} Now using: {}", c_ok("✓"), c_accent(&name));
                        if !steps.is_empty() {
                            println!("  {}", c_muted(&format!("Cleared {} step(s) from previous contract", steps.len())));
                        }
                        InputResult::SwitchContract(name)
                    }
                    Err(e) => {
                        eprintln!("  {}", c_danger(&e));
                        InputResult::Continue
                    }
                }
            } else {
                let name = arg.to_string();
                if name == contract {
                    println!("  Already using {}", c_accent(&name));
                    return InputResult::Continue;
                }
                println!("  {} Now using: {}", c_ok("✓"), c_accent(&name));
                if !steps.is_empty() {
                    println!("  {}", c_muted(&format!("Cleared {} step(s) from previous contract", steps.len())));
                }
                InputResult::SwitchContract(name)
            }
        }

        "c" | "call" => {
            if arg.is_empty() {
                println!("  Usage: call <function>");
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
                println!("  Clear {} steps? (y/n)", steps.len());
                let mut input = String::new();
                match std::io::stdin().read_line(&mut input) {
                    Ok(_) if input.trim().eq_ignore_ascii_case("y") => { /* proceed */ }
                    _ => {
                        println!("  Cancelled.");
                        return InputResult::Continue;
                    }
                }
            }
            let body = serde_json::json!({
                "contract": contract,
                "command": "Clear"
            });
            match send_command(handle, client, base_url, &body) {
                Ok(result) => {
                    print_result(&result, steps);
                    steps.clear();
                    return InputResult::UpdatePrompt;
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
                Ok(result) => print_result(&result, steps),
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
                                Ok(result) => print_result(&result, steps),
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
            match send_get(handle, client, &format!("{base_url}/api/contract/{contract}")) {
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
            match send_get(handle, client, &format!("{base_url}/api/session/function/{contract}/{arg}")) {
                Ok(val) => print_narrative(&val),
                Err(e) => eprintln!("  {}", c_danger(&e)),
            }
            InputResult::Continue
        }
        "tr" | "trace" => {
            if arg.is_empty() {
                println!("  Usage: trace <function> [--depth N] [--reverts] [+N...] [-i]");
                println!("         trace step <N>");
                return InputResult::Continue;
            }
            let parsed = parse_trace_args(arg);
            let target = match parsed.target {
                Some(t) => t,
                None => {
                    println!("  Usage: trace <function> [--depth N] [--reverts] [+N...] [-i]");
                    println!("         trace step <N>");
                    return InputResult::Continue;
                }
            };
            let url = match target {
                TraceTarget::Function(func_name) => {
                    let mut url = format!("{base_url}/api/session/trace/{contract}/{func_name}");
                    let mut sep = '?';
                    let effective_depth = parsed.depth
                        .or(if parsed.interactive { Some(4) } else { None });
                    if let Some(d) = effective_depth {
                        url.push_str(&format!("{sep}depth={d}"));
                        sep = '&';
                    }
                    if parsed.reverts {
                        url.push_str(&format!("{sep}reverts=true"));
                        sep = '&';
                    }
                    if !parsed.expand.is_empty() {
                        let csv = parsed.expand.iter()
                            .map(|n| n.to_string())
                            .collect::<Vec<_>>()
                            .join(",");
                        url.push_str(&format!("{sep}expand={csv}"));
                    }
                    url
                }
                TraceTarget::SessionStep(idx) => {
                    format!("{base_url}/api/session/step/{idx}/trace")
                }
            };
            match send_get(handle, client, &url) {
                Ok(val) => match serde_json::from_value::<ilold_core::narrative::trace::FlowTree>(val) {
                    Ok(tree) => {
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
            match send_get(handle, client, &format!("{base_url}/api/session/sequence")) {
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
            match send_get(handle, client, &format!("{base_url}/api/session/timeline/{arg}")) {
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
            if positionals.len() < 2 {
                println!("  Usage: slice <function> <variable> [--backward|--forward|--both]");
                return InputResult::Continue;
            }
            let func_name = positionals[0];
            let var_name = positionals[1];
            let mut url = format!("{base_url}/api/session/slice/{func_name}/{var_name}");
            if let Some(d) = direction {
                url.push_str(&format!("?direction={d}"));
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
            match send_get(handle, client, &format!("{base_url}/api/session/step/{arg}/narrative")) {
                Ok(val) => print_narrative(&val),
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
            let body = serde_json::json!({ "contract": contract, "command": "SaveSession" });
            match send_command(handle, client, base_url, &body) {
                Ok(CommandResult::SessionSaved { json }) => {
                    let dir = dirs::home_dir()
                        .map(|h| h.join(".ilold").join("sessions"))
                        .unwrap_or_else(|| std::path::PathBuf::from(".ilold/sessions"));
                    std::fs::create_dir_all(&dir).ok();
                    let path = dir.join(format!("{}.json", arg));
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
            let dir = dirs::home_dir()
                .map(|h| h.join(".ilold").join("sessions"))
                .unwrap_or_else(|| std::path::PathBuf::from(".ilold/sessions"));
            let path = dir.join(format!("{}.json", arg));
            let json = match std::fs::read_to_string(&path) {
                Ok(j) => j,
                Err(e) => {
                    eprintln!("  {} File not found: {} ({})", c_danger("✗"), path.display(), e);
                    return InputResult::Continue;
                }
            };
            let body = serde_json::json!({ "contract": contract, "command": { "LoadSession": { "json": json } } });
            match send_command(handle, client, base_url, &body) {
                Ok(CommandResult::SessionLoaded { steps: loaded_steps, .. }) => {
                    *steps = loaded_steps;
                    println!("  {} Session loaded ({} steps)", c_ok("✓"), steps.len());
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

fn sync_steps(
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    base_url: &str,
    contract: &str,
) -> Option<Vec<String>> {
    let body = serde_json::json!({
        "contract": contract,
        "command": "Session"
    });
    match send_command(handle, client, base_url, &body) {
        Ok(CommandResult::SessionView { steps, .. }) => Some(steps),
        _ => None,
    }
}

fn send_command(
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    base_url: &str,
    body: &serde_json::Value,
) -> Result<CommandResult, String> {
    handle.block_on(async {
        client.post(format!("{base_url}/api/cmd"))
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?
            .json::<CommandResult>()
            .await
            .map_err(|e| format!("Parse failed: {e}"))
    })
}

fn send_get(
    handle: &tokio::runtime::Handle,
    client: &reqwest::Client,
    url: &str,
) -> Result<serde_json::Value, String> {
    handle.block_on(async {
        let resp = client.get(url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(if body.is_empty() {
                format!("Server error: {status}")
            } else {
                body
            });
        }

        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| format!("Parse failed: {e}"))
    })
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

enum TraceTarget {
    Function(String),
    SessionStep(usize),
}

struct TraceArgs {
    target: Option<TraceTarget>,
    depth: Option<usize>,
    reverts: bool,
    expand: Vec<usize>,
    interactive: bool,
}

fn parse_trace_args(arg: &str) -> TraceArgs {
    let tokens: Vec<&str> = arg.split_whitespace().collect();
    let mut target: Option<TraceTarget> = None;
    let mut depth: Option<usize> = None;
    let mut reverts = false;
    let mut expand: Vec<usize> = Vec::new();
    let mut interactive = false;
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if t == "--depth" {
            if let Some(v) = tokens.get(i + 1).and_then(|s| s.parse::<usize>().ok()) {
                depth = Some(v);
                i += 2;
                continue;
            }
            i += 1;
        } else if t == "--reverts" {
            reverts = true;
            i += 1;
        } else if t == "-i" || t == "--interactive" {
            interactive = true;
            i += 1;
        } else if let Some(rest) = t.strip_prefix('+') {
            if let Ok(id) = rest.parse::<usize>() {
                expand.push(id);
            }
            i += 1;
        } else if t == "step"
            && target.is_none()
            && tokens.get(i + 1).and_then(|s| s.parse::<usize>().ok()).is_some()
        {
            let idx = tokens[i + 1].parse::<usize>().unwrap();
            target = Some(TraceTarget::SessionStep(idx));
            i += 2;
        } else if target.is_none() {
            target = Some(TraceTarget::Function(t.to_string()));
            i += 1;
        } else {
            i += 1;
        }
    }
    TraceArgs { target, depth, reverts, expand, interactive }
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
    std::io::Write::flush(&mut std::io::stdout()).ok();
    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            let trimmed = input.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        }
    }
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
        Ok(result) => print_result(&result, steps),
        Err(e) => eprintln!("  {}", c_danger(&e)),
    }
}

fn print_contracts(state: &std::sync::Arc<ilold_web::state::AppState>, current: &str) {
    use ilold_core::model::contract::ContractKind;
    println!();
    let max_name = state.project.contracts.iter()
        .filter(|c| !c.name.is_empty())
        .map(|c| c.name.chars().count())
        .max().unwrap_or(0);

    for c in &state.project.contracts {
        if c.name.is_empty() { continue; }
        let badge = match c.kind {
            ContractKind::Contract => c_accent("[C]"),
            ContractKind::Interface => c_muted("[I]"),
            ContractKind::Library => c_muted("[L]"),
            ContractKind::Abstract => c_warn("[A]"),
        };
        let marker = if c.name == current {
            c_ok(" ← current").to_string()
        } else {
            String::new()
        };
        let padded = fmt::pad_right(&c.name, max_name);
        let details = format!(
            "{} functions, {} state vars",
            c.functions.iter().filter(|f| !f.name.is_empty()).count(),
            c.state_vars.len(),
        );
        let inherits = if c.inherits.is_empty() {
            String::new()
        } else {
            format!(", inherits {}", c.inherits.join(", "))
        };
        println!("  {} {}  {}{}{}", badge, c_accent(&padded), c_muted(&details), c_muted(&inherits), marker);
    }
    println!();
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
        let rows = crate::deps_view::dep_rows(&state.dep_graph, name, reverse);
        crate::deps_view::print_direction(name, reverse, rows);
        return;
    }

    let url = format!("{base_url}/api/contract/{name}/depgraph");
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
    let body = serde_json::json!({ "contract": contract, "command": "Session" });
    match send_command(handle, client, base_url, &body) {
        Ok(CommandResult::SessionView { findings_count, .. }) => {
            if findings_count == 0 {
                println!("  No findings recorded yet.");
            } else {
                println!("  {} finding(s) recorded. Use {} to export.", findings_count, c_accent("export"));
            }
        }
        _ => println!("  Could not retrieve findings."),
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
                println!("  {} {}{}", badge, c_accent(&padded_name), tag_str);
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
                println!("  {} {}{}", badge, c_accent(&padded_name), tag_str);
            }

            if !inherited.is_empty() {
                println!();
                println!("  {}", c_muted("inherited:"));
                for entry in &inherited {
                    let badge = access_colored(&entry.access);
                    let padded_name = fmt::pad_right(&entry.name, max_name);
                    let origin = format!("from {}", entry.origin);
                    println!("  {} {}  {}", badge, c_muted(&padded_name), c_muted(&origin));
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
        let access = val.get("access").and_then(|v| v.as_str()).unwrap_or("");
        let mods = val.get("modifiers").and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|m| m.as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        let mod_str = if mods.is_empty() { String::new() } else { format!(" — {}", c_muted(&mods)) };
        println!("  {} [{}]{}", c_bright(name), c_accent(access), mod_str);
    }

    #[derive(Default)]
    struct TransitiveGroup {
        writes: Vec<String>,
        reads: Vec<String>,
        external: Vec<String>,
        events: Vec<String>,
    }

    enum Section<'a> {
        Paths { total: u64, happy: u64, revert: u64 },
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
        sections.push(Section::Paths { total, happy, revert });
    }

    let collect_strs = |key: &str| -> Vec<String> {
        val.get(key)
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default()
    };

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
            Section::Paths { total, happy, revert } => {
                println!(
                    "  {} {} path(s): {} happy, {} revert",
                    c_muted(branch), total, c_ok(&happy.to_string()), c_danger(&revert.to_string())
                );
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

fn print_help() {
    let groups: &[(&str, &[(&str, &str, &str)])] = &[
        ("Session", &[
            ("c",      "call <func>",      "Add function to sequence"),
            ("b",      "back",             "Remove last step"),
            ("cl",     "clear",            "Reset sequence"),
            ("s",      "state",            "Show accumulated state"),
            ("seq",    "sequence",         "Sequence narrative with dependencies"),
            ("st",     "step <index>",     "Re-inspect a specific step"),
            ("ss",     "session",          "Full session state"),
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
            ("ct",     "contracts",        "List project contracts"),
            ("",       "deps [contract]",  "What this contract depends on"),
            ("",       "usedby [contract]","What depends on it (blast radius)"),
            ("",       "use <contract>",   "Switch active contract"),
        ]),
        ("Findings", &[
            ("fi",     "finding [sev] [t]","Record a finding"),
            ("n",      "note <text>",      "Add note to current step"),
            ("sc",     "scenario <name>",  "Name the current sequence"),
            ("",       "status <f> <s>",   "Change review status"),
            ("fl",     "findings",         "List recorded findings"),
            ("ex",     "export",           "Export findings as markdown"),
        ]),
        ("Workspace", &[
            ("",       "save <name>",      "Save session to disk"),
            ("",       "load <name>",      "Load session from disk"),
            ("",       "browser",          "Open web UI"),
            ("q",      "quit/exit",        "Exit"),
        ]),
    ];

    println!();
    println!("  {}  {}", c_bright("ilold explore"), c_muted("— append ? to any command for inline help (e.g. sl?)"));
    println!();
    for (group_name, cmds) in groups {
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

fn print_inline_help(cmd: &str) {
    let entries: &[(&[&str], &str, &str)] = &[
        (&["c", "call"],      "call <func>",                     "Add function call to session. Example: c deposit"),
        (&["b", "back"],      "back",                            "Remove last step from the session sequence."),
        (&["cl", "clear"],    "clear",                           "Reset the entire session (all steps removed)."),
        (&["s", "state"],     "state",                           "Show accumulated state mutations across all steps."),
        (&["f", "functions"], "functions",                       "List callable functions in the active contract."),
        (&["v", "vars"],      "vars",                            "List state variables of the active contract."),
        (&["w", "who"],       "who <variable>",                  "Show which functions read/write a variable. Example: who totalStaked"),
        (&["i", "info"],      "info <func>",                     "Function detail: paths, reads, writes, calls. Example: i deposit"),
        (&["tr", "trace"],    "trace <func> [--depth N] [-i]",   "Execution flow tree. -i for interactive TUI. Example: tr swap --depth 3"),
        (&["seq", "sequence"],"sequence",                        "Show the narrative of the current call sequence."),
        (&["tl", "timeline"], "timeline <variable>",             "Cross-step mutation history. Example: tl totalStaked"),
        (&["sl", "slice"],    "slice <func> <var> [--backward]", "Dataflow slice. Example: sl deposit totalStaked --backward"),
        (&["st", "step"],     "step <index>",                    "Re-inspect a specific session step. Example: st 0"),
        (&["ss", "session"],  "session",                         "Full session state with all steps."),
        (&["fi", "finding"],  "finding [severity] [text]",       "Record a security finding for the current step."),
        (&["n", "note"],      "note <text>",                     "Attach a note to the current step."),
        (&["sc", "scenario"],"scenario <name>",                  "Name the current sequence. Example: sc reentrancy-attack"),
        (&["fl", "findings"], "findings",                        "List all recorded findings."),
        (&["ex", "export"],   "export",                          "Export findings as a markdown report."),
        (&["fa", "funcs-all"],"funcs-all",                       "List all accessible functions including inherited."),
        (&["va", "vars-all"], "vars-all",                        "List all accessible state variables including inherited."),
        (&["ct", "contracts"],"contracts",                       "List all contracts in the project."),
        (&["deps", "dep"],    "deps [contract]",                 "Contracts the current (or named) contract depends on. Example: deps Vault"),
        (&["usedby", "rdeps"],"usedby [contract]",               "Contracts that depend on it — the blast radius. Example: usedby IPool"),
        (&["use"],            "use <contract>",                  "Switch the active contract. Example: use Staking"),
        (&["status"],         "status <func> <status>",          "Change review status. Example: status deposit reviewed"),
        (&["save"],           "save <name>",                     "Save session to disk. Example: save my-audit"),
        (&["load"],           "load <name>",                     "Load session from disk. Example: load my-audit"),
        (&["browser"],        "browser",                         "Open the web UI in a browser."),
    ];

    for (aliases, usage, desc) in entries {
        if aliases.iter().any(|a| *a == cmd) {
            println!("  {} {}", c_accent(usage), c_muted(desc));
            return;
        }
    }
    println!("  {} unknown command: {}", c_danger("✗"), cmd);
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
    contracts: Vec<String>,
    scenarios: Vec<String>,
}

impl Completer for IloldCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion> {
        let line_lower = line[..pos].to_lowercase();

        let needs_func = line_lower.starts_with("c ")
            || line_lower.starts_with("call ")
            || line_lower.starts_with("i ")
            || line_lower.starts_with("info ")
            || line_lower.starts_with("tr ")
            || line_lower.starts_with("trace ")
            || line_lower.starts_with("w ")
            || line_lower.starts_with("who ")
            || line_lower.starts_with("status ")
            || (line_lower.starts_with("sl ") && line[..pos].matches(' ').count() == 1)
            || (line_lower.starts_with("slice ") && line[..pos].matches(' ').count() == 1);

        let needs_contract = line_lower.starts_with("use ")
            || line_lower.starts_with("deps ")
            || line_lower.starts_with("dep ")
            || line_lower.starts_with("usedby ")
            || line_lower.starts_with("rdeps ");

        let needs_scenario = line_lower.starts_with("scenario switch ")
            || line_lower.starts_with("scenario delete ")
            || line_lower.starts_with("sc switch ")
            || line_lower.starts_with("sc delete ")
            || line_lower.starts_with("scen switch ")
            || line_lower.starts_with("scen delete ");

        if !needs_func && !needs_contract && !needs_scenario {
            return Vec::new();
        }

        let arg_start = line[..pos].rfind(' ').map(|i| i + 1).unwrap_or(0);
        let partial = &line[arg_start..pos];

        let source = if needs_scenario {
            &self.scenarios
        } else if needs_contract {
            &self.contracts
        } else {
            &self.functions
        };

        source.iter()
            .filter(|f| f.starts_with(partial))
            .map(|f| Suggestion {
                value: f.clone(),
                display_override: None,
                description: None,
                style: None,
                extra: None,
                span: Span::new(arg_start, pos),
                append_whitespace: true,
                match_indices: None,
            })
            .collect()
    }
}
