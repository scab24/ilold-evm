// Responses written by crates/ilold-web/tests/ts_fixtures_test.rs; `npm run check` fails when a type drifts from them.
import type { CytoscapeGraph, ProjectDepGraph, ProjectMap } from '../rest';
import projectMap from './project_map.json';
import projectDepgraph from './project_depgraph.json';
import callgraph from './callgraph.json';

projectMap satisfies ProjectMap;
projectDepgraph satisfies ProjectDepGraph;
callgraph satisfies CytoscapeGraph;
