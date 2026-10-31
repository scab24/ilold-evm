
import type { ScenarioInfo, AllScenariosResponse, FunctionNarrative } from './types';
import { BASE, body, segment } from './rest';


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

// without `?contract=` the server reads the contract of the most recent command
export function contractQuery(contract: string | undefined): string {
  return contract ? `?contract=${encodeURIComponent(contract)}` : '';
}

export async function postCommand(command: SessionCommand, contract?: string) {
  const request: CommandRequest = { command };
  if (contract) request.contract = contract;

  const res = await fetch(`${BASE}/api/cmd`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  const result = await body<any>(res, 'Command failed');
  if (result && typeof result === 'object' && 'Error' in result) {
    throw new CommandError(result.Error?.message ?? 'Command failed');
  }
  return result;
}


export async function getSessionState(contract?: string) {
  return body<any>(await fetch(`${BASE}/api/session/state${contractQuery(contract)}`), 'Session state unavailable');
}

export async function getSessionSequence(contract?: string) {
  return body<any>(await fetch(`${BASE}/api/session/sequence${contractQuery(contract)}`), 'Session sequence unavailable');
}

export async function getStepNarrative(index: number, contract?: string): Promise<FunctionNarrative> {
  return body(await fetch(`${BASE}/api/session/step/${index}/narrative${contractQuery(contract)}`), `Step ${index} unavailable`);
}

export async function getStepTrace(index: number, contract?: string) {
  return body<any>(await fetch(`${BASE}/api/session/step/${index}/trace${contractQuery(contract)}`), `Step ${index} unavailable`);
}

export async function getVariableTimeline(variable: string, contract?: string) {
  const res = await fetch(`${BASE}/api/session/timeline/${segment(variable)}${contractQuery(contract)}`);
  return body<any>(res, `No timeline for ${variable}`);
}

export interface SliceParams {
  func: string;
  variable: string;
  direction?: 'backward' | 'forward' | 'both';
  contract?: string;
}

export async function getSlice(params: SliceParams) {
  let url = `${BASE}/api/session/slice/${segment(params.func)}/${segment(params.variable)}${contractQuery(params.contract)}`;
  if (params.direction) url += `${params.contract ? '&' : '?'}direction=${params.direction}`;
  return body<any>(await fetch(url), `No slice for ${params.func}`);
}

export interface TraceParams {
  contract: string;
  func: string;
  depth?: number;
  reverts?: boolean;
  expand?: number[];
}

export async function getFlowTrace(params: TraceParams) {
  let url = `${BASE}/api/session/trace/${segment(params.contract)}/${segment(params.func)}`;
  const qs: string[] = [];
  if (params.depth != null) qs.push(`depth=${params.depth}`);
  if (params.reverts) qs.push('reverts=true');
  if (params.expand?.length) qs.push(`expand=${params.expand.join(',')}`);
  if (qs.length) url += `?${qs.join('&')}`;
  return body<any>(await fetch(url), `No trace for ${params.func}`);
}

export async function getFunctionNarrative(contract: string, func: string) {
  const res = await fetch(`${BASE}/api/session/function/${segment(contract)}/${segment(func)}`);
  return body<any>(res, `No narrative for ${func}`);
}


export async function getScenarios(contract?: string): Promise<ScenarioInfo[]> {
  return body(await fetch(`${BASE}/api/scenarios${contractQuery(contract)}`), 'Scenarios unavailable');
}

export async function getAllScenarios(contract?: string): Promise<AllScenariosResponse> {
  return body(await fetch(`${BASE}/api/scenarios/all${contractQuery(contract)}`), 'Scenarios unavailable');
}
