//! Value a function moves between accounts and the balances it reads, from one table of recognized calls.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::cfg::builder::{call_arguments, expr_to_string};
use crate::cfg::types::{CfgGraph, CfgStatement};
use crate::classify::code_facts::{builtin, walk_function, Builtin};
use crate::model::common::SourceSpan;
use crate::model::contract::{ContractDef, ContractKind};
use crate::model::decl_id::{DeclId, DeclKind};
use crate::model::expression::{CallKind, CallType, Expression, ExpressionKind, LiteralType};
use crate::model::function::FunctionDef;
use crate::model::project::Project;
use crate::model::source::uncast;
use crate::sequence::effects::reachable;

use super::dispatch::contract;
use super::{call_text, Tag};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Native,
    Erc20,
    Erc721,
    Erc1155,
    Erc6909,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Call,
    Create,
    Transfer,
    Send,
    TransferFrom,
    SafeTransferFrom,
    Mint,
    Burn,
    Settle,
    Take,
}

/// Seen from the contract that runs the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    In,
    Out,
    /// Between two other accounts, such as `transferFrom(a, b, x)`.
    Other,
}

/// Why the asset of a fact is not decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueReason {
    /// The call names no declaration, so only its name matches.
    UnresolvedDeclaration,
    /// The receiver's interface follows none of the token standards.
    NoStandard,
    /// The receiver's interface follows several token standards.
    SeveralStandards,
    /// A Uniswap v4 `Currency` is ether or an ERC-20 token at run time.
    Currency,
    /// Whether ERC-6909 claims move depends on a flag that is not a literal.
    ClaimsFlag,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueTransfer {
    /// `<function>#<n>`, `n` counting the function's transfers in source order.
    pub id: String,
    /// `<contract key>::<function key>` declaring the code.
    pub function: String,
    /// Key of the contract that runs the function.
    pub in_context: String,
    /// The modifier whose code makes the call.
    pub modifier: Option<String>,
    /// The call's span; the `calls` fact at the same span is the call.
    pub span: SourceSpan,
    pub asset_kind: AssetKind,
    /// The kinds an AMBIGUOUS fact may be.
    pub asset_candidates: Vec<AssetKind>,
    pub operation: Operation,
    pub direction: Direction,
    pub token_expr: Option<String>,
    /// None: the running contract.
    pub from_expr: Option<String>,
    /// None when the value leaves circulation, as in a burn, or goes to a contract being created.
    pub to_expr: Option<String>,
    pub amount_expr: Option<String>,
    pub id_expr: Option<String>,
    /// `<target>.<function>`, as the call facts name it.
    pub call: String,
    /// `<library key>::<function key>` when a library function moves the value.
    pub via_library: Option<String>,
    pub tag: Tag,
    pub reason: Option<ValueReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalanceRead {
    /// `<function>#<n>`, `n` counting the function's balance reads in source order.
    pub id: String,
    pub function: String,
    pub in_context: String,
    pub modifier: Option<String>,
    pub span: SourceSpan,
    pub asset_kind: AssetKind,
    pub asset_candidates: Vec<AssetKind>,
    pub token_expr: Option<String>,
    /// None: the running contract.
    pub owner_expr: Option<String>,
    pub id_expr: Option<String>,
    pub via_library: Option<String>,
    pub tag: Tag,
    pub reason: Option<ValueReason>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueFacts {
    pub transfers: Vec<ValueTransfer>,
    pub balance_reads: Vec<BalanceRead>,
}

/// Transfers of one direction, asset and token with the same counterparty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueFlow {
    pub direction: Direction,
    pub asset_kind: AssetKind,
    pub token_expr: Option<String>,
    /// The sender of what comes in, the receiver of the rest.
    pub counterparty_expr: Option<String>,
    pub functions: Vec<String>,
    /// Ids of the transfers.
    pub transfers: Vec<String>,
}

/// Where a recognizer reads a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arg {
    /// The account or token the call is made on.
    Receiver,
    /// The `value` call option.
    Value,
    /// An argument; the receiver of a `using for` call is argument 0.
    At(usize),
    /// No expression: for a sender or an owner, the running contract.
    Absent,
}

#[derive(Debug, Clone, Copy)]
enum Asset {
    Is(AssetKind),
    /// A Uniswap v4 `Currency`: ether or an ERC-20 token.
    Currency,
    /// ERC-6909 claims when the bool argument at the index is `true`, else a `Currency`.
    ClaimsIf(usize),
}

#[derive(Debug, Clone, Copy)]
enum Callee {
    /// Calls of these kinds.
    Kind(&'static [CallKind]),
    /// Calls of these kinds that send ether with `{value: ...}`.
    WithValue(&'static [CallKind]),
    /// `<address>.balance`.
    Balance,
    /// An external function by ABI signature and selector, on a receiver whose interface follows the asset's standard.
    Token(&'static str, &'static str),
    /// A function of the library with that name, by the declaration the call resolves to.
    Library(&'static str, &'static str),
}

#[derive(Debug, Clone, Copy)]
enum Effect {
    Moves { operation: Operation, from: Arg, to: Arg, amount: Arg },
    Reads { owner: Arg },
}

#[derive(Debug, Clone, Copy)]
struct Recognizer {
    callee: Callee,
    asset: Asset,
    token: Arg,
    id: Arg,
    effect: Effect,
}

/// Fields in order: token, from, to, amount, id.
const fn moves(callee: Callee, asset: Asset, operation: Operation, [token, from, to, amount, id]: [Arg; 5]) -> Recognizer {
    Recognizer { callee, asset, token, id, effect: Effect::Moves { operation, from, to, amount } }
}

/// Fields in order: token, owner, id.
const fn reads(callee: Callee, asset: Asset, [token, owner, id]: [Arg; 3]) -> Recognizer {
    Recognizer { callee, asset, token, id, effect: Effect::Reads { owner } }
}

use Arg::{Absent, At, Receiver, Value};
use Callee::{Balance, Kind, Library, Token, WithValue};

const NATIVE: Asset = Asset::Is(AssetKind::Native);
const ERC20: Asset = Asset::Is(AssetKind::Erc20);
const ERC721: Asset = Asset::Is(AssetKind::Erc721);
const ERC1155: Asset = Asset::Is(AssetKind::Erc1155);
const ERC6909: Asset = Asset::Is(AssetKind::Erc6909);
const SENDS: &[CallKind] = &[CallKind::LowLevelCall, CallKind::External, CallKind::ExternalSelf];

/// Every call and member access recognized as moving value or reading a balance; anything else yields no fact.
const RECOGNIZERS: &[Recognizer] = &[
    moves(Kind(&[CallKind::Transfer]), NATIVE, Operation::Transfer, [Absent, Absent, Receiver, At(0), Absent]),
    moves(Kind(&[CallKind::Send]), NATIVE, Operation::Send, [Absent, Absent, Receiver, At(0), Absent]),
    moves(WithValue(SENDS), NATIVE, Operation::Call, [Absent, Absent, Receiver, Value, Absent]),
    moves(WithValue(&[CallKind::Creation]), NATIVE, Operation::Create, [Absent, Absent, Absent, Value, Absent]),
    reads(Balance, NATIVE, [Absent, Receiver, Absent]),
    moves(Token("transfer(address,uint256)", "a9059cbb"), ERC20, Operation::Transfer, [Receiver, Absent, At(0), At(1), Absent]),
    moves(Token("transferFrom(address,address,uint256)", "23b872dd"), ERC20, Operation::TransferFrom, [Receiver, At(0), At(1), At(2), Absent]),
    moves(Token("mint(address,uint256)", "40c10f19"), ERC20, Operation::Mint, [Receiver, Absent, At(0), At(1), Absent]),
    moves(Token("burn(uint256)", "42966c68"), ERC20, Operation::Burn, [Receiver, Absent, Absent, At(0), Absent]),
    moves(Token("burn(address,uint256)", "9dc29fac"), ERC20, Operation::Burn, [Receiver, At(0), Absent, At(1), Absent]),
    moves(Token("burnFrom(address,uint256)", "79cc6790"), ERC20, Operation::Burn, [Receiver, At(0), Absent, At(1), Absent]),
    reads(Token("balanceOf(address)", "70a08231"), ERC20, [Receiver, At(0), Absent]),
    moves(Token("transferFrom(address,address,uint256)", "23b872dd"), ERC721, Operation::TransferFrom, [Receiver, At(0), At(1), Absent, At(2)]),
    moves(Token("safeTransferFrom(address,address,uint256)", "42842e0e"), ERC721, Operation::SafeTransferFrom, [Receiver, At(0), At(1), Absent, At(2)]),
    moves(Token("safeTransferFrom(address,address,uint256,bytes)", "b88d4fde"), ERC721, Operation::SafeTransferFrom, [Receiver, At(0), At(1), Absent, At(2)]),
    moves(Token("mint(address,uint256)", "40c10f19"), ERC721, Operation::Mint, [Receiver, Absent, At(0), Absent, At(1)]),
    moves(Token("burn(uint256)", "42966c68"), ERC721, Operation::Burn, [Receiver, Absent, Absent, Absent, At(0)]),
    reads(Token("balanceOf(address)", "70a08231"), ERC721, [Receiver, At(0), Absent]),
    moves(Token("safeTransferFrom(address,address,uint256,uint256,bytes)", "f242432a"), ERC1155, Operation::SafeTransferFrom, [Receiver, At(0), At(1), At(3), At(2)]),
    moves(
        Token("safeBatchTransferFrom(address,address,uint256[],uint256[],bytes)", "2eb2c2d6"),
        ERC1155,
        Operation::SafeTransferFrom,
        [Receiver, At(0), At(1), At(3), At(2)],
    ),
    moves(Token("mint(address,uint256,uint256,bytes)", "731133e9"), ERC1155, Operation::Mint, [Receiver, Absent, At(0), At(2), At(1)]),
    moves(Token("burn(address,uint256,uint256)", "f5298aca"), ERC1155, Operation::Burn, [Receiver, At(0), Absent, At(2), At(1)]),
    reads(Token("balanceOf(address,uint256)", "00fdd58e"), ERC1155, [Receiver, At(0), At(1)]),
    moves(Token("transfer(address,uint256,uint256)", "095bcdb6"), ERC6909, Operation::Transfer, [Receiver, Absent, At(0), At(2), At(1)]),
    moves(Token("transferFrom(address,address,uint256,uint256)", "fe99049a"), ERC6909, Operation::TransferFrom, [Receiver, At(0), At(1), At(3), At(2)]),
    moves(Token("mint(address,uint256,uint256)", "156e29f6"), ERC6909, Operation::Mint, [Receiver, Absent, At(0), At(2), At(1)]),
    moves(Token("burn(address,uint256,uint256)", "f5298aca"), ERC6909, Operation::Burn, [Receiver, At(0), Absent, At(2), At(1)]),
    reads(Token("balanceOf(address,uint256)", "00fdd58e"), ERC6909, [Receiver, At(0), At(1)]),
    moves(Library("SafeERC20", "safeTransfer"), ERC20, Operation::Transfer, [At(0), Absent, At(1), At(2), Absent]),
    moves(Library("SafeERC20", "trySafeTransfer"), ERC20, Operation::Transfer, [At(0), Absent, At(1), At(2), Absent]),
    moves(Library("SafeERC20", "safeTransferFrom"), ERC20, Operation::TransferFrom, [At(0), At(1), At(2), At(3), Absent]),
    moves(Library("SafeERC20", "trySafeTransferFrom"), ERC20, Operation::TransferFrom, [At(0), At(1), At(2), At(3), Absent]),
    moves(Library("Address", "sendValue"), NATIVE, Operation::Call, [Absent, Absent, At(0), At(1), Absent]),
    moves(Library("Address", "functionCallWithValue"), NATIVE, Operation::Call, [Absent, Absent, At(0), At(2), Absent]),
    moves(Library("SafeTransferLib", "safeTransferETH"), NATIVE, Operation::Call, [Absent, Absent, At(0), At(1), Absent]),
    moves(Library("SafeTransferLib", "forceSafeTransferETH"), NATIVE, Operation::Call, [Absent, Absent, At(0), At(1), Absent]),
    moves(Library("SafeTransferLib", "trySafeTransferETH"), NATIVE, Operation::Call, [Absent, Absent, At(0), At(1), Absent]),
    moves(Library("SafeTransferLib", "safeTransfer"), ERC20, Operation::Transfer, [At(0), Absent, At(1), At(2), Absent]),
    moves(Library("SafeTransferLib", "safeTransferFrom"), ERC20, Operation::TransferFrom, [At(0), At(1), At(2), At(3), Absent]),
    reads(Library("SafeTransferLib", "balanceOf"), ERC20, [At(0), At(1), Absent]),
    moves(Library("CurrencyLibrary", "transfer"), Asset::Currency, Operation::Transfer, [At(0), Absent, At(1), At(2), Absent]),
    reads(Library("CurrencyLibrary", "balanceOf"), Asset::Currency, [At(0), At(1), Absent]),
    reads(Library("CurrencyLibrary", "balanceOfSelf"), Asset::Currency, [At(0), Absent, Absent]),
    moves(Library("CurrencySettler", "settle"), Asset::ClaimsIf(4), Operation::Settle, [At(0), At(2), At(1), At(3), Absent]),
    moves(Library("CurrencySettler", "take"), Asset::ClaimsIf(4), Operation::Take, [At(0), At(1), At(2), At(3), Absent]),
];

/// Functions whose selector, declared or inherited, marks an interface as following a token standard.
const STANDARDS: &[(AssetKind, &[(&str, &str)])] = &[
    (AssetKind::Erc20, &[("transfer(address,uint256)", "a9059cbb"), ("allowance(address,address)", "dd62ed3e")]),
    (AssetKind::Erc721, &[("ownerOf(uint256)", "6352211e")]),
    (
        AssetKind::Erc1155,
        &[
            ("safeTransferFrom(address,address,uint256,uint256,bytes)", "f242432a"),
            ("safeBatchTransferFrom(address,address,uint256[],uint256[],bytes)", "2eb2c2d6"),
        ],
    ),
    (AssetKind::Erc6909, &[("transfer(address,uint256,uint256)", "095bcdb6"), ("transferFrom(address,address,uint256,uint256)", "fe99049a")]),
];

/// A recognizer a call matches, and how sure the match is.
struct Match {
    recognizer: Recognizer,
    tag: Tag,
    via_library: Option<String>,
    /// Several standards fit, or none does: the candidates and why.
    undecided: Option<(Vec<AssetKind>, ValueReason)>,
}

/// The expressions a recognizer reads, from the call's source when the code walk found it.
struct Operands<'a> {
    receiver: Option<&'a Expression>,
    args: Vec<&'a Expression>,
    value: Option<&'a Expression>,
    stmt: Option<&'a CfgStatement>,
}

/// The text of a field, and whether it names the running contract.
struct Operand {
    text: Option<String>,
    this: bool,
}

impl Operands<'_> {
    fn of<'a>(stmt: &'a CfgStatement, expr: Option<&'a Expression>) -> Operands<'a> {
        let mut operands = Operands { receiver: None, args: Vec::new(), value: None, stmt: Some(stmt) };
        if let Some(ExpressionKind::FunctionCall { callee, arguments, options, call: CallType::Call(kind) }) = expr.map(|e| &e.kind) {
            if let ExpressionKind::MemberAccess { object, .. } = &callee.kind {
                operands.receiver = Some(object);
            }
            operands.args = call_arguments(callee, arguments, *kind);
            operands.value = options.iter().find(|o| o.name == "value").map(|o| &o.value);
        }
        operands
    }

    fn expr(&self, arg: Arg) -> Option<&Expression> {
        match arg {
            Receiver => self.receiver,
            Value => self.value,
            At(i) => self.args.get(i).copied(),
            Absent => None,
        }
    }

    /// `absent_is_this`: an absent sender or owner is the running contract.
    fn operand(&self, arg: Arg, absent_is_this: bool) -> Operand {
        let Some(expr) = self.expr(arg) else {
            let text = match (arg, self.stmt) {
                (Receiver, Some(CfgStatement::ExternalCall { target, .. })) => Some(target.clone()),
                (Value, Some(CfgStatement::ExternalCall { options, .. })) => options.iter().find(|(n, _)| n == "value").map(|(_, v)| v.clone()),
                _ => None,
            };
            return Operand { text, this: arg == Absent && absent_is_this };
        };
        let this = matches!(&uncast(expr).kind, ExpressionKind::Identifier { name, .. } if name == "this");
        Operand { text: Some(expr_to_string(expr)), this }
    }

    fn text(&self, arg: Arg) -> Option<String> {
        self.operand(arg, false).text
    }

    fn flag(&self, index: usize) -> Option<bool> {
        match &self.args.get(index)?.kind {
            ExpressionKind::Literal { value, literal_type: LiteralType::Bool } => Some(value == "true"),
            _ => None,
        }
    }

    /// The asset, its candidates, the tag and the reason.
    fn asset(&self, found: &Match) -> (AssetKind, Vec<AssetKind>, Tag, Option<ValueReason>) {
        let currency = || (AssetKind::Unknown, vec![AssetKind::Native, AssetKind::Erc20], Tag::Ambiguous, Some(ValueReason::Currency));
        if let Some((candidates, reason)) = &found.undecided {
            return (AssetKind::Unknown, candidates.clone(), Tag::Ambiguous, Some(*reason));
        }
        match found.recognizer.asset {
            Asset::Is(kind) => (kind, Vec::new(), found.tag, None),
            Asset::Currency => currency(),
            Asset::ClaimsIf(index) => match self.flag(index) {
                Some(true) => (AssetKind::Erc6909, Vec::new(), found.tag, None),
                Some(false) => currency(),
                None => {
                    let candidates = vec![AssetKind::Native, AssetKind::Erc20, AssetKind::Erc6909];
                    (AssetKind::Unknown, candidates, Tag::Ambiguous, Some(ValueReason::ClaimsFlag))
                }
            },
        }
    }
}

/// Where a fact is: its function, context and the modifier whose code holds it.
struct Place<'a> {
    function: &'a str,
    in_context: &'a str,
    modifier: Option<&'a str>,
    span: SourceSpan,
}

fn balance_read(place: &Place, found: &Match, operands: &Operands, owner: Arg) -> BalanceRead {
    let (asset_kind, asset_candidates, tag, reason) = operands.asset(found);
    BalanceRead {
        id: String::new(),
        function: place.function.to_string(),
        in_context: place.in_context.to_string(),
        modifier: place.modifier.map(str::to_string),
        span: place.span,
        asset_kind,
        asset_candidates,
        token_expr: operands.text(found.recognizer.token),
        owner_expr: operands.text(owner),
        id_expr: operands.text(found.recognizer.id),
        via_library: found.via_library.clone(),
        tag,
        reason,
    }
}

/// What `func`'s reachable code moves and reads, as `context` runs it, in source order.
pub fn value_facts(project: &Project, context: &ContractDef, owner: &ContractDef, func: &FunctionDef, cfg: &CfgGraph) -> ValueFacts {
    let mut calls = HashMap::new();
    let mut balances = Vec::new();
    walk_function(Some(project), context, func, &mut |_| {}, &mut |modifier, e| match &e.kind {
        ExpressionKind::FunctionCall { .. } => drop(calls.insert(e.span, e)),
        ExpressionKind::MemberAccess { object, .. } if builtin(e) == Some(Builtin::Balance) => balances.push((modifier, e.span, object.as_ref())),
        _ => {}
    });
    let function = format!("{}::{}", owner.key(), func.key);
    let mut facts = ValueFacts::default();
    if let Some(&recognizer @ Recognizer { effect: Effect::Reads { owner: holder }, .. }) = RECOGNIZERS.iter().find(|r| matches!(r.callee, Balance)) {
        let found = Match { recognizer, tag: Tag::Extracted, via_library: None, undecided: None };
        for (modifier, span, object) in balances {
            let place = Place { function: &function, in_context: context.key(), modifier, span };
            let operands = Operands { receiver: Some(object), args: Vec::new(), value: None, stmt: None };
            facts.balance_reads.push(balance_read(&place, &found, &operands, holder));
        }
    }
    for n in reachable(cfg) {
        for stmt in &cfg[n].statements {
            let (span, modifier) = stmt.origin();
            let span = span.unwrap_or(func.span);
            let operands = Operands::of(stmt, calls.get(&span).copied());
            let place = Place { function: &function, in_context: context.key(), modifier, span };
            for found in matches(project, stmt, &operands) {
                match found.recognizer.effect {
                    Effect::Reads { owner } => facts.balance_reads.push(balance_read(&place, &found, &operands, owner)),
                    Effect::Moves { operation, from, to, amount } => {
                        let (asset_kind, asset_candidates, tag, reason) = operands.asset(&found);
                        let (from, to) = (operands.operand(from, true), operands.operand(to, false));
                        let direction = if to.this { Direction::In } else if from.this { Direction::Out } else { Direction::Other };
                        facts.transfers.push(ValueTransfer {
                            id: String::new(),
                            function: function.clone(),
                            in_context: context.key().to_string(),
                            modifier: modifier.map(str::to_string),
                            span,
                            asset_kind,
                            asset_candidates,
                            operation,
                            direction,
                            token_expr: operands.text(found.recognizer.token),
                            from_expr: from.text,
                            to_expr: to.text,
                            amount_expr: operands.text(amount),
                            id_expr: operands.text(found.recognizer.id),
                            call: call_text(stmt, &project.decl_table).unwrap_or_default(),
                            via_library: found.via_library,
                            tag,
                            reason,
                        });
                    }
                }
            }
        }
    }
    facts.transfers.sort_by(|a, b| (a.span, a.operation, a.asset_kind, &a.via_library).cmp(&(b.span, b.operation, b.asset_kind, &b.via_library)));
    facts.transfers.dedup();
    facts.balance_reads.sort_by(|a, b| (a.span, a.asset_kind, &a.via_library).cmp(&(b.span, b.asset_kind, &b.via_library)));
    facts.balance_reads.dedup();
    for (n, t) in facts.transfers.iter_mut().enumerate() {
        t.id = format!("{function}#{n}");
    }
    for (n, r) in facts.balance_reads.iter_mut().enumerate() {
        r.id = format!("{function}#{n}");
    }
    facts
}

/// Transfers grouped by direction, asset, token and counterparty, in that order.
pub fn value_flows(transfers: &[ValueTransfer]) -> Vec<ValueFlow> {
    type Key = (Direction, AssetKind, Option<String>, Option<String>);
    let mut flows: BTreeMap<Key, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    for t in transfers {
        let counterparty = if t.direction == Direction::In { &t.from_expr } else { &t.to_expr };
        let (functions, ids) = flows.entry((t.direction, t.asset_kind, t.token_expr.clone(), counterparty.clone())).or_default();
        functions.insert(t.function.clone());
        ids.insert(t.id.clone());
    }
    flows
        .into_iter()
        .map(|((direction, asset_kind, token_expr, counterparty_expr), (functions, ids))| ValueFlow {
            direction,
            asset_kind,
            token_expr,
            counterparty_expr,
            functions: functions.into_iter().collect(),
            transfers: ids.into_iter().collect(),
        })
        .collect()
}

impl Recognizer {
    /// Token, from or owner, to, amount, id.
    fn args(&self) -> [Arg; 5] {
        match self.effect {
            Effect::Moves { from, to, amount, .. } => [self.token, from, to, amount, self.id],
            Effect::Reads { owner } => [self.token, owner, Absent, Absent, self.id],
        }
    }

    fn with_args(self, [token, from, to, amount, id]: [Arg; 5]) -> Recognizer {
        let effect = match self.effect {
            Effect::Moves { operation, .. } => Effect::Moves { operation, from, to, amount },
            Effect::Reads { .. } => Effect::Reads { owner: from },
        };
        Recognizer { token, id, effect, ..self }
    }

    fn kind(&self) -> AssetKind {
        match self.asset {
            Asset::Is(kind) => kind,
            Asset::Currency | Asset::ClaimsIf(_) => AssetKind::Unknown,
        }
    }
}

/// Ether a call sends, then the token or library function it calls.
fn matches(project: &Project, stmt: &CfgStatement, operands: &Operands) -> Vec<Match> {
    let mut found = Vec::new();
    if let CfgStatement::ExternalCall { kind, options, .. } = stmt {
        let sends_value = options.iter().any(|(name, _)| name == "value");
        let native = RECOGNIZERS.iter().filter(|r| match r.callee {
            Kind(kinds) => kinds.contains(kind),
            WithValue(kinds) => sends_value && kinds.contains(kind),
            _ => false,
        });
        found.extend(native.map(|r| Match { recognizer: *r, tag: Tag::Extracted, via_library: None, undecided: None }));
    }
    found.extend(token(project, stmt, operands));
    found.extend(library(project, stmt));
    found
}

/// A call to a recognized library function, by the declaration it resolves to.
fn library(project: &Project, stmt: &CfgStatement) -> Option<Match> {
    let (contract_key, function_key) = match stmt {
        CfgStatement::InternalCall { kind: CallKind::InternalLibrary, declaration: Some((c, f)), .. } => (c.as_str(), f.as_str()),
        CfgStatement::ExternalCall { kind: CallKind::LibraryDelegatecall, resolved: Some(id), .. } => {
            let t = project.decl_table.function(*id)?;
            (t.contract.as_str(), t.function.as_str())
        }
        _ => return None,
    };
    let library = contract(project, contract_key).filter(|c| c.kind == ContractKind::Library)?;
    let function = library.functions.iter().find(|f| f.key == function_key)?;
    let recognizer = RECOGNIZERS.iter().find(|r| matches!(r.callee, Library(l, f) if l == library.name && f == function.name))?;
    let via_library = Some(format!("{}::{}", library.key(), function.key));
    Some(Match { recognizer: *recognizer, tag: Tag::TypeResolved, via_library, undecided: None })
}

/// An external call by selector, on the standard the declaring interface follows; when that does not decide, AMBIGUOUS between the rows that fit.
fn token(project: &Project, stmt: &CfgStatement, operands: &Operands) -> Option<Match> {
    let CfgStatement::ExternalCall { kind: CallKind::External | CallKind::ExternalSelf, resolved, function, .. } = stmt else {
        return None;
    };
    let rows = |keep: &dyn Fn(&str, &str) -> bool| -> Vec<&Recognizer> {
        RECOGNIZERS.iter().filter(|r| matches!(r.callee, Token(signature, selector) if keep(signature, selector))).collect()
    };
    let (rows, reason) = match resolved {
        Some(id) => {
            let (declared, selector) = selector_of(project, *id)?;
            let rows = rows(&|_, s| s == selector);
            let standards = standards(project, declared);
            let fitting: Vec<&Recognizer> = rows.iter().copied().filter(|r| standards.contains(&r.kind())).collect();
            match fitting.as_slice() {
                [only] => return Some(Match { recognizer: **only, tag: Tag::TypeResolved, via_library: None, undecided: None }),
                [] => (rows, ValueReason::NoStandard),
                _ => (fitting, ValueReason::SeveralStandards),
            }
        }
        None => (rows(&|signature, _| arity(signature, function) == Some(operands.args.len())), ValueReason::UnresolvedDeclaration),
    };
    let first = **rows.first()?;
    // a field the fitting rows read from different places is not decided
    let args = std::array::from_fn(|i| {
        let arg = first.args()[i];
        if rows.iter().all(|r| r.args()[i] == arg) { arg } else { Absent }
    });
    let mut candidates: Vec<AssetKind> = rows.iter().map(|r| r.kind()).collect();
    candidates.sort();
    candidates.dedup();
    Some(Match { recognizer: first.with_args(args), tag: Tag::Ambiguous, via_library: None, undecided: Some((candidates, reason)) })
}

/// The parameter count of `signature` when it names `function`.
fn arity(signature: &str, function: &str) -> Option<usize> {
    let params = signature.strip_prefix(function)?.strip_prefix('(')?.strip_suffix(')')?;
    Some(if params.is_empty() { 0 } else { params.split(',').count() })
}

/// The contract declaring a function or public state variable, and its selector.
fn selector_of(project: &Project, id: DeclId) -> Option<(&ContractDef, &str)> {
    let t = project.decl_table.lookup(id)?;
    let c = contract(project, &t.contract)?;
    let selector = match t.kind {
        DeclKind::Function => c.functions.iter().find(|f| f.key == t.function)?.selector.as_deref(),
        DeclKind::StateVariable => c.state_vars.iter().find(|v| v.name == t.name)?.selector.as_deref(),
        _ => None,
    };
    Some((c, selector?))
}

/// The token standards a contract follows, by the selectors it declares or inherits.
fn standards(project: &Project, c: &ContractDef) -> Vec<AssetKind> {
    let exposed: HashSet<&str> = project
        .linearization(c)
        .into_iter()
        .flat_map(|b| b.functions.iter().filter_map(|f| f.selector.as_deref()).chain(b.state_vars.iter().filter_map(|v| v.selector.as_deref())))
        .collect();
    STANDARDS.iter().filter(|(_, marks)| marks.iter().any(|(_, s)| exposed.contains(s))).map(|(kind, _)| *kind).collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::parse::solc_frontend::SolcFrontend;

    #[test]
    fn every_selector_in_the_table_is_the_one_solc_gives_its_signature() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/solc/value");
        let project = SolcFrontend.parse_project(&root).expect("parse fixture");
        let declared: HashMap<&str, &str> = project
            .contracts
            .iter()
            .flat_map(|c| c.functions.iter())
            .filter_map(|f| Some((f.signature.as_str(), f.selector.as_deref()?)))
            .collect();
        let tokens = RECOGNIZERS.iter().filter_map(|r| match r.callee {
            Token(signature, selector) => Some((signature, selector)),
            _ => None,
        });
        for (signature, selector) in tokens.chain(STANDARDS.iter().flat_map(|(_, marks)| marks.iter().copied())) {
            assert_eq!(declared.get(signature), Some(&selector), "{signature}");
        }
    }
}
