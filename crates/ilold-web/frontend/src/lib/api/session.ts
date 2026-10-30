
import type { ScenarioInfo, AllScenariosResponse } from './types';

const BASE = '';


export interface CommandRequest {
  contract?: string;
  command: SessionCommand;
}

export type ScenarioAction =
  | { New: { name: string } }
  | 'List'
  | { Switch: { name: string } }
  | { Fork: { name: string; at_step?: number } }
  | { Delete: { name: string } };

export type SessionCommand =
  | { Call: { func: string } }
  | 'Back'
  | 'Clear'
  | 'State'
  | 'Functions'
  | 'FunctionsAll'
  | 'StateVarsAll'
  | { Who: { variable: string } }
  | 'Session'
  | 'Export'
  | 'SaveSession'
  | { Scenario: { sub: ScenarioAction } };

/** A command the server rejected with `CommandResult::Error` (HTTP 200). */
export class CommandError extends Error {}

/** `?contract=` selector for session-scoped reads; without it the server uses
 *  the contract of the most recent command. */
function contractQuery(contract: string | undefined): string {
  return contract ? `?contract=${encodeURIComponent(contract)}` : '';
}

export async function postCommand(command: SessionCommand, contract?: string) {
  const body: CommandRequest = { command };
  if (contract) body.contract = contract;

  const res = await fetch(`${BASE}/api/cmd`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `Command failed: ${res.status}`);
  }
  const result = await res.json();
  if (result && typeof result === 'object' && 'Error' in result) {
    throw new CommandError(result.Error?.message ?? 'Command failed');
  }
  return result;
}


export async function getSessionState(contract?: string) {
  const res = await fetch(`${BASE}/api/session/state${contractQuery(contract)}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function getSessionSequence(contract?: string) {
  const res = await fetch(`${BASE}/api/session/sequence${contractQuery(contract)}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function getStepNarrative(index: number, contract?: string) {
  const res = await fetch(`${BASE}/api/session/step/${index}/narrative${contractQuery(contract)}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function getStepTrace(index: number, contract?: string) {
  const res = await fetch(`${BASE}/api/session/step/${index}/trace${contractQuery(contract)}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function getVariableTimeline(variable: string, contract?: string) {
  const res = await fetch(`${BASE}/api/session/timeline/${variable}${contractQuery(contract)}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export interface SliceParams {
  func: string;
  variable: string;
  direction?: 'backward' | 'forward' | 'both';
  contract?: string;
}

export async function getSlice(params: SliceParams) {
  let url = `${BASE}/api/session/slice/${params.func}/${params.variable}${contractQuery(params.contract)}`;
  if (params.direction) url += `${params.contract ? '&' : '?'}direction=${params.direction}`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export interface TraceParams {
  contract: string;
  func: string;
  depth?: number;
  reverts?: boolean;
  expand?: number[];
}

export async function getFlowTrace(params: TraceParams) {
  let url = `${BASE}/api/session/trace/${params.contract}/${params.func}`;
  const qs: string[] = [];
  if (params.depth != null) qs.push(`depth=${params.depth}`);
  if (params.reverts) qs.push('reverts=true');
  if (params.expand?.length) qs.push(`expand=${params.expand.join(',')}`);
  if (qs.length) url += `?${qs.join('&')}`;
  const res = await fetch(url);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function getFunctionNarrative(contract: string, func: string) {
  const res = await fetch(`${BASE}/api/session/function/${contract}/${func}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}


export async function getScenarios(contract?: string): Promise<ScenarioInfo[]> {
  const res = await fetch(`${BASE}/api/scenarios${contractQuery(contract)}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function getAllScenarios(contract?: string): Promise<AllScenariosResponse> {
  const res = await fetch(`${BASE}/api/scenarios/all${contractQuery(contract)}`);
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}
