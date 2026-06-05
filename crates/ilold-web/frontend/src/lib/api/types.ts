export type AccessLevel =
  | "Public"
  | "Internal"
  | { Restricted: { role: string } }
  | { Special: { kind: string } };


export interface SessionAddNode {
  type: "session_add_node";
  scenario: string;
  function: string;
  access: AccessLevel;
  step_index: number;
}

export interface SessionRemoveNode {
  type: "session_remove_node";
  scenario: string;
}

export interface SessionClear {
  type: "session_clear";
  scenario: string;
}

export interface SessionHighlight {
  type: "session_highlight";
  scenario: string;
  function: string;
}


export interface ScenarioCreated {
  type: "scenario_created";
  name: string;
}

export interface ScenarioSwitched {
  type: "scenario_switched";
  from: string;
  to: string;
}

export interface ScenarioDeleted {
  type: "scenario_deleted";
  name: string;
}

export interface ScenarioForked {
  type: "scenario_forked";
  from: string;
  to: string;
  at_step: number;
}

export interface ScenarioStoreReloaded {
  type: "scenario_store_reloaded";
  active: string;
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
  active: string;
  scenarios: ScenarioSnapshot[];
}

export interface SearchResult {
  type: "search_result";
  contract: string;
  function: string;
  path_id: number;
  terminal: string;
  matches: Array<{ field: string; value: string }>;
  depth: number;
}

export interface SearchComplete {
  type: "search_complete";
  total: number;
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
