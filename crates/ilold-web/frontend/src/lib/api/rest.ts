
export interface ProjectSummary {
  files: number;
  contracts: ContractSummary[];
  /** Dependency contracts in the model; listed only with `?include_dependencies=true`. */
  dependency_contracts: number;
  /** Non-fatal compilation problems, e.g. test files left out of the model. */
  warnings: string[];
}

/** Identity shared by contract listings and details. */
export interface ContractIdentity {
  /** Route segment: the bare name when unique, otherwise the qualified id. */
  name: string;
  /** Solidity name as declared. */
  display_name: string;
  /** Stable `<source path>:<Name>` id. */
  qualified_id: string;
  /** Declared outside the project sources (libraries, vendored code). */
  is_dependency: boolean;
}

export interface ContractSummary extends ContractIdentity {
  kind: string;
  functions: number;
  state_vars: number;
  inherits: string[];
}

export interface ContractDetail extends ContractIdentity {
  kind: string;
  inherits: string[];
  functions: FunctionSummary[];
  state_vars: StateVarSummary[];
  inherited_functions?: FunctionSummary[];
  inherited_state_vars?: StateVarSummary[];
}

export interface FunctionSummary {
  name: string;
  /** Route segment for /api/contract/{c}/{key}/...: the name, or the signature when overloaded. */
  key: string;
  signature: string;
  kind: string;
  visibility: string;
  mutability: string;
  params: { name: string; type_name: string }[];
  path_count: number;
  happy_paths: number;
  revert_paths: number;
  modifiers: string[];
}

export interface StateVarSummary {
  name: string;
  type_name: string;
  visibility: string;
  is_constant: boolean;
  is_immutable: boolean;
}

export interface CytoscapeGraph {
  nodes: { data: CytoscapeNodeData }[];
  edges: { data: CytoscapeEdgeData }[];
}

export interface CytoscapeNodeData {
  id: string;
  label: string;
  node_type: string;
  contract: string;
  is_external: boolean;
  statements?: string[];
}

export interface CytoscapeEdgeData {
  id: string;
  source: string;
  target: string;
  kind: string;
  call_count: number;
  /** Call-graph edges only: human label of the call kind (e.g. "library delegatecall"). */
  kind_label?: string;
  /** Call-graph edges only: declaration named in source when virtual dispatch moved the target. */
  static_target?: string;
}


export interface SearchSuggestions {
  functions: string[];
  state_vars: string[];
  events: string[];
  external_calls: string[];
  categories: { label: string; items: string[] }[];
}

export interface ProjectMap {
  contracts: MapContract[];
  relationships: MapRelationship[];
}

export interface MapContract {
  name: string;
  kind: string;
  folder: string;
  inherits: string[];
  functions: MapFunction[];
  state_vars: { name: string; type_name: string; is_constant: boolean }[];
}

export interface MapFunction {
  name: string;
  visibility: string;
  mutability: string;
  path_count: number;
  happy_paths: number;
  revert_paths: number;
  has_external_calls: boolean;
}

export interface MapRelationship {
  from_contract: string;
  from_function: string;
  to_contract: string;
  to_function: string;
  kind: string;
}

const BASE = '';  // same origin in production, proxied in dev

export async function getProjectMap(): Promise<ProjectMap> {
  const res = await fetch(`${BASE}/api/project/map`);
  return res.json();
}

export interface DepGraphNode {
  data: {
    id: string;
    label: string;
    kind: string;
    folder: string;
    layer: number;
    focus: boolean;
    qualified_id: string;
    is_dependency: boolean;
  };
}

export interface DepGraphEdge {
  data: {
    id: string;
    source: string;
    target: string;
    kind: string;
    kinds: string[];
    call_count: number;
  };
}

export interface ProjectDepGraph {
  nodes: DepGraphNode[];
  edges: DepGraphEdge[];
}

export async function getProjectDepgraph(): Promise<ProjectDepGraph> {
  const res = await fetch(`${BASE}/api/project/depgraph`);
  return res.json();
}

export async function getProject(): Promise<ProjectSummary> {
  const res = await fetch(`${BASE}/api/project`);
  return res.json();
}

export async function getContract(name: string): Promise<ContractDetail> {
  const res = await fetch(`${BASE}/api/contract/${encodeURIComponent(name)}`);
  if (!res.ok) throw new Error(`Contract ${name} not found`);
  return res.json();
}

export async function getCallGraph(contractName: string): Promise<CytoscapeGraph> {
  const res = await fetch(`${BASE}/api/contract/${encodeURIComponent(contractName)}/callgraph`);
  return res.json();
}

export async function getCfg(contractName: string, funcName: string): Promise<CytoscapeGraph> {
  const res = await fetch(`${BASE}/api/contract/${encodeURIComponent(contractName)}/${funcName}/cfg`);
  return res.json();
}

export async function getPaths(contractName: string, funcName: string) {
  const res = await fetch(`${BASE}/api/contract/${encodeURIComponent(contractName)}/${funcName}/paths`);
  return res.json();
}

export interface FunctionSourceResponse {
  file_path: string;
  source: string;
  span: {
    file_index: number;
    start_line: number;
    start_col: number;
    end_line: number;
    end_col: number;
  };
}

export async function getFunctionSource(
  contractName: string,
  funcName: string,
): Promise<FunctionSourceResponse> {
  const res = await fetch(`${BASE}/api/contract/${encodeURIComponent(contractName)}/${funcName}/source`);
  if (!res.ok) throw new Error(`Source for ${funcName} not found`);
  return res.json();
}

export interface SequenceAnalysis {
  functions: {
    name: string;
    preconditions: string[];
    state_writes: string[];
    state_reads: string[];
    external_calls: string[];
    events: string[];
    can_revert: boolean;
    always_reverts: boolean;
    read_only: boolean;
  }[];
  transitions: {
    from: string;
    to: string;
    shared_state: string[];
    conditions_affected: string[];
    has_external_in_from: boolean;
    has_external_in_to: boolean;
  }[];
}

export async function getSequenceAnalysis(contractName: string): Promise<SequenceAnalysis> {
  const res = await fetch(`${BASE}/api/contract/${encodeURIComponent(contractName)}/analysis`);
  return res.json();
}

export async function getSearchSuggestions(contractName: string): Promise<SearchSuggestions> {
  const res = await fetch(`${BASE}/api/contract/${encodeURIComponent(contractName)}/suggestions`);
  return res.json();
}

export async function getSequences(contractName: string, depth?: number) {
  const url = depth
    ? `${BASE}/api/contract/${encodeURIComponent(contractName)}/sequences?depth=${depth}`
    : `${BASE}/api/contract/${encodeURIComponent(contractName)}/sequences`;
  const res = await fetch(url);
  return res.json();
}
