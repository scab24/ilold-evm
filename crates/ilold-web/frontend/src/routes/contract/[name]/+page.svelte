<script lang="ts">
  import { page } from '$app/state';
  import { onMount, onDestroy, tick, untrack } from 'svelte';
  import { getContract, getCallGraph, getCfg, getPaths, getSequences, getSequenceAnalysis, getFunctionSource, getProjectMap, type ContractDetail, type CytoscapeGraph, type SequenceAnalysis, type MapContract } from '$lib/api/rest';
  import { goto } from '$app/navigation';
  import { toggleTerminal } from '$lib/stores/terminal.svelte';
  import { openInIde } from '$lib/utils/ide-links';
  import { setSearchContext, getSearchNavigate, setSearchNavigate } from '$lib/stores/search.svelte';
  import { togglePalette, setPaletteCommands, clearPaletteCommands } from '$lib/stores/palette.svelte';
  import type { Command } from '$lib/commands/registry';
  import { getHighlightedFunction, getScenarios, getActiveScenario, getForkOrigins, setSessionContract } from '$lib/stores/session.svelte';
  import { notifyError } from '$lib/components/ui/notifications.svelte';
  import ErrorToasts from '$lib/components/ui/ErrorToasts.svelte';
  import { composeScenarioTree, type ComposedNode } from '$lib/canvas/scenarios';
  import { promptScenarioName } from '$lib/scenarios/name';
  import { dispatchScenarioAction } from '$lib/scenarios/dispatch';
  import { postCommand } from '$lib/api/session';
  import Legend from '$lib/components/contract/Legend.svelte';
  import FunctionSidebar from '$lib/components/contract/FunctionSidebar.svelte';
  import TopBar from '$lib/components/contract/TopBar.svelte';
  import StatusBar from '$lib/components/contract/StatusBar.svelte';
  import ContextMenu from '$lib/components/contract/ContextMenu.svelte';
  import FunctionSourcePanel from '$lib/components/contract/FunctionSourcePanel.svelte';
  import GraphCanvasFlow from '$lib/components/contract/GraphCanvasFlow.svelte';
  import SessionSidebar from '$lib/components/session/SessionSidebar.svelte';
  import {
    getNodes, getEdges,
    setNodes, setEdges,
    addNode, addEdge,
    addNodes, addEdges,
    removeNodesById, findNode,
    findDescendants,
    clearGraph,
    type GraphNodeData,
  } from '$lib/stores/graph.svelte';
  import { runDagreLayout } from '$lib/utils/graph-helpers';
  import { MarkerType } from '@xyflow/svelte';
  import type { Node, Edge } from '@xyflow/svelte';

  let contract: ContractDetail | null = $state(null);
  let error: string | null = $state(null);
  let selectedNode: any = $state(null);
  let selectedPath: any = $state(null);
  // Raw, replaced on every update: a $state proxy hides inherited keys such as `constructor`
  let funcPaths: Record<string, any> = $state.raw({});
  let expandedFuncs: Set<string> = $state(new Set());
  let selectionCount: number = $state(0);
  let mode: 'cfg' | 'sequences' | 'session' = $state('sequences');
  let seqTree: any = $state(null);
  let seqAnalysis: SequenceAnalysis | null = $state(null);
  let seqExpanded: Map<string, boolean> = $state(new Map());
  let seqDirection: 'TB' | 'LR' = $state('TB');

  let contextMenu: {
    x: number;
    y: number;
    nodeId: string;
    funcName: string;
    nodeType: string;
    sessionStep?: { stepIndex: number };
  } | null = $state(null);

  let canvasFuncs: Set<string> = $state(new Set()); // functions currently on canvas

  let projectMap: MapContract[] = $state([]);

  let sourcePanel: { func: string } | null = $state(null);

  let sessionVisCount = $state(0);
  const sessionHighlight = $derived(getHighlightedFunction());

  let callgraphRaw: CytoscapeGraph | null = $state(null);
  let flowApi: { fitView: (opts?: any) => Promise<boolean> } | null = $state(null);
  let graphCanvas: GraphCanvasFlow;
  const cfgCache = new Map<string, CytoscapeGraph>();

  // Caches keyed by Solidity names; own-property checks keep `constructor` & co. off Object.prototype
  function hasPaths(funcName: string): boolean {
    return Object.hasOwn(funcPaths, funcName) && !!funcPaths[funcName];
  }

  function liveNodePosition(nodeId: string): { x: number; y: number } | null {
    const live = graphCanvas?.getLiveNode?.(nodeId);
    if (live?.position) return live.position;
    const stored = findNode(nodeId);
    return stored?.position ?? null;
  }

  function findSeqRootFunction(nodeId: string): Node<GraphNodeData> | null {
    const visited = new Set<string>();
    let current = findNode(nodeId);
    while (current && !visited.has(current.id)) {
      visited.add(current.id);
      if (current.data._type === 'function') return current;
      const parentId = (current.data as any)._seqParent;
      if (!parentId) return null;
      current = findNode(parentId);
    }
    return null;
  }

  const SEQ_NODE_W = 220;
  const SEQ_NODE_H = 80;
  const SEQ_SIBLING_GAP = 30; // gap between siblings at the same level
  const SEQ_LEVEL_GAP = 120;  // gap between parent and children rank

  function relayoutSeqTree(rootId: string) {
    const root = findNode(rootId);
    if (!root) return;
    const rootPos = liveNodePosition(rootId) ?? root.position;

    const subtreeIds = new Set<string>([rootId]);
    let added = true;
    while (added) {
      added = false;
      for (const n of getNodes()) {
        if (n.data._type === 'seq-next' && !subtreeIds.has(n.id)) {
          const sp = (n.data as any)._seqParent as string;
          if (sp && subtreeIds.has(sp)) {
            subtreeIds.add(n.id);
            added = true;
          }
        }
      }
    }

    const childrenMap = new Map<string, string[]>();
    const subtreeEdgeIds = new Set<string>();
    for (const e of getEdges()) {
      if (e.data?._type === 'seq-edge' && subtreeIds.has(e.source) && subtreeIds.has(e.target)) {
        const arr = childrenMap.get(e.source) ?? [];
        arr.push(e.target);
        childrenMap.set(e.source, arr);
        subtreeEdgeIds.add(e.id);
      }
    }

    const levels = new Map<string, number>();
    levels.set(rootId, 0);
    const queue = [rootId];
    let maxLevel = 0;
    while (queue.length > 0) {
      const id = queue.shift()!;
      const lvl = levels.get(id)!;
      for (const kid of childrenMap.get(id) ?? []) {
        if (!levels.has(kid)) {
          levels.set(kid, lvl + 1);
          maxLevel = Math.max(maxLevel, lvl + 1);
          queue.push(kid);
        }
      }
    }

    const byLevel: string[][] = Array.from({ length: maxLevel + 1 }, () => []);
    for (const [id, lvl] of levels) byLevel[lvl].push(id);

    const isLR = seqDirection === 'LR';
    const posMap = new Map<string, { x: number; y: number }>();
    for (let lvl = 1; lvl <= maxLevel; lvl++) {
      const ids = byLevel[lvl];
      const count = ids.length;
      if (isLR) {
        const totalH = count * SEQ_NODE_H + (count - 1) * SEQ_SIBLING_GAP;
        const startY = rootPos.y + SEQ_NODE_H / 2 - totalH / 2;
        const x = rootPos.x + lvl * (SEQ_NODE_W + SEQ_LEVEL_GAP);
        ids.forEach((id, i) => {
          posMap.set(id, { x, y: startY + i * (SEQ_NODE_H + SEQ_SIBLING_GAP) });
        });
      } else {
        const totalW = count * SEQ_NODE_W + (count - 1) * SEQ_SIBLING_GAP;
        const startX = rootPos.x + SEQ_NODE_W / 2 - totalW / 2;
        const y = rootPos.y + lvl * (SEQ_NODE_H + SEQ_LEVEL_GAP);
        ids.forEach((id, i) => {
          posMap.set(id, { x: startX + i * (SEQ_NODE_W + SEQ_SIBLING_GAP), y });
        });
      }
    }

    setNodes(getNodes().map(n => {
      if (n.data._type === 'seq-next' && posMap.has(n.id)) {
        return { ...n, position: posMap.get(n.id)! };
      }
      return n;
    }));

    const sh = isLR ? 'r' : 'b';
    const th = isLR ? 'l' : 't';
    setEdges(getEdges().map(e => {
      if (subtreeEdgeIds.has(e.id)) {
        return { ...e, sourceHandle: sh, targetHandle: th };
      }
      return e;
    }));
  }

  function reorientAllSeqSubtrees() {
    const roots = new Set<string>();
    for (const n of getNodes()) {
      if (n.data._type === 'seq-next') {
        const root = findSeqRootFunction(n.id);
        if (root) roots.add(root.id);
      }
    }
    for (const rootId of roots) {
      relayoutSeqTree(rootId);
    }
  }

  function edgeStyle(base: string | undefined, opacity: number): string {
    const cleaned = (base ?? '').replace(/opacity:\s*[\d.]+;?/g, '').trim();
    const sep = cleaned && !cleaned.endsWith(';') ? '; ' : ' ';
    return `${cleaned}${cleaned ? sep : ''}opacity: ${opacity}`.trim();
  }

  function resetAllDimmed() {
    setNodes(getNodes().map(n => {
      if ('_dimmed' in n.data && n.data._dimmed) {
        return { ...n, data: { ...n.data, _dimmed: false } as GraphNodeData };
      }
      return n;
    }));
    setEdges(getEdges().map(e => {
      if (e.data?._dimmed) {
        return { ...e, style: edgeStyle(e.style, 1), data: { ...e.data, _dimmed: false } };
      }
      return e;
    }));
  }

  function collapseAllDescendants(nodeId: string) {
    const allDesc = findDescendants(nodeId);
    const toRemove = new Set<string>();
    for (const id of allDesc) {
      const n = findNode(id);
      if (n && n.data._type === 'seq-next') toRemove.add(id);
    }
    if (toRemove.size > 0) removeNodesById(toRemove);
  }

  function dimFunctionLayer(excludeId?: string) {
    setNodes(getNodes().map(n => {
      if (n.data._type === 'function' && n.id !== excludeId) {
        return { ...n, data: { ...n.data, _dimmed: true } as GraphNodeData };
      }
      return n;
    }));
    setEdges(getEdges().map(e => {
      if (e.data?._type === 'call') {
        return { ...e, style: edgeStyle(e.style, 0.1), data: { ...e.data, _dimmed: true } };
      }
      return e;
    }));
  }

  function dashedEdgeStyle(stroke: string): string {
    return `stroke-dasharray: 5 3; stroke: ${stroke}`;
  }

  function extractCondition(kind: string): string | undefined {
    const match = kind.match(/condition:\s*"([^"]+)"/);
    return match?.[1];
  }

  function extractCatchKind(kind: string): string | undefined {
    const match = kind.match(/kind:\s*"([^"]+)"/);
    return match?.[1];
  }

  function truncateLabel(text: string, max = 30): string {
    return text.length > max ? text.slice(0, max - 1) + '…' : text;
  }

  function cfgEdgeStyle(kind: string): {
    color: string;
    label?: string;
    animated: boolean;
    sourceHandle: string;
    targetHandle: string;
  } {
    if (kind.startsWith('ConditionalTrue')) {
      const cond = extractCondition(kind);
      return {
        color: 'var(--color-success)',
        label: cond ? truncateLabel(cond) : '✓',
        animated: false,
        sourceHandle: 'b',
        targetHandle: 't',
      };
    }
    if (kind.startsWith('ConditionalFalse')) {
      const cond = extractCondition(kind);
      return {
        color: 'var(--color-warning)',
        label: cond ? truncateLabel(cond) : '✗',
        animated: false,
        sourceHandle: 'b',
        targetHandle: 't',
      };
    }
    if (kind === 'LoopBack') {
      return {
        color: 'var(--color-accent)',
        animated: true,
        sourceHandle: 'r',
        targetHandle: 'r',
      };
    }
    if (kind === 'LoopExit') {
      return {
        color: 'var(--color-text-muted)',
        animated: false,
        sourceHandle: 'b',
        targetHandle: 't',
      };
    }
    if (kind === 'ExternalCallSuccess') {
      return {
        color: 'var(--color-success-light)',
        animated: false,
        sourceHandle: 'b',
        targetHandle: 't',
      };
    }
    if (kind === 'ExternalCallFailure') {
      return {
        color: 'var(--color-danger)',
        animated: false,
        sourceHandle: 'b',
        targetHandle: 't',
      };
    }
    if (kind.startsWith('CatchClause')) {
      const catchKind = extractCatchKind(kind);
      return {
        color: 'var(--color-danger-light)',
        label: catchKind ? truncateLabel(catchKind) : undefined,
        animated: false,
        sourceHandle: 'b',
        targetHandle: 't',
      };
    }
    return {
      color: 'var(--color-text-dim)',
      animated: false,
      sourceHandle: 'b',
      targetHandle: 't',
    };
  }

  $effect(() => {
    const scenarios = getScenarios();
    const forkOrigins = getForkOrigins();
    const active = getActiveScenario();
    if (!contract || !callgraphRaw) return;

    const tree = composeScenarioTree(scenarios, forkOrigins);

    untrack(() => {
      const toRemove = new Set<string>();
      for (const n of getNodes()) {
        if (n.id.startsWith('session:')) toRemove.add(n.id);
      }
      if (toRemove.size > 0) removeNodesById(toRemove);

      if (tree.nodes.length === 0) {
        sessionVisCount = 0;
        return;
      }

      const allFuncs = [...(contract?.functions ?? []), ...(contract?.inherited_functions ?? [])];

      const SESSION_BASE_X = 200;
      const SESSION_BASE_Y = 300;
      const SESSION_STEP_WIDTH = 280;
      const SESSION_LANE_HEIGHT = 100;

      const composedNodes: Node<GraphNodeData>[] = tree.nodes.map((cn: ComposedNode) => {
        const funcDetail = allFuncs.find((f: any) => f.name === cn.function);
        return {
          id: cn.id,
          type: 'function',
          position: {
            x: SESSION_BASE_X + cn.stepIndex * SESSION_STEP_WIDTH,
            y: SESSION_BASE_Y + cn.lane * SESSION_LANE_HEIGHT,
          },
          data: {
            _type: 'function',
            _sessionStep: true,
            _scenario: cn._scenario,
            _scenariosPassingThrough: cn._scenariosPassingThrough,
            _activeScenario: active,
            stepIndex: cn.stepIndex,
            label: cn.function,
            is_external: false,
            contractName: contract!.name,
            visibility: funcDetail?.visibility,
            mutability: funcDetail?.mutability,
            path_count: funcDetail?.path_count,
            modifiers: funcDetail?.modifiers,
          } as GraphNodeData,
        } as Node<GraphNodeData>;
      });
      addNodes(composedNodes);

      const nodeScenarios = new Map<string, string[]>(
        tree.nodes.map((n) => [n.id, n._scenariosPassingThrough]),
      );

      const composedEdges: Edge[] = tree.edges.map((ce) => {
        const isFork = ce._forkEdge === true;
        const sourceScns = nodeScenarios.get(ce.source) ?? [];
        const targetScns = nodeScenarios.get(ce.target) ?? [];
        const onActivePath = sourceScns.includes(active) && targetScns.includes(active);

        const color = onActivePath
          ? (isFork ? 'var(--color-accent)' : 'var(--color-accent-hover)')
          : 'var(--color-text-dim)';
        const opacity = onActivePath ? 1 : 0.4;

        return {
          id: ce.id,
          source: ce.source,
          target: ce.target,
          sourceHandle: 'r',
          targetHandle: 'l',
          type: 'default',
          animated: isFork && onActivePath,
          style: `stroke: ${color}; opacity: ${opacity}; ${isFork ? 'stroke-dasharray: 4 4;' : ''}`,
          markerEnd: { type: MarkerType.ArrowClosed, width: 12, height: 12, color },
          labelBgStyle: { fill: 'var(--color-surface)', fillOpacity: 0.85 },
          labelBgPadding: [3, 5] as [number, number],
          data: { _type: 'session-path', _scenario: ce._scenario, _forkEdge: isFork },
        };
      });
      addEdges(composedEdges);

      sessionVisCount = tree.nodes.length;
    });
  });

  $effect(() => {
    const currentMode = mode;
    untrack(() => {
      setNodes(getNodes().map((n) => {
        const isSessionNode = (n.data as any)._sessionStep === true;
        const shouldHide = currentMode === 'session' && !isSessionNode;
        return n.hidden === shouldHide ? n : { ...n, hidden: shouldHide };
      }));
      setEdges(getEdges().map((e) => {
        const isSessionEdge = (e.data as any)?._type === 'session-path';
        const shouldHide = currentMode === 'session' && !isSessionEdge;
        return e.hidden === shouldHide ? e : { ...e, hidden: shouldHide };
      }));
    });
  });

  $effect(() => {
    if (!selectedNode) return;
    if (!findNode(selectedNode.id)) {
      selectedNode = null;
      selectedPath = null;
    }
  });

  $effect(() => {
    const funcName = sessionHighlight;
    if (!funcName) return;
    const node = getNodes().find(
      n => n.data._type === 'function' && n.data.label === funcName
    );
    if (node) selectedNode = node;
  });

  onMount(async () => {
    const contractName = page.params.name;
    if (!contractName) return;
    clearGraph();
    canvasFuncs = new Set();
    expandedFuncs = new Set();
    seqExpanded = new Map();
    selectedNode = null;
    selectedPath = null;
    setSearchContext(contractName);
    try {
      contract = await getContract(contractName);
      const callgraphData = await getCallGraph(contractName);
      callgraphRaw = callgraphData;
      try { seqTree = await getSequences(contractName); } catch {}
      try { seqAnalysis = await getSequenceAnalysis(contractName); } catch {}
      try {
        const pm = await getProjectMap();
        projectMap = pm.contracts;
      } catch {}
    } catch (e) {
      error = `Contract "${contractName}" not found`;
    }
  });

  $effect(() => {
    const nav = getSearchNavigate();
    if (!nav) return;
    untrack(() => {
      if (!contract) return;
      if (nav.contract !== contract.name) return;

      let stale = false;

      (async () => {
        try {
          if (!canvasFuncs.has(nav.func)) {
            addFuncToCanvas(nav.func);
            await tick();
          }
          if (stale || !contract) return;

          if (!hasPaths(nav.func)) {
            const paths = await getPaths(contract.name, nav.func);
            funcPaths = { ...funcPaths, [nav.func]: paths };
          }
          if (stale || !contract) return;

          if (!expandedFuncs.has(nav.func)) {
            await toggleFuncExpand(nav.func);
          }
          if (stale) return;

          const funcNode = getNodes().find(
            n => n.data._type === 'function' && n.data.label === nav.func
          );
          if (funcNode) {
            selectedNode = { ...funcNode.data, id: funcNode.id };
            const path = (hasPaths(nav.func) ? funcPaths[nav.func] : undefined)?.paths?.find((p: any) => p.id === nav.pathId);
            if (path) highlightPath(nav.func, path);
            if (flowApi) {
              await tick();
              flowApi.fitView({ nodes: [{ id: funcNode.id }], padding: 0.5, duration: 400 });
            }
          }
        } finally {
          if (!stale) setSearchNavigate(null);
        }
      })();
    });
  });

  // The session store follows the contract this page shows.
  $effect(() => {
    const name = contract?.name;
    if (name) untrack(() => setSessionContract(name));
  });

  function notifyFailure(label: string, e: unknown) {
    notifyError(label, e);
  }

  async function handleSidebarAdd(funcName: string) {
    if (mode === 'session') {
      try {
        await postCommand({ Call: { func: funcName } }, contract?.name);
      } catch (e) {
        notifyFailure('add step', e);
      }
      return;
    }
    addFuncToCanvas(funcName);
  }

  function addFuncToCanvas(funcName: string) {
    if (!callgraphRaw || canvasFuncs.has(funcName)) return;
    const nodeData = callgraphRaw.nodes.find(n => n.data.label === funcName);
    if (!nodeData) return;

    const allFuncs = [...(contract?.functions ?? []), ...(contract?.inherited_functions ?? [])];
    const funcDetail = allFuncs.find((f: any) => f.name === funcName);

    const count = canvasFuncs.size;
    const position = {
      x: 300 + (count % 3 - 1) * 200,
      y: 200 + Math.floor(count / 3) * 100,
    };

    addNode({
      id: nodeData.data.id,
      type: 'function',
      position,
      data: {
        _type: 'function',
        label: nodeData.data.label,
        is_external: nodeData.data.is_external ?? false,
        contractName: nodeData.data.contract,
        visibility: funcDetail?.visibility,
        mutability: funcDetail?.mutability,
        path_count: funcDetail?.path_count,
        modifiers: funcDetail?.modifiers,
      },
    } as Node<GraphNodeData>);

    for (const e of callgraphRaw.edges) {
      const srcOnCanvas = canvasFuncs.has(
        callgraphRaw.nodes.find(n => n.data.id === e.data.source)?.data.label ?? ''
      ) || e.data.source === nodeData.data.id;
      const tgtOnCanvas = canvasFuncs.has(
        callgraphRaw.nodes.find(n => n.data.id === e.data.target)?.data.label ?? ''
      ) || e.data.target === nodeData.data.id;

      if (srcOnCanvas && tgtOnCanvas) {
        if (!getEdges().some(existing => existing.id === e.data.id)) {
          addEdge({
            id: e.data.id,
            source: e.data.source,
            target: e.data.target,
            sourceHandle: 'r',
            targetHandle: 'l',
            type: 'default',
            style: `stroke: var(--color-text-muted)`,
            markerEnd: { type: MarkerType.ArrowClosed, width: 12, height: 12, color: 'var(--color-text-muted)' },
            data: { _type: 'call', kind: e.data.kind },
            ...(e.data.call_count > 1 ? {
              label: `\u00D7${e.data.call_count}`,
              labelBgStyle: { fill: 'var(--color-surface)', fillOpacity: 0.85 },
              labelBgPadding: [3, 5] as [number, number],
            } : {}),
          });
        }
      }
    }

    canvasFuncs.add(funcName);
    canvasFuncs = new Set(canvasFuncs);
  }

  function removeSeqNode(nodeId: string) {
    const descendants = findDescendants(nodeId);
    descendants.add(nodeId);
    removeNodesById(descendants);

    seqExpanded.delete(nodeId);
    for (const k of seqExpanded.keys()) {
      if (!findNode(k)) seqExpanded.delete(k);
    }
    seqExpanded = new Map(seqExpanded);
  }

  function removeFuncFromCanvas(funcName: string) {
    if (!canvasFuncs.has(funcName)) return;
    const funcNode = getNodes().find(
      n => n.data._type === 'function' && n.data.label === funcName
    );
    if (!funcNode) return;
    const nodeId = funcNode.id;

    const toRemove = new Set<string>([nodeId]);

    for (const n of getNodes()) {
      if ('_parentFunc' in n.data && n.data._parentFunc === funcName) {
        toRemove.add(n.id);
      }
    }

    const seqDesc = findDescendants(nodeId);
    for (const id of seqDesc) toRemove.add(id);

    for (const n of getNodes()) {
      if ('_seqParent' in n.data) {
        const sp = n.data._seqParent as string;
        if (sp === nodeId || sp.startsWith(nodeId + '→')) {
          toRemove.add(n.id);
        }
      }
    }

    removeNodesById(toRemove);

    if (expandedFuncs.has(funcName)) {
      resetAllDimmed();
    }

    canvasFuncs.delete(funcName);
    canvasFuncs = new Set(canvasFuncs);
    expandedFuncs.delete(funcName);
    expandedFuncs = new Set(expandedFuncs);
    seqExpanded.delete(nodeId);
    seqExpanded = new Map(seqExpanded);
  }

  function handleNodesDelete(nodes: Node<GraphNodeData>[]) {
    if (mode === 'session') return;
    const funcsToRemove = new Set<string>();
    const seqNodesToRemove: string[] = [];
    for (const n of nodes) {
      const d = n.data;
      if (d._type === 'function') {
        funcsToRemove.add(d.label);
      } else if (d._type === 'block') {
        const parent = (d as any)._parentFunc;
        if (parent) funcsToRemove.add(parent);
      } else if (d._type === 'seq-next') {
        seqNodesToRemove.push(n.id);
      }
    }
    for (const nid of seqNodesToRemove) removeSeqNode(nid);
    for (const fname of funcsToRemove) removeFuncFromCanvas(fname);
    selectedNode = null;
  }


  async function handleNodeTap(node: Node<GraphNodeData>) {
    const data = node.data;

    if (!selectedNode || selectedNode.id !== node.id) {
      selectedPath = null;
      setNodes(getNodes().map(n => {
        if (n.data._type === 'block' && '_dimmed' in n.data && n.data._dimmed) {
          return { ...n, data: { ...n.data, _dimmed: false } as GraphNodeData };
        }
        return n;
      }));
    }

    selectedNode = { ...data, id: node.id };
    contextMenu = null;

    const funcName = data._type === 'function' ? data.label
      : data._type === 'block' ? (data as any)._parentFunc
      : data._type === 'seq-next' ? ((data as any)._funcName || data.label)
      : null;

    if (funcName && contract && !hasPaths(funcName)) {
      try {
        const paths = await getPaths(contract.name, funcName);
        funcPaths = { ...funcPaths, [funcName]: paths };
      } catch {}
    }
  }

  function handleBackgroundTap() {
    selectedNode = null;
    selectedPath = null;
    contextMenu = null;
    resetAllDimmed();
  }

  async function handleForkScenario(stepIndex: number) {
    contextMenu = null;
    const name = promptScenarioName();
    if (!name) return;
    await dispatchScenarioAction(
      { Fork: { name, at_step: stepIndex + 1 } },
      contract?.name,
      'fork',
    );
  }

  async function handleSessionBack() {
    try {
      await postCommand('Back', contract?.name);
    } catch (e) {
      notifyFailure('session back', e);
    }
  }

  async function handleSessionClear() {
    const active = getActiveScenario();
    const steps = getScenarios().get(active) ?? [];
    if (steps.length === 0) return;
    if (!confirm(`Clear all ${steps.length} step(s) from scenario "${active}"?`)) return;
    try {
      await postCommand('Clear', contract?.name);
    } catch (e) {
      notifyFailure('session clear', e);
    }
  }

  function handleViewSource(funcName: string) {
    contextMenu = null;
    sourcePanel = { func: funcName };
  }

  async function handleOpenInIde(funcName: string) {
    contextMenu = null;
    if (!contract) return;
    try {
      const res = await getFunctionSource(contract.name, funcName);
      openInIde(res.file_path, res.span.start_line, res.span.start_col);
    } catch (e) {
      notifyFailure('open in IDE', e);
    }
  }

  async function handleRemoveFromHere(stepIndex: number) {
    contextMenu = null;
    const active = getActiveScenario();
    const steps = getScenarios().get(active) ?? [];
    const backCount = steps.length - stepIndex;
    if (backCount <= 0) return;
    for (let i = 0; i < backCount; i++) {
      try {
        await postCommand('Back', contract?.name);
      } catch (e) {
        notifyFailure('remove-from-here iteration', e);
        break;
      }
    }
  }

  function handleContextMenu(event: MouseEvent, node: Node<GraphNodeData>) {
    const data = node.data;
    let sessionStep: { stepIndex: number } | undefined;
    if (data._type === 'function' && data._sessionStep === true) {
      const { _scenariosPassingThrough: scns, _activeScenario: active, stepIndex: idx } = data;
      if (typeof idx === 'number' && active && scns?.includes(active)) {
        sessionStep = { stepIndex: idx };
      }
    }
    contextMenu = {
      x: event.clientX,
      y: event.clientY,
      nodeId: node.id,
      funcName: data._type === 'function' ? data.label : ('_parentFunc' in data ? (data as any)._parentFunc : ('_funcName' in data ? (data as any)._funcName : (data as any).label)),
      nodeType: data._type,
      sessionStep,
    };
  }

  async function handleNodeClick(node: Node<GraphNodeData>, event?: MouseEvent) {
    handleNodeTap(node);
    if (mode === 'session') return;
    const d = node.data;
    if (d._type === 'function' && !d.is_external) {
      await handleFunctionTap(d.label, node.id);
    } else if (d._type === 'seq-next') {
      await handleSeqNodeTap((d as any)._funcName || d.label, node.id, (d as any)._seqParent, event);
    }
  }

  async function handleFunctionTap(funcName: string, nodeId: string) {
    if (mode === 'cfg') {
      await toggleFuncExpand(funcName);
    } else if (mode === 'sequences') {
      await toggleSeqExpand(funcName, nodeId);
    }
  }

  async function handleSeqNodeTap(funcName: string, nodeId: string, seqParent: string, event?: MouseEvent) {
    const keepSiblings = event?.shiftKey === true;
    if (seqParent && !keepSiblings) {
      const siblings = getNodes().filter(
        n => n.data._type === 'seq-next'
          && (n.data as any)._seqParent === seqParent
          && n.id !== nodeId
      );
      const toRemove = new Set<string>();
      for (const sib of siblings) {
        for (const id of findDescendants(sib.id)) toRemove.add(id);
        toRemove.add(sib.id);
        seqExpanded.delete(sib.id);
      }
      if (toRemove.size > 0) removeNodesById(toRemove);
      seqExpanded = new Map(seqExpanded);
    }

    await toggleSeqExpand(funcName, nodeId);
  }

  async function toggleFuncExpand(funcName: string, anchorNodeId?: string) {
    if (!contract) return;
    const parentId = anchorNodeId || `${contract.name}::${funcName}`;

    if (expandedFuncs.has(funcName)) {
      const toRemove = new Set<string>();
      for (const n of getNodes()) {
        if ('_parentFunc' in n.data && n.data._parentFunc === funcName) {
          toRemove.add(n.id);
        }
      }
      removeNodesById(toRemove);
      resetAllDimmed();

      expandedFuncs.delete(funcName);
      expandedFuncs = new Set(expandedFuncs);
      return;
    }

    let cfg = cfgCache.get(funcName);
    if (!cfg) {
      cfg = await getCfg(contract.name, funcName);
      cfgCache.set(funcName, cfg);
    }
    const parentPos = liveNodePosition(parentId) ?? { x: 300, y: 200 };

    const cfgNodes: Node<GraphNodeData>[] = cfg.nodes.map(n => ({
      id: `cfg:${funcName}:${n.data.id}`,
      type: 'block',
      position: { ...parentPos },
      data: {
        _type: 'block' as const,
        label: n.data.label,
        node_type: n.data.node_type,
        _parentFunc: funcName,
        statements: n.data.statements,
      },
    }));

    const cfgEdges: Edge[] = cfg.edges.map((e, i) => {
      const es = cfgEdgeStyle(e.data.kind);
      return {
        id: `cfg-edge:${funcName}:${i}`,
        source: `cfg:${funcName}:${e.data.source}`,
        target: `cfg:${funcName}:${e.data.target}`,
        sourceHandle: es.sourceHandle,
        targetHandle: es.targetHandle,
        type: 'smoothstep',
        data: {
          _type: 'cfg-edge',
          _parentFunc: funcName,
          kind: e.data.kind,
        },
        label: es.label,
        labelBgStyle: es.label ? { fill: 'var(--color-surface)', fillOpacity: 0.85 } : undefined,
        labelBgPadding: es.label ? [4, 6] as [number, number] : undefined,
        style: `stroke: ${es.color}`,
        animated: es.animated,
        markerEnd: { type: MarkerType.ArrowClosed, width: 16, height: 16, color: es.color },
      };
    });

    const entryNode = cfg.nodes.find(n => n.data.node_type === 'Entry');
    if (entryNode) {
      cfgEdges.push({
        id: `cfg-link:${funcName}`,
        source: parentId,
        target: `cfg:${funcName}:${entryNode.data.id}`,
        sourceHandle: 'b',
        targetHandle: 't',
        type: 'smoothstep',
        data: { _type: 'cfg-edge', _parentFunc: funcName, kind: 'expand' },
        style: dashedEdgeStyle('var(--color-accent-dark)'),
        markerEnd: { type: MarkerType.ArrowClosed, width: 16, height: 16, color: 'var(--color-accent-dark)' },
      });
    }

    const layoutNodes = runDagreLayout(cfgNodes, cfgEdges, {
      rankDir: 'TB', nodeSep: 40, rankSep: 60, nodeWidth: 180,
    });

    let minX = Infinity, minY = Infinity, maxX = -Infinity;
    for (const n of layoutNodes) {
      if (n.position.x < minX) minX = n.position.x;
      if (n.position.x > maxX) maxX = n.position.x;
      if (n.position.y < minY) minY = n.position.y;
    }
    const centerX = (minX + maxX) / 2;
    const offsetX = parentPos.x - centerX;
    const offsetY = parentPos.y + 60 - minY;

    const finalPositions = new Map<string, { x: number; y: number }>();
    for (const n of layoutNodes) {
      finalPositions.set(n.id, { x: n.position.x + offsetX, y: n.position.y + offsetY });
    }

    for (const n of cfgNodes) {
      const final = finalPositions.get(n.id);
      if (final) n.position = final;
    }
    addNodes(cfgNodes);
    addEdges(cfgEdges);

    dimFunctionLayer(parentId);

    expandedFuncs.add(funcName);
    expandedFuncs = new Set(expandedFuncs);
  }

  async function toggleSeqExpand(funcName: string, parentNodeId: string) {
    if (seqExpanded.has(parentNodeId)) {
      collapseAllDescendants(parentNodeId);
      seqExpanded.delete(parentNodeId);
      seqExpanded = new Map(seqExpanded);

      const anySeq = getNodes().some(n => n.data._type === 'seq-next');
      if (!anySeq) resetAllDimmed();
      return;
    }

    if (!seqTree || !seqTree.functions) return;

    const rootFunc = findSeqRootFunction(parentNodeId);
    if (!rootFunc) return;

    const seqFunctions: Array<{ name: string; visibility: string; read_only: boolean; path_count: number }> = seqTree.functions;

    const targets = seqFunctions;

    const allFuncs = [...(contract?.functions ?? []), ...(contract?.inherited_functions ?? [])];

    const newNodes: Node<GraphNodeData>[] = [];
    const newEdges: Edge[] = [];
    for (const func of targets) {
      const targetName = func.name;
      const nodeId = `seq:${parentNodeId}→${targetName}`;
      const transition = seqAnalysis?.transitions?.find(
        t => t.from === funcName && t.to === targetName
      ) ?? null;
      const funcDetail = allFuncs.find((f: any) => f.name === targetName);

      newNodes.push({
        id: nodeId,
        type: 'sequence',
        position: { x: 0, y: 0 },
        data: {
          _type: 'seq-next',
          label: targetName,
          _funcName: targetName,
          _seqParent: parentNodeId,
          readOnly: func.read_only,
          pathCount: func.path_count,
          visibility: func.visibility,
          modifiers: funcDetail?.modifiers,
          _transition: transition,
        },
      } as Node<GraphNodeData>);

      newEdges.push({
        id: `seq-edge:${parentNodeId}→${targetName}`,
        source: parentNodeId,
        sourceHandle: seqDirection === 'LR' ? 'r' : 'b',
        target: nodeId,
        targetHandle: seqDirection === 'LR' ? 'l' : 't',
        type: 'default',
        data: { _type: 'seq-edge' },
        style: transition?.shared_state?.length
          ? dashedEdgeStyle('var(--color-warning)')
          : undefined,
      });
    }

    addNodes(newNodes);
    addEdges(newEdges);
    relayoutSeqTree(rootFunc.id);

    dimFunctionLayer(rootFunc.id);

    seqExpanded.set(parentNodeId, true);
    seqExpanded = new Map(seqExpanded);
  }

  function switchMode(newMode: 'cfg' | 'sequences' | 'session') {
    const toRemove = new Set<string>();
    for (const n of getNodes()) {
      if (n.data._type === 'block' || n.data._type === 'seq-next') {
        toRemove.add(n.id);
      }
    }
    if (toRemove.size > 0) removeNodesById(toRemove);
    resetAllDimmed();

    expandedFuncs = new Set();
    seqExpanded = new Map();
    selectedNode = null;
    selectedPath = null;
    mode = newMode;
  }

  function highlightPath(funcName: string, path: any) {
    selectedPath = path;

    const highlightedIds = new Set<string>(
      path.nodes.map((n: any) => `cfg:${funcName}:b${n.block_id}`)
    );

    const highlightedEdgePairs = new Set<string>();
    const blockIds = [...highlightedIds];
    for (let i = 0; i < blockIds.length - 1; i++) {
      highlightedEdgePairs.add(`${blockIds[i]}→${blockIds[i + 1]}`);
    }

    setNodes(getNodes().map(n => {
      if (n.data._type === 'block' && n.data._parentFunc === funcName) {
        const dimmed = !highlightedIds.has(n.id);
        return { ...n, data: { ...n.data, _dimmed: dimmed } as GraphNodeData };
      }
      return n;
    }));

    setEdges(getEdges().map(e => {
      if (e.data?._parentFunc === funcName && e.data?._type === 'cfg-edge') {
        const key = `${e.source}→${e.target}`;
        const dimmed = !highlightedEdgePairs.has(key);
        return { ...e, style: edgeStyle(e.style, dimmed ? 0.1 : 1), data: { ...e.data, _dimmed: dimmed } };
      }
      return e;
    }));
  }

  $effect(() => {
    if (!contract) {
      setPaletteCommands([]);
      return;
    }
    const ctr = contract;
    const cmds: Command[] = [];

    cmds.push(
      { id: 'mode:cfg', label: 'Mode: CFG', category: 'Mode', icon: '⊟', keywords: ['cfg', 'control flow'], run: () => switchMode('cfg') },
      { id: 'mode:sequences', label: 'Mode: Sequences', category: 'Mode', icon: '⇵', keywords: ['seq', 'calls'], run: () => switchMode('sequences') },
      { id: 'mode:session', label: 'Mode: Session', category: 'Mode', icon: '⎇', keywords: ['scenario', 'session'], run: () => switchMode('session') },
    );

    cmds.push(
      { id: 'canvas:center', label: 'Center canvas', category: 'Action', icon: '⊙', keywords: ['fit', 'zoom', 'reset view'], run: () => { flowApi?.fitView({ padding: 0.1 }); } },
      { id: 'canvas:clear', label: 'Clear canvas', category: 'Action', icon: '✕', keywords: ['reset', 'wipe'], run: () => {
        const names = Array.from(canvasFuncs);
        for (const n of names) removeFuncFromCanvas(n);
      } },
      { id: 'terminal:toggle', label: 'Toggle terminal', category: 'Action', icon: '>_', keywords: ['console', 'repl', 'pty'], run: () => toggleTerminal() },
    );

    cmds.push(
      { id: 'session:back', label: 'Back — remove last step', category: 'Action', icon: '↶', keywords: ['undo', 'step'], run: () => handleSessionBack() },
      { id: 'session:clear', label: 'Clear scenario', category: 'Action', icon: '🗑', keywords: ['reset scenario'], run: () => handleSessionClear() },
    );

    cmds.push({
      id: 'scenario:new',
      label: 'New scenario',
      category: 'Scenario',
      icon: '+',
      keywords: ['create', 'scenario'],
      run: async () => {
        const name = promptScenarioName();
        if (!name) return;
        await dispatchScenarioAction({ New: { name } }, ctr.name, 'new');
      },
    });
    const activeScn = getActiveScenario();
    for (const [name] of getScenarios()) {
      if (name !== activeScn) {
        cmds.push({
          id: `scenario:switch:${name}`,
          label: `Switch to scenario: ${name}`,
          category: 'Scenario',
          icon: '⎇',
          run: () => dispatchScenarioAction({ Switch: { name } }, ctr.name, 'switch'),
        });
        cmds.push({
          id: `scenario:delete:${name}`,
          label: `Delete scenario: ${name}`,
          category: 'Scenario',
          icon: '✕',
          run: () => dispatchScenarioAction({ Delete: { name } }, ctr.name, 'delete'),
        });
      }
    }

    const allFuncs = [
      ...(ctr.functions ?? []).map((f) => ({ name: f.name, source: 'own' as const })),
      ...(ctr.inherited_functions ?? []).map((f) => ({ name: f.name, source: 'inherited' as const })),
    ];
    allFuncs.forEach((f, i) => {
      cmds.push({
        id: `func:${f.source}:${i}:${f.name}`,
        label: f.name,
        category: 'Function',
        icon: 'ƒ',
        detail: f.source === 'inherited' ? 'inherited' : undefined,
        keywords: ['jump', 'function', 'canvas'],
        run: () => handleSidebarAdd(f.name),
      });
    });

    const seenContracts = new Set<string>([ctr.name]);
    for (const c of projectMap) {
      if (seenContracts.has(c.name)) continue;
      seenContracts.add(c.name);
      cmds.push({
        id: `contract:${c.name}`,
        label: c.name,
        category: 'Contract',
        icon: '◈',
        detail: c.kind,
        keywords: ['contract', 'navigate', 'open'],
        run: () => goto(`/contract/${encodeURIComponent(c.name)}`),
      });
    }

    setPaletteCommands(cmds);
  });

  onDestroy(() => {
    clearPaletteCommands();
  });
</script>

<div class="fixed inset-0 flex flex-col bg-dark">
  {#if error}
    <div class="p-6 text-danger">{error}</div>
  {:else}
    <TopBar
      contractName={contract?.name ?? '...'}
      {mode}
      {seqDirection}
      onmodechange={switchMode}
      onsearch={togglePalette}
      oncenter={() => flowApi?.fitView({ padding: 0.1 })}
      onseqdirection={(dir) => { seqDirection = dir; reorientAllSeqSubtrees(); }}
      onsessionback={handleSessionBack}
      onsessionclear={handleSessionClear}
    />
    <div class="flex-1 flex overflow-hidden h-full">
      {#if contract}
        <FunctionSidebar {contract} {canvasFuncs} {mode} onadd={handleSidebarAdd} onremove={removeFuncFromCanvas} />
      {/if}

      <GraphCanvasFlow
        bind:this={graphCanvas}
        onnodetap={(node, event) => handleNodeClick(node, event)}
        onbackgroundtap={handleBackgroundTap}
        oncontextmenu={handleContextMenu}
        onnodesdelete={handleNodesDelete}
        canDeleteNodes={mode !== 'session'}
        onselectionchange={(nodes) => { selectionCount = nodes.length; }}
        onready={(api) => { flowApi = api; }}
      />

      {#if contract}
        <SessionSidebar
          contract={contract.name}
          {selectedNode}
          {selectedPath}
          {funcPaths}
          {expandedFuncs}
          {seqExpanded}
          {mode}
          {seqAnalysis}
          contractDetail={{ name: contract.name, functions: contract.functions }}
          lookupBlock={(blockId) => {
            const node = findNode(blockId);
            if (!node || node.data._type !== 'block') return null;
            return { statements: (node.data as any).statements ?? [], node_type: (node.data as any).node_type };
          }}
          onpathselect={(funcName, path) => { selectedPath = path; highlightPath(funcName, path); }}
          onexpandcfg={(funcName, nodeId) => toggleFuncExpand(funcName, nodeId)}
        />
      {/if}
    </div>

    <StatusBar
      {mode}
      canvasCount={canvasFuncs.size}
      expandedCount={expandedFuncs.size}
      activeScenario={getActiveScenario()}
      {selectionCount}
    />

    <ContextMenu
      menu={contextMenu}
      {expandedFuncs}
      {seqExpanded}
      {mode}
      onexpandcfg={(func, nodeId) => { toggleFuncExpand(func, nodeId); contextMenu = null; }}
      onremovefunc={(func) => { removeFuncFromCanvas(func); contextMenu = null; selectedNode = null; }}
      onremovenode={(nodeId) => { removeSeqNode(nodeId); contextMenu = null; selectedNode = null; }}
      onforkscenario={handleForkScenario}
      onremovefromhere={handleRemoveFromHere}
      onviewsource={handleViewSource}
      onopenide={handleOpenInIde}
      onclose={() => contextMenu = null}
    />

    {#if sourcePanel && contract}
      <FunctionSourcePanel
        contract={contract.name}
        func={sourcePanel.func}
        onclose={() => sourcePanel = null}
      />
    {/if}

    <Legend {mode} />
  {/if}
  <ErrorToasts />
</div>

