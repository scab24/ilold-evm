import { getProject, getProjectConnections, getProjectDepgraph, getValueFlow, type ProjectConnections, type ProjectDepGraph, type ProjectSummary, type ValueFlowPairs } from '$lib/api/rest';

function once<T>(load: () => Promise<T>): () => Promise<T> {
  let pending: Promise<T> | null = null;
  return () => {
    pending ??= load().catch((e) => {
      pending = null;
      throw e;
    });
    return pending;
  };
}

export const projectSummary: () => Promise<ProjectSummary> = once(getProject);
export const projectConnections: () => Promise<ProjectConnections> = once(getProjectConnections);
export const projectDepgraph: () => Promise<ProjectDepGraph> = once(getProjectDepgraph);
export const valueFlow: () => Promise<ValueFlowPairs> = once(getValueFlow);
