// Responses written by crates/ilold-web/tests/ts_fixtures_test.rs; `npm run check` fails when a type drifts from them.
import type { ContractDetail, CytoscapeGraph, ProjectDepGraph, ProjectMap, VariableInfo } from '../rest';
import type { FunctionNarrative } from '../types';
import projectMap from './project_map.json';
import projectDepgraph from './project_depgraph.json';
import callgraph from './callgraph.json';
import contractDetail from './contract_detail.json';
import keysContractDetail from './keys_contract_detail.json';
import keysCfg from './keys_cfg.json';
import keysWho from './keys_who.json';
import keysFunction from './keys_function.json';
import trycatchContractDetail from './trycatch_contract_detail.json';
import trycatchCfg from './trycatch_cfg.json';
import trycatchFunction from './trycatch_function.json';

// imported JSON widens string literals, so unions of literals are compared as strings
type Json<T> = T extends string ? string : T extends (infer U)[] ? Json<U>[] : T extends object ? { [K in keyof T]: Json<T[K]> } : T;

projectMap satisfies Json<ProjectMap>;
projectDepgraph satisfies Json<ProjectDepGraph>;
callgraph satisfies Json<CytoscapeGraph>;
contractDetail satisfies Json<ContractDetail>;
keysContractDetail satisfies Json<ContractDetail>;
keysCfg satisfies Json<CytoscapeGraph>;
keysWho.VariableInfo satisfies Json<VariableInfo>;
keysFunction satisfies Json<FunctionNarrative>;
trycatchContractDetail satisfies Json<ContractDetail>;
trycatchCfg satisfies Json<CytoscapeGraph>;
trycatchFunction satisfies Json<FunctionNarrative>;
