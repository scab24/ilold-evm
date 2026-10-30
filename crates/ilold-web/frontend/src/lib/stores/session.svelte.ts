import { subscribe } from '$lib/api/ws';
import { getAllScenarios } from '$lib/api/session';
import type {
  SessionStep,
  SessionAddNode,
  SessionRemoveNode,
  SessionClear,
  SessionHighlight,
  ScenarioCreated,
  ScenarioSwitched,
  ScenarioDeleted,
  ScenarioForked,
  ScenarioStoreReloaded,
  SessionResync,
  ConnectionEvent,
  ForkOrigin,
} from '$lib/api/types';


// Contract whose session this page shows; events for other contracts are ignored.
let sessionContract = $state<string | null>(null);
let scenarios = $state<Map<string, SessionStep[]>>(new Map([['main', []]]));
let activeScenario = $state<string>('main');
let highlightedFunction = $state<string | null>(null);
let forkOrigins = $state<Map<string, ForkOrigin>>(new Map());

function resetState() {
  scenarios = new Map([['main', []]]);
  activeScenario = 'main';
  highlightedFunction = null;
  forkOrigins = new Map();
}

function isOwn(msg: { contract: string }): boolean {
  return sessionContract !== null && msg.contract === sessionContract;
}

/** Bind the store to `contract` and load its session from the server. */
export function setSessionContract(contract: string): void {
  if (contract === sessionContract) return;
  sessionContract = contract;
  resetState();
  resync();
}


subscribe('session_add_node', (msg: SessionAddNode) => {
  if (!isOwn(msg)) return;
  const next = new Map(scenarios);
  const existing = next.get(msg.scenario) ?? [];
  next.set(msg.scenario, [
    ...existing,
    { function: msg.function, access: msg.access, step_index: msg.step_index },
  ]);
  scenarios = next;
});

subscribe('session_remove_node', (msg: SessionRemoveNode) => {
  if (!isOwn(msg)) return;
  const existing = scenarios.get(msg.scenario);
  if (!existing || existing.length === 0) return;
  const next = new Map(scenarios);
  next.set(msg.scenario, existing.slice(0, -1));
  scenarios = next;
});

subscribe('session_clear', (msg: SessionClear) => {
  if (!isOwn(msg)) return;
  const next = new Map(scenarios);
  next.set(msg.scenario, []);
  scenarios = next;
});

subscribe('session_highlight', (msg: SessionHighlight) => {
  if (!isOwn(msg)) return;
  highlightedFunction = msg.function;
});


subscribe('scenario_created', (msg: ScenarioCreated) => {
  if (!isOwn(msg)) return;
  const next = new Map(scenarios);
  next.set(msg.name, []);
  scenarios = next;
});

subscribe('scenario_switched', (msg: ScenarioSwitched) => {
  if (!isOwn(msg)) return;
  activeScenario = msg.to;
});

subscribe('scenario_deleted', (msg: ScenarioDeleted) => {
  if (!isOwn(msg)) return;
  const next = new Map(scenarios);
  next.delete(msg.name);
  scenarios = next;
  if (forkOrigins.has(msg.name)) {
    const nextOrigins = new Map(forkOrigins);
    nextOrigins.delete(msg.name);
    forkOrigins = nextOrigins;
  }
});

subscribe('scenario_forked', (msg: ScenarioForked) => {
  if (isOwn(msg)) resync();
});

subscribe('scenario_store_reloaded', (msg: ScenarioStoreReloaded) => {
  if (isOwn(msg)) resync();
});

// The server dropped patches for this client (broadcast lag): refetch everything.
subscribe('session_resync', (_msg: SessionResync) => {
  resync();
});

subscribe('connection', (event: ConnectionEvent) => {
  if (event.state === 'connected') {
    resync();
  }
});


let resyncGen = 0;

async function resync(): Promise<void> {
  const contract = sessionContract;
  if (contract === null) return;
  const gen = ++resyncGen;
  try {
    const response = await getAllScenarios(contract);
    if (gen !== resyncGen || contract !== sessionContract) return; // stale — superseded

    const newMap = new Map<string, SessionStep[]>();
    const newOrigins = new Map<string, ForkOrigin>();
    for (const snapshot of response.scenarios) {
      newMap.set(
        snapshot.name,
        snapshot.steps.map((s) => ({
          function: s.function,
          access: s.access,
          step_index: s.step_index,
        })),
      );
      if (snapshot.forked_from) {
        newOrigins.set(snapshot.name, {
          scenario: snapshot.forked_from.scenario,
          at_step: snapshot.forked_from.at_step,
        });
      }
    }
    if (newMap.size === 0) newMap.set('main', []);

    scenarios = newMap;
    forkOrigins = newOrigins;
    activeScenario = response.active || 'main';
    highlightedFunction = null;
  } catch (e) {
    if (gen === resyncGen) console.warn('Session re-sync failed:', e);
  }
}


export function getScenarios(): Map<string, SessionStep[]> {
  return scenarios;
}

export function getActiveScenario(): string {
  return activeScenario;
}

export function getForkOrigins(): Map<string, ForkOrigin> {
  return forkOrigins;
}

export function getScenarioSteps(name: string): SessionStep[] {
  return scenarios.get(name) ?? [];
}

export function getSteps(): SessionStep[] {
  return getScenarioSteps(activeScenario);
}

export function getHighlightedFunction(): string | null {
  return highlightedFunction;
}

export function clearSession(): void {
  resetState();
}
