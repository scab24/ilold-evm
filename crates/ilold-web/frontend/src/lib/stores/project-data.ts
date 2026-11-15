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

interface PerName<T> {
  (name: string): Promise<T>;
  peek(name: string): T | undefined;
}

function perName<T>(load: (name: string) => Promise<T>): PerName<T> {
  const loaders = new Map<string, () => Promise<T>>();
  const loaded = new Map<string, T>();
  const get = (name: string) => {
    let loader = loaders.get(name);
    if (!loader) loaders.set(name, (loader = once(() => load(name).then((v) => (loaded.set(name, v), v)))));
    return loader();
  };
  return Object.assign(get, { peek: (name: string) => loaded.get(name) });
}

export const contractConnections: PerName<Connections> = perName(getConnections);
export const contractDetail: PerName<ContractDetail> = perName(getContract);
