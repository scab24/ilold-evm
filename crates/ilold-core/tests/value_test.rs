//! Value transfers and balance reads from the recognizer table. Fixtures: tests/fixtures/solc/value, tests/fixtures/solc/call_resolution.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use ilold_core::analysis::{AnalysisParams, ProjectAnalysis};
use ilold_core::model::project::Project;
use ilold_core::parse::solc_frontend::SolcFrontend;
use ilold_core::relations::{value_facts, value_flows, AssetKind, BalanceRead, Direction, Operation, Tag, ValueFacts, ValueReason, ValueTransfer};

type Analysed = (Project, ProjectAnalysis);

fn analysed(fixture: &str) -> &'static Analysed {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, &'static Analysed>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    cache.entry(fixture.to_string()).or_insert_with(|| {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc").join(fixture);
        let project = SolcFrontend.parse_project(&root).expect("parse fixture");
        let analysis = ProjectAnalysis::build(&project, &AnalysisParams::new(1));
        Box::leak(Box::new((project, analysis)))
    })
}

fn facts_in(fixture: &str, contract: &str, func: &str) -> ValueFacts {
    let (project, analysis) = analysed(fixture);
    let c = project.lookup_contract(contract).unwrap();
    let f = c.functions.iter().find(|f| f.name == func).unwrap();
    value_facts(project, c, c, f, &analysis.cfgs[&(c.key().to_string(), f.key.clone())])
}

fn transfers(func: &str) -> Vec<ValueTransfer> {
    facts_in("value", "Payments", func).transfers
}

fn reads(func: &str) -> Vec<BalanceRead> {
    facts_in("value", "Payments", func).balance_reads
}

/// asset, operation, token, from, to, amount, id
type Moved = (AssetKind, Operation, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);

fn moved(t: &ValueTransfer) -> Moved {
    (t.asset_kind, t.operation, t.token_expr.clone(), t.from_expr.clone(), t.to_expr.clone(), t.amount_expr.clone(), t.id_expr.clone())
}

fn s(text: &str) -> Option<String> {
    Some(text.to_string())
}

#[test]
fn ether_sent_by_call_transfer_and_send() {
    let call = transfers("payCall");
    assert_eq!(call.iter().map(moved).collect::<Vec<_>>(), [(AssetKind::Native, Operation::Call, None, None, s("to"), s("amount"), None)]);
    assert_eq!((call[0].tag, call[0].direction, call[0].reason, call[0].via_library.as_deref()), (Tag::Extracted, Direction::Out, None, None));
    assert_eq!((call[0].span.start_line, call[0].call.as_str(), call[0].id.as_str()), (35, "to.call", "Payments::payCall#0"));
    // the model drops a literal's unit, so `1 ether` reads `1`
    assert_eq!(transfers("payTransfer").iter().map(moved).collect::<Vec<_>>(), [(AssetKind::Native, Operation::Transfer, None, None, s("to"), s("1"), None)]);
    assert_eq!(transfers("paySend").iter().map(moved).collect::<Vec<_>>(), [(AssetKind::Native, Operation::Send, None, None, s("to"), s("1"), None)]);
}

#[test]
fn a_modifier_transfer_is_part_of_the_function_that_runs_it() {
    let buy = transfers("buy");
    let mint = buy.iter().find(|t| t.operation == Operation::Mint).unwrap();
    assert_eq!(moved(mint), (AssetKind::Erc20, Operation::Mint, s("share"), None, s("msg.sender"), s("price"), None));
    assert_eq!((mint.tag, mint.modifier.as_deref()), (Tag::TypeResolved, None));
    let refund = buy.iter().find(|t| t.asset_kind == AssetKind::Native).unwrap();
    assert_eq!(moved(refund), (AssetKind::Native, Operation::Transfer, None, None, s("payable(msg.sender)"), s("msg.value - price"), None));
    assert_eq!((refund.modifier.as_deref(), refund.span.start_line), (Some("refundExcess"), 31));
    assert_eq!(buy.len(), 2);
}

#[test]
fn token_calls_are_recognized_by_selector_and_the_standard_of_the_receiver() {
    let deposit = transfers("deposit");
    assert_eq!(
        deposit.iter().map(moved).collect::<Vec<_>>(),
        [(AssetKind::Erc20, Operation::TransferFrom, s("asset"), s("msg.sender"), s("address(this)"), s("amount"), None)]
    );
    assert_eq!((deposit[0].direction, deposit[0].tag), (Direction::In, Tag::TypeResolved));
    let withdraw = transfers("withdraw");
    assert_eq!((moved(&withdraw[0]), withdraw[0].direction), ((AssetKind::Erc20, Operation::Transfer, s("asset"), None, s("msg.sender"), s("amount"), None), Direction::Out));

    let nft: Vec<Moved> = transfers("moveNft").iter().map(moved).collect();
    assert_eq!(
        nft,
        [
            (AssetKind::Erc721, Operation::SafeTransferFrom, s("nft"), s("address(this)"), s("to"), None, s("id")),
            (AssetKind::Erc721, Operation::TransferFrom, s("nft"), s("msg.sender"), s("address(this)"), None, s("id")),
            (AssetKind::Erc721, Operation::Mint, s("nft"), None, s("to"), None, s("id")),
        ]
    );
    assert_eq!(
        transfers("moveMulti").iter().map(moved).collect::<Vec<_>>(),
        [(AssetKind::Erc1155, Operation::SafeTransferFrom, s("multi"), s("address(this)"), s("to"), s("amount"), s("id"))]
    );
    let claims = transfers("moveClaims");
    assert_eq!(
        claims.iter().map(moved).collect::<Vec<_>>(),
        [
            (AssetKind::Erc6909, Operation::Transfer, s("manager"), None, s("to"), s("amount"), s("id")),
            (AssetKind::Erc6909, Operation::TransferFrom, s("manager"), s("msg.sender"), s("to"), s("amount"), s("id")),
            (AssetKind::Erc6909, Operation::Mint, s("manager"), None, s("address(this)"), s("amount"), s("id")),
            (AssetKind::Erc6909, Operation::Burn, s("manager"), s("address(this)"), None, s("amount"), s("id")),
        ]
    );
    assert_eq!(claims.iter().map(|t| t.direction).collect::<Vec<_>>(), [Direction::Out, Direction::Other, Direction::In, Direction::Out]);
}

#[test]
fn a_burn_on_an_interface_of_no_standard_is_ambiguous_between_the_standards_it_could_be() {
    let burns = transfers("burnShares");
    assert_eq!(
        burns.iter().map(moved).take(2).collect::<Vec<_>>(),
        [
            (AssetKind::Erc20, Operation::Burn, s("share"), None, None, s("amount"), None),
            (AssetKind::Erc20, Operation::Burn, s("share"), s("msg.sender"), None, s("amount"), None),
        ]
    );
    let unknown = &burns[2];
    assert_eq!((unknown.asset_kind, unknown.operation, unknown.token_expr.as_deref()), (AssetKind::Unknown, Operation::Burn, Some("burnable")));
    assert_eq!((unknown.tag, unknown.reason), (Tag::Ambiguous, Some(ValueReason::NoStandard)));
    assert_eq!(unknown.asset_candidates, [AssetKind::Erc20, AssetKind::Erc721]);
    assert_eq!((unknown.amount_expr.as_deref(), unknown.id_expr.as_deref()), (None, None), "the argument is an amount or an id");
}

#[test]
fn library_transfers_are_recorded_at_the_call_site_with_the_library_function() {
    let safe = transfers("safePay");
    assert_eq!(
        safe.iter().map(|t| (moved(t), t.via_library.clone(), t.tag)).collect::<Vec<_>>(),
        [
            ((AssetKind::Erc20, Operation::Transfer, s("asset"), None, s("to"), s("amount"), None), s("SafeERC20::safeTransfer"), Tag::TypeResolved),
            (
                (AssetKind::Erc20, Operation::TransferFrom, s("asset"), s("msg.sender"), s("to"), s("amount"), None),
                s("SafeERC20::safeTransferFrom"),
                Tag::TypeResolved
            ),
        ]
    );
    let send = transfers("sendEth");
    assert_eq!((moved(&send[0]), send[0].via_library.as_deref()), ((AssetKind::Native, Operation::Call, None, None, s("to"), s("amount"), None), Some("Address::sendValue")));
    assert_eq!(send[0].span.start_line, 88);
    let with_value = transfers("callWithValue");
    assert_eq!(moved(&with_value[0]), (AssetKind::Native, Operation::Call, None, None, s("target"), s("msg.value"), None));
    let solady = transfers("soladyPay");
    assert_eq!(
        solady.iter().map(|t| (moved(t), t.via_library.clone())).collect::<Vec<_>>(),
        [
            ((AssetKind::Native, Operation::Call, None, None, s("to"), s("amount"), None), s("SafeTransferLib::safeTransferETH")),
            ((AssetKind::Erc20, Operation::Transfer, s("address(asset)"), None, s("to"), s("amount"), None), s("SafeTransferLib::safeTransfer")),
        ]
    );
}

#[test]
fn currency_settles_and_takes_follow_the_claims_flag() {
    let settle = transfers("settleQuote");
    let row = |t: &ValueTransfer| (t.asset_kind, t.operation, t.from_expr.clone(), t.to_expr.clone(), t.tag, t.reason, t.asset_candidates.clone());
    let currency = vec![AssetKind::Native, AssetKind::Erc20];
    assert_eq!(
        settle.iter().map(row).collect::<Vec<_>>(),
        [
            (AssetKind::Unknown, Operation::Settle, s("address(this)"), s("manager"), Tag::Ambiguous, Some(ValueReason::Currency), currency.clone()),
            (AssetKind::Erc6909, Operation::Settle, s("address(this)"), s("manager"), Tag::TypeResolved, None, vec![]),
            (
                AssetKind::Unknown,
                Operation::Take,
                s("manager"),
                s("msg.sender"),
                Tag::Ambiguous,
                Some(ValueReason::ClaimsFlag),
                vec![AssetKind::Native, AssetKind::Erc20, AssetKind::Erc6909]
            ),
            (AssetKind::Erc6909, Operation::Take, s("manager"), s("address(this)"), Tag::TypeResolved, None, vec![]),
            (AssetKind::Unknown, Operation::Transfer, None, s("treasury"), Tag::Ambiguous, Some(ValueReason::Currency), currency),
        ]
    );
    assert!(settle.iter().all(|t| t.token_expr.as_deref() == Some("quote") && t.amount_expr.as_deref() == Some("amount")));
    let via: Vec<&str> = settle.iter().filter_map(|t| t.via_library.as_deref()).collect();
    assert_eq!(via, ["CurrencySettler::settle", "CurrencySettler::settle", "CurrencySettler::take", "CurrencySettler::take", "CurrencyLibrary::transfer"]);
}

#[test]
fn balances_are_read_from_ether_tokens_and_libraries() {
    let read = |r: &BalanceRead| (r.asset_kind, r.token_expr.clone(), r.owner_expr.clone(), r.id_expr.clone(), r.tag);
    assert_eq!(
        reads("balances").iter().map(read).collect::<Vec<_>>(),
        [
            (AssetKind::Native, None, s("address(this)"), None, Tag::Extracted),
            (AssetKind::Native, None, s("who"), None, Tag::Extracted),
            (AssetKind::Erc20, s("asset"), s("who"), None, Tag::TypeResolved),
            (AssetKind::Erc721, s("nft"), s("who"), None, Tag::TypeResolved),
            (AssetKind::Erc1155, s("multi"), s("who"), s("id"), Tag::TypeResolved),
            (AssetKind::Erc6909, s("manager"), s("who"), s("id"), Tag::TypeResolved),
            (AssetKind::Erc20, s("address(asset)"), s("who"), None, Tag::TypeResolved),
            (AssetKind::Unknown, s("quote"), None, None, Tag::Ambiguous),
        ]
    );
    let balances = reads("balances");
    assert_eq!((balances[6].via_library.as_deref(), balances[7].via_library.as_deref()), (Some("SafeTransferLib::balanceOf"), Some("CurrencyLibrary::balanceOfSelf")));
    assert_eq!((balances[7].reason, balances[0].id.as_str()), (Some(ValueReason::Currency), "Payments::balances#0"));
    assert!(transfers("balances").is_empty());
}

#[test]
fn unrecognized_calls_emit_nothing() {
    let facts = facts_in("value", "Payments", "unrecognized");
    assert!(facts.transfers.is_empty() && facts.balance_reads.is_empty(), "{facts:#?}");
}

#[test]
fn library_code_records_its_own_transfers_too() {
    let functions = facts_in("value", "Address", "functionCallWithValue").transfers;
    assert_eq!(moved(&functions[0]), (AssetKind::Native, Operation::Call, None, None, s("target"), s("value"), None));
    let settle = facts_in("value", "CurrencySettler", "settle").transfers;
    assert_eq!(
        settle.iter().map(moved).collect::<Vec<_>>(),
        [
            (AssetKind::Erc6909, Operation::Burn, s("manager"), s("payer"), None, s("amount"), s("currency.toId()")),
            (AssetKind::Unknown, Operation::Transfer, s("currency"), None, s("address(manager)"), s("amount"), None),
        ]
    );
}

#[test]
fn existing_fixture_calls_carry_their_value() {
    let caller = |func: &str| facts_in("call_resolution", "Caller", func);
    assert_eq!(
        caller("pay").transfers.iter().map(moved).collect::<Vec<_>>(),
        [
            (AssetKind::Native, Operation::Transfer, None, None, s("to"), s("1"), None),
            (AssetKind::Native, Operation::Send, None, None, s("payable(other)"), s("1"), None),
        ]
    );
    assert_eq!(caller("lowLevel").transfers.iter().map(moved).collect::<Vec<_>>(), [(AssetKind::Native, Operation::Call, None, None, s("target"), s("msg.value"), None)]);
    assert_eq!(caller("viaCast").transfers.iter().map(moved).collect::<Vec<_>>(), [(AssetKind::Erc20, Operation::Transfer, s("IToken(t)"), None, s("to"), s("1"), None)]);
    let read = caller("viaState").balance_reads;
    assert_eq!((read[0].asset_kind, read[0].owner_expr.as_deref(), read[0].tag), (AssetKind::Erc20, Some("msg.sender"), Tag::TypeResolved));
}

#[test]
fn flows_add_up_by_direction_asset_and_counterparty() {
    let (project, analysis) = analysed("value");
    let c = project.lookup_contract("Payments").unwrap();
    let all: Vec<ValueTransfer> = c
        .functions
        .iter()
        .flat_map(|f| value_facts(project, c, c, f, &analysis.cfgs[&(c.key().to_string(), f.key.clone())]).transfers)
        .collect();
    let flows = value_flows(&all);
    let to_sender = flows
        .iter()
        .find(|f| (f.direction, f.asset_kind, f.token_expr.as_deref(), f.counterparty_expr.as_deref()) == (Direction::Out, AssetKind::Erc20, Some("asset"), Some("msg.sender")))
        .unwrap();
    assert_eq!(to_sender.functions, ["Payments::withdraw"]);
    assert_eq!(to_sender.transfers, ["Payments::withdraw#0"]);
    let deposits = flows.iter().find(|f| f.direction == Direction::In && f.token_expr.as_deref() == Some("asset")).unwrap();
    assert_eq!((deposits.counterparty_expr.as_deref(), deposits.functions.as_slice()), (Some("msg.sender"), ["Payments::deposit".to_string()].as_slice()));
    assert_eq!(flows.iter().map(|f| f.transfers.len()).sum::<usize>(), all.len(), "every transfer is in one flow");
    let mut sorted = flows.clone();
    sorted.sort_by(|a, b| (a.direction, a.asset_kind, &a.token_expr, &a.counterparty_expr).cmp(&(b.direction, b.asset_kind, &b.token_expr, &b.counterparty_expr)));
    assert_eq!(flows, sorted);
}
