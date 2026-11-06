// Responses written by crates/ilold-web/tests/ts_fixtures_test.rs; `npm run check` fails when a type drifts from them.
import type { ContractDetail, CytoscapeGraph, ProjectDepGraph, ProjectMap } from '../rest';
import projectMap from './project_map.json';
import projectDepgraph from './project_depgraph.json';
import callgraph from './callgraph.json';
import contractDetail from './contract_detail.json';

// imported JSON widens string literals, so unions of literals are compared as strings
type Json<T> = T extends string ? string : T extends (infer U)[] ? Json<U>[] : T extends object ? { [K in keyof T]: Json<T[K]> } : T;

projectMap satisfies Json<ProjectMap>;
projectDepgraph satisfies Json<ProjectDepGraph>;
callgraph satisfies Json<CytoscapeGraph>;
contractDetail satisfies Json<ContractDetail>;
