import type { AccessLevel, GuardFact } from './types';

export interface ProjectSummary {
  files: number;
  contracts: ContractSummary[];
  dependency_contracts: number;
  hidden_contracts: number;
  warnings: string[];
}

export interface ContractIdentity {
  /** The bare name when unique, otherwise the qualified id. */
  name: string;
  display_name: string;
  qualified_id: string;
  is_dependency: boolean;
}

interface Bases {
  inherits: string[];
  inherit_keys: string[];
}

export interface ContractSummary extends ContractIdentity, Bases {
  kind: string;
  functions: number;
  state_vars: number;
}

export interface ContractDetail extends ContractIdentity, Bases {
  kind: string;
  functions: FunctionSummary[];
  state_vars: StateVarSummary[];
  inherited_functions?: FunctionSummary[];
  inherited_state_vars?: StateVarSummary[];
}

export interface FunctionSummary {
  name: string;
  /** The name, or the signature when overloaded. */
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
  access: AccessLevel;
  guards: GuardFact[];
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

export interface SourceSpan {
  file_index: number;
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
}

export interface CytoscapeNodeData {
  id: string;
  label: string;
  node_type: string;
  contract: string;
  is_external: boolean;
  statements?: string[];
  span?: SourceSpan;
  file?: string;
}

export interface CytoscapeEdgeData {
  id: string;
  source: string;
  target: string;
  kind: string;
  call_count: number;
  kind_label?: string;
  /** Declaration named in source when virtual dispatch moved the target. */
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

export interface MapContract extends Bases {
  name: string;
  kind: string;
  folder: string;
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

export const BASE = '';  // same origin in production, proxied in dev

export async function body<T>(res: Response, fallback: string): Promise<T> {
  if (res.ok) return res.json();
  const text = await res.text();
  let message = text;
  try {
    const err = JSON.parse(text);
    if (err?.error) message = err.candidates ? `${err.error}: ${err.candidates.join(', ')}` : err.error;
  } catch {}
  throw new Error(message || `${fallback} (HTTP ${res.status})`);
}

// browsers resolve `.` and `..` segments away, even percent-encoded
export function segment(value: string): string {
  if (value === '.' || value === '..') throw new Error(`'${value}' is not a valid path segment`);
  return encodeURIComponent(value);
}

export async function getProjectMap(): Promise<ProjectMap> {
  return body(await fetch(`${BASE}/api/project/map`), 'Project map unavailable');
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
  return body(await fetch(`${BASE}/api/project/depgraph`), 'Dependency graph unavailable');
}

export async function getProject(): Promise<ProjectSummary> {
  return body(await fetch(`${BASE}/api/project`), 'Project unavailable');
}

export async function getContract(name: string): Promise<ContractDetail> {
  return body(await fetch(`${BASE}/api/contract/${segment(name)}`), `Contract ${name} not found`);
}

export async function getCallGraph(contractName: string): Promise<CytoscapeGraph> {
  const res = await fetch(`${BASE}/api/contract/${segment(contractName)}/callgraph`);
  return body(res, `No call graph for ${contractName}`);
}

export async function getCfg(contractName: string, funcName: string): Promise<CytoscapeGraph> {
  const res = await fetch(`${BASE}/api/contract/${segment(contractName)}/${segment(funcName)}/cfg`);
  return body(res, `No CFG for ${funcName}`);
}

export async function getPaths(contractName: string, funcName: string, offset = 0, limit?: number) {
  const params = new URLSearchParams({ offset: String(offset) });
  if (limit !== undefined) params.set('limit', String(limit));
  const res = await fetch(`${BASE}/api/contract/${segment(contractName)}/${segment(funcName)}/paths?${params}`);
  return body<any>(res, `No paths for ${funcName}`);
}

export interface FunctionSourceResponse {
  file_path: string;
  source: string;
  span: SourceSpan;
}

export async function getFunctionSource(
  contractName: string,
  funcName: string,
): Promise<FunctionSourceResponse> {
  const res = await fetch(`${BASE}/api/contract/${segment(contractName)}/${segment(funcName)}/source`);
  return body(res, `Source for ${funcName} not found`);
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
  const res = await fetch(`${BASE}/api/contract/${segment(contractName)}/analysis`);
  return body(res, `No sequence analysis for ${contractName}`);
}

export async function getSearchSuggestions(contractName: string): Promise<SearchSuggestions> {
  const res = await fetch(`${BASE}/api/contract/${segment(contractName)}/suggestions`);
  return body(res, `No suggestions for ${contractName}`);
}

/** `limit: 0` fetches only the functions. */
export async function getSequences(
  contractName: string,
  options: { depth?: number; offset?: number; limit?: number } = {},
) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(options)) {
    if (value !== undefined) params.set(key, String(value));
  }
  const query = params.toString();
  const url = `${BASE}/api/contract/${segment(contractName)}/sequences${query ? `?${query}` : ''}`;
  return body<any>(await fetch(url), `No sequences for ${contractName}`);
}
