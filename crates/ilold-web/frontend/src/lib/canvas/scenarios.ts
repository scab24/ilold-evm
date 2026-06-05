import type { SessionStep, AccessLevel, ForkOrigin } from '$lib/api/types';


export interface ComposedNode {
  id: string;
  stepIndex: number;
  lane: number;
  function: string;
  access: AccessLevel;
  _sessionStep: true;
  _scenario: string;
  _scenariosPassingThrough: string[];
}

export interface ComposedEdge {
  id: string;
  source: string;
  target: string;
  _scenario: string;
  _forkEdge?: true;
}

export interface ComposedTree {
  nodes: ComposedNode[];
  edges: ComposedEdge[];
}


export function composeScenarioTree(
  scenarios: Map<string, SessionStep[]>,
  forkOrigins: Map<string, ForkOrigin>,
): ComposedTree {
  const nodes: ComposedNode[] = [];
  const edges: ComposedEdge[] = [];
  if (scenarios.size === 0) return { nodes, edges };

  const scenarioNames = Array.from(scenarios.keys());
  const laneIndex = new Map(scenarioNames.map((n, i) => [n, i]));

  for (const name of scenarioNames) {
    const steps = scenarios.get(name)!;
    const origin = forkOrigins.get(name);
    const renderFrom = origin?.at_step ?? 0;
    const lane = laneIndex.get(name)!;

    for (let i = renderFrom; i < steps.length; i++) {
      const step = steps[i];
      const id = nodeId(name, i);
      nodes.push({
        id,
        stepIndex: i,
        lane,
        function: step.function,
        access: step.access,
        _sessionStep: true,
        _scenario: name,
        _scenariosPassingThrough: [], // filled in the second pass below
      });
      if (i > renderFrom) {
        edges.push({
          id: `session-edge:${name}:${i - 1}→${i}`,
          source: nodeId(name, i - 1),
          target: id,
          _scenario: name,
        });
      }
    }

    if (origin && origin.at_step > 0 && steps.length > renderFrom) {
      const originSteps = scenarios.get(origin.scenario);
      if (originSteps && originSteps.length >= origin.at_step) {
        const sourceNodeId = resolveOriginNodeId(
          origin.scenario,
          origin.at_step - 1,
          forkOrigins,
        );
        if (sourceNodeId !== null) {
          edges.push({
            id: `session-fork:${origin.scenario}:${origin.at_step - 1}→${name}:${renderFrom}`,
            source: sourceNodeId,
            target: nodeId(name, renderFrom),
            _scenario: name,
            _forkEdge: true,
          });
        }
      }
    }
  }

  const passesThrough = new Map<string, Set<string>>();
  for (const n of nodes) passesThrough.set(n.id, new Set([n._scenario]));

  for (const name of scenarioNames) {
    const path = fullPath(name, scenarios, forkOrigins);
    for (const nid of path) {
      const set = passesThrough.get(nid);
      if (set) set.add(name);
    }
  }

  for (const n of nodes) {
    n._scenariosPassingThrough = Array.from(passesThrough.get(n.id) ?? [n._scenario]);
  }

  return { nodes, edges };
}


function nodeId(scenario: string, stepIndex: number): string {
  return `session:${scenario}:step:${stepIndex}`;
}

function resolveOriginNodeId(
  scenario: string,
  stepIndex: number,
  forkOrigins: Map<string, ForkOrigin>,
): string | null {
  let current: string | undefined = scenario;
  let guard = 0;
  while (current && guard++ < 64) {
    const origin = forkOrigins.get(current);
    if (!origin || stepIndex >= origin.at_step) {
      return nodeId(current, stepIndex);
    }
    current = origin.scenario;
  }
  return null;
}

function fullPath(
  name: string,
  scenarios: Map<string, SessionStep[]>,
  forkOrigins: Map<string, ForkOrigin>,
): string[] {
  const origin = forkOrigins.get(name);
  const ownStart = origin?.at_step ?? 0;
  const path: string[] = [];

  for (let i = 0; i < ownStart; i++) {
    const owner = resolveOriginNodeId(name, i, forkOrigins);
    if (owner !== null) path.push(owner);
  }
  const steps = scenarios.get(name);
  if (steps) {
    for (let i = ownStart; i < steps.length; i++) {
      path.push(nodeId(name, i));
    }
  }
  return path;
}
