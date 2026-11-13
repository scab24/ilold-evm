import { getConnections, getContract, getProject, getProjectConnections, getProjectDepgraph, getValueFlow, type Connections, type ContractDetail, type ProjectConnections, type ProjectDepGraph, type ProjectSummary, type ValueFlowPairs } from '$lib/api/rest';

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

function perName<T>(load: (name: string) => Promise<T>): (name: string) => Promise<T> {
  const loaders = new Map<string, () => Promise<T>>();
  return (name) => {
    let loader = loaders.get(name);
    if (!loader) loaders.set(name, (loader = once(() => load(name))));
    return loader();
  };
}

export const contractConnections: (name: string) => Promise<Connections> = perName(getConnections);
export const contractDetail: (name: string) => Promise<ContractDetail> = perName(getContract);
