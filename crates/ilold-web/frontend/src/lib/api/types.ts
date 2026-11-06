import type { StorageAccess } from './rest';
export type AccessLevel =
  | "Public"
  | "Internal"
  | { Restricted: { role: string } }
  | { Special: { kind: string } };


export interface SessionAddNode {
  type: "session_add_node";
  contract: string;
  scenario: string;
  function: string;
  access: AccessLevel;
  step_index: number;
}

export interface SessionRemoveNode {
  type: "session_remove_node";
  contract: string;
  scenario: string;
}

export interface SessionClear {
  type: "session_clear";
  contract: string;
  scenario: string;
}

export interface SessionHighlight {
  type: "session_highlight";
  contract: string;
  scenario: string;
  function: string;
}


export interface ScenarioCreated {
  type: "scenario_created";
  contract: string;
  name: string;
}

export interface ScenarioSwitched {
  type: "scenario_switched";
  contract: string;
  from: string;
  to: string;
}

export interface ScenarioDeleted {
  type: "scenario_deleted";
  contract: string;
  name: string;
}

export interface ScenarioForked {
  type: "scenario_forked";
  contract: string;
  from: string;
  to: string;
  at_step: number;
}

export interface ScenarioStoreReloaded {
  type: "scenario_store_reloaded";
  contract: string;
  active: string;
}


export interface SessionResync {
  type: "session_resync";
}

export interface ScenarioInfo {
  name: string;
  active: boolean;
  step_count: number;
}

export interface SessionStepView {
  function: string;
  access: AccessLevel;
  step_index: number;
}

export interface ForkOrigin {
  scenario: string;
  at_step: number;
}

export interface ScenarioSnapshot {
  name: string;
  steps: SessionStepView[];
  forked_from: ForkOrigin | null;
}

export interface AllScenariosResponse {
  contract: string;
  active: string;
  scenarios: ScenarioSnapshot[];
}

export interface SearchResult {
  type: "search_result";
  contract: string;
  function: string;
  /** Addresses the function on `contract`, overloads included. */
  key: string;
  path_id: number;
  terminal: string;
  matches: Array<{ field: string; value: string }>;
  depth: number;
}

export interface SearchComplete {
  type: "search_complete";
  /** Only the first matches are streamed when `truncated`. */
  total: number;
  truncated: boolean;
}

export interface SearchError {
  type: "error";
  message: string;
  disconnected?: boolean;
}


export type ServerMessage =
  | SessionAddNode
  | SessionRemoveNode
  | SessionClear
  | SessionHighlight
  | ScenarioCreated
  | ScenarioSwitched
  | ScenarioDeleted
  | ScenarioForked
  | ScenarioStoreReloaded
  | SessionResync
  | SearchResult
  | SearchComplete
  | SearchError;


export interface ConnectionEvent {
  state: "connected" | "disconnected";
}

export type ConnectionState = "connected" | "connecting" | "disconnected";


export interface TopicMap {
  search_result: SearchResult;
  search_complete: SearchComplete;
  error: SearchError;
  session_add_node: SessionAddNode;
  session_remove_node: SessionRemoveNode;
  session_clear: SessionClear;
  session_highlight: SessionHighlight;
  scenario_created: ScenarioCreated;
  scenario_switched: ScenarioSwitched;
  scenario_deleted: ScenarioDeleted;
  scenario_forked: ScenarioForked;
  scenario_store_reloaded: ScenarioStoreReloaded;
  session_resync: SessionResync;
  connection: ConnectionEvent;
}


export interface SessionStep {
  function: string;
  access: AccessLevel;
  step_index: number;
}


export interface SessionViewPayload {
  contract: string;
  steps: string[];
  findings_count: number;
}

export interface SessionViewResponse {
  SessionView: SessionViewPayload;
}


export interface SearchNavigatePayload {
  contract: string;
  func: string;
  pathId: number;
}


export interface SearchCallbacks {
  onResult: (result: SearchResult) => void;
  onComplete: (total: number) => void;
  onError: (message: string) => void;
}

export interface SearchOptions {
  contract?: string;
  function?: string;
}

export interface SessionEventCallbacks {
  onAddNode?: (msg: SessionAddNode) => void;
  onRemoveNode?: (msg: SessionRemoveNode) => void;
  onClear?: (msg: SessionClear) => void;
  onHighlight?: (msg: SessionHighlight) => void;
}

export type GuardKind =
  | "CallerEquals"
  | "RoleCheck"
  | "TxOrigin"
  | "AccessByName"
  | "Pause"
  | "Reentrancy"
  | "Initializer"
  /** Only inside a compound check. */
  | "Condition";

/** Where a value comes from: a call's receiver or what a guard compares the caller with. */
export type ValueSource =
  | { StateVar: { name: string; key: string | null } }
  | { Immutable: { name: string; key: string | null } }
  | { Constant: { name: string; key: string | null } }
  | { Param: { index: number; name: string } }
  | { Literal: { text: string } }
  | 'MsgSender'
  | 'TxOrigin'
  | { Expression: { text: string } };

/** A conditional guard holds on some paths only and never restricts the caller. */
export type GuardScope = 'Unconditional' | 'Conditional';

/** `via` is empty for the body. */
export interface GuardFact {
  kind: GuardKind;
  condition: string;
  via: string[];
  span: { file_index: number; start_line: number; start_col: number; end_line: number; end_col: number } | null;
  /** The modifier has no code to read. */
  heuristic: boolean;
  /** Absent in older snapshots, which only had unconditional guards. */
  scope?: GuardScope;
  /** The tested call or comparison must be false to pass: a deny-list, which never restricts. */
  negated?: boolean;
  /** Every way to pass needs a caller test that must hold; absent in older snapshots. */
  requires_caller?: boolean;
  /** Empty on a compound guard: its parts carry them. */
  compared_against?: ValueSource;
  checked_by?: string;
  role?: ValueSource;
  role_implied_by?: string;
  account?: ValueSource;
  alternatives?: GuardTerm[];
  conjuncts?: GuardTerm[];
  /** Not split into terms: part of the condition tests the caller in a way the analysis cannot read. */
  ambiguous?: boolean;
}

/** A test inside a guard's condition; a compound one lists its parts. */
export interface GuardTerm {
  kind: GuardKind;
  condition: string;
  negated?: boolean;
  compared_against?: ValueSource;
  checked_by?: string;
  role?: ValueSource;
  role_implied_by?: string;
  account?: ValueSource;
  alternatives?: GuardTerm[];
  conjuncts?: GuardTerm[];
}

export interface TransitiveEffect {
  via: string[];
  item: string;
  origin_contract: string;
}

export interface FunctionNarrative {
  contract: string;
  name: string;
  access: AccessLevel;
  total_paths: number;
  happy_paths: number;
  revert_paths: number;
  observations: { kind: string; description: string }[];
  state_writes: string[];
  state_reads: string[];
  external_calls: string[];
  internal_calls?: string[];
  modifiers: string[];
  events?: string[];
  transitive_state_writes?: TransitiveEffect[];
  transitive_state_reads?: TransitiveEffect[];
  transitive_external_calls?: TransitiveEffect[];
  transitive_events?: TransitiveEffect[];
  guards?: GuardFact[];
  /** Reads and writes of its own code, with their keys; absent when there are none. */
  accesses?: StorageAccess[];
}
