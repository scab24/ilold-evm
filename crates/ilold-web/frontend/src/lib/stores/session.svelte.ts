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
  ConnectionEvent,
  ForkOrigin,
} from '$lib/api/types';


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



subscribe('session_add_node', (msg: SessionAddNode) => {
  const next = new Map(scenarios);
  const existing = next.get(msg.scenario) ?? [];
  next.set(msg.scenario, [
    ...existing,
    { function: msg.function, access: msg.access, step_index: msg.step_index },
  ]);
  scenarios = next;
});

subscribe('session_remove_node', (msg: SessionRemoveNode) => {
  const existing = scenarios.get(msg.scenario);
  if (!existing || existing.length === 0) return;
  const next = new Map(scenarios);
  next.set(msg.scenario, existing.slice(0, -1));
  scenarios = next;
});

subscribe('session_clear', (msg: SessionClear) => {
  const next = new Map(scenarios);
  next.set(msg.scenario, []);
  scenarios = next;
});

subscribe('session_highlight', (msg: SessionHighlight) => {
  highlightedFunction = msg.function;
});


subscribe('scenario_created', (msg: ScenarioCreated) => {
  const next = new Map(scenarios);
  next.set(msg.name, []);
  scenarios = next;
});

subscribe('scenario_switched', (msg: ScenarioSwitched) => {
  activeScenario = msg.to;
});

subscribe('scenario_deleted', (msg: ScenarioDeleted) => {
  const next = new Map(scenarios);
  next.delete(msg.name);
  scenarios = next;
  if (forkOrigins.has(msg.name)) {
    const nextOrigins = new Map(forkOrigins);
    nextOrigins.delete(msg.name);
    forkOrigins = nextOrigins;
  }
});

subscribe('scenario_forked', (_msg: ScenarioForked) => {
  resync();
});

subscribe('scenario_store_reloaded', (_msg: ScenarioStoreReloaded) => {
  resync();
});

subscribe('connection', (event: ConnectionEvent) => {
  if (event.state === 'connected') {
    resync();
  }
});


let resyncGen = 0;

async function resync(): Promise<void> {
  const gen = ++resyncGen;
  try {
    const response = await getAllScenarios();
    if (gen !== resyncGen) return; // stale — a newer resync superseded this

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
