<script lang="ts">
  import type { AssignmentSite, Connections, ContractCall, ContractDetail } from '$lib/api/rest';
  import Icon, { type IconName } from '$lib/components/chrome/Icon.svelte';
  import Row from '$lib/components/chrome/Row.svelte';
  import { openFunctionLine, openInIde } from '$lib/utils/ide-links';
  import { BASIS_LABEL, splitRef } from '$lib/utils/links';
  import { assetLabel } from '$lib/utils/value';
  import { shortContract } from './connections/board-links';

  let { contract, member, connections, detail, main, titled, onopen, onsource }: {
    contract: string;
    titled: boolean;
    member: string;
    connections: Connections;
    detail: ContractDetail | null;
    main: string;
    onopen: (contract: string) => void;
    onsource: (contract: string, func: string) => void;
  } = $props();

  interface Listed {
    id: string;
    row: ContractCall;
    other: string;
    otherFn: string;
    out: boolean;
  }

  const name = (ref: string) => splitRef(ref).member.split('(')[0];
  const mine = (ref: string) => splitRef(ref).member === member;
  const summary = $derived(detail ? [...detail.functions, ...(detail.inherited_functions ?? [])].find((f) => f.key === member) ?? null : null);

  const outgoing = $derived(
    connections.outgoing.flatMap((g) =>
      g.calls.filter((r) => r.in_context === contract && mine(r.function) && g.contract !== contract).map((row) => {
        const callee = row.callees.find((d) => d.contract === g.contract);
        return { id: `o|${g.contract}|${row.function}|${row.span.start_line}:${row.span.start_col}`, row, other: g.contract, otherFn: callee ? name(callee.function) : name(row.target ?? ''), out: true };
      }),
    ),
  );
  const incoming = $derived(
    connections.incoming.flatMap((g) =>
      g.calls.filter((r) => r.callees.some((d) => d.contract === contract && mine(d.function))).map((row) => ({ id: `i|${row.in_context}|${row.function}|${row.span.start_line}:${row.span.start_col}`, row, other: row.in_context, otherFn: name(row.function), out: false })),
    ),
  );
  const self = $derived(connections.outgoing.filter((g) => g.contract === contract).flatMap((g) => g.calls.filter((r) => mine(r.function))));
  const unresolved = $derived(connections.unresolved.filter((r) => mine(r.function)));
  const sections = $derived([
    { title: 'Calls', icon: 'call' as IconName, list: outgoing.filter((l) => l.row.kind === 'call') },
    { title: 'Called by', icon: 'entry' as IconName, list: incoming.filter((l) => l.row.kind !== 'read') },
    { title: 'Reads', icon: 'read' as IconName, list: outgoing.filter((l) => l.row.kind === 'read') },
    { title: 'Read by', icon: 'read' as IconName, list: incoming.filter((l) => l.row.kind === 'read') },
    { title: 'Library calls', icon: 'lib' as IconName, list: outgoing.filter((l) => l.row.kind === 'library') },
  ]);
  const transfers = $derived(summary?.value_transfers ?? []);

  function assignment(row: ContractCall): AssignmentSite | null {
    return row.via ? (connections.assignments.find((a) => a.state_var === row.via) ?? null) : null;
  }
  function describe(l: Listed): string {
    const parts: string[] = [];
    const callees = l.row.callees.filter((d) => d.contract === (l.out ? l.other : contract));
    const basis = callees[0]?.basis;
    if (basis === 'inherits_interface') parts.push(BASIS_LABEL.inherits_interface);
    else if (basis === 'selector_match') parts.push(BASIS_LABEL.selector_match);
    if (l.row.via) parts.push(`address in ${splitRef(l.row.via).member}`);
    if (l.row.callees.some((d) => d.tag === 'AMBIGUOUS')) parts.push(`${l.row.callees.length} possible · AMBIGUOUS`);
    if (l.row.in_try) parts.push('inside try');
    return parts.join(' · ');
  }
  const source = (row: ContractCall) => (row.file ? () => openInIde(row.file!, row.span.start_line) : undefined);
</script>

{#if titled}
  <h1 class="mono">{member}</h1>
  <p class="sub">{contract}{summary ? ` · ${summary.visibility.toLowerCase()} · ${summary.mutability.toLowerCase()}` : ''}</p>
{/if}
{#if contract !== main}
  <div class="actions"><button type="button" class="btn" onclick={() => onopen(contract)} title="Make {contract} the main contract"><Icon name="out" />Open {contract}</button></div>
{/if}

{#each sections as s (s.title)}
  {#if s.list.length}
    <h2><Icon name={s.icon} /><span class="ht">{s.title}</span><span class="n">{s.list.length}</span></h2>
    <ul class="list">
      {#each s.list as l (l.id)}
        {@const site = assignment(l.row)}
        <Row icon={l.row.transfers.length ? 'value' : s.icon} tone={l.row.callees.some((d) => d.tag === 'AMBIGUOUS') ? 'amb' : l.row.transfers.length ? 'val' : ''} title={l.out ? `${name(l.row.function)} → ${shortContract(l.other)}.${l.otherFn}` : `${shortContract(l.other)}.${l.otherFn} → ${member.split('(')[0]}`} line={l.row.span.start_line} onsource={source(l.row)}>
          {#snippet meta()}{describe(l)}{#if l.row.transfers.length}{describe(l) ? ' · ' : ''}<span class="val">moves value</span>{/if}{#if site}{describe(l) || l.row.transfers.length ? ' · ' : ''}set by {site.kind.replaceAll('_', ' ')} <span class="mono">{site.expr_text}</span>{#if site.function}{' '}<button type="button" class="src" onclick={() => onsource(splitRef(site.function!).contract, splitRef(site.function!).member)}>{name(site.function)} ↗</button>{/if}{/if}{/snippet}
        </Row>
      {/each}
    </ul>
  {/if}
{/each}

{#if self.length}
  <h2><Icon name="loop" /><span class="ht">Calls itself through this</span><span class="n">{self.length}</span></h2>
  <ul class="list">
    {#each self as row (`${row.function}|${row.span.start_line}:${row.span.start_col}`)}
      <Row icon="loop" title="{name(row.function)} → this.{name(row.callees[0]?.function ?? row.target ?? '')}" line={row.span.start_line} onsource={source(row)} />
    {/each}
  </ul>
{/if}

{#if transfers.length}
  <h2><Icon name="value" /><span class="ht">Value</span><span class="n">{transfers.length}</span></h2>
  <ul class="list">
    {#each transfers as t (t.id)}
      <Row icon="value" tone="val" title="{t.operation} {assetLabel(t)}{t.to_expr ? ` to ${t.to_expr}` : ''}" line={t.span.start_line} onsource={() => openFunctionLine(t.function, t.span.start_line)}>
        {#snippet meta()}{t.amount_expr ? `amount ${t.amount_expr}` : 'amount not recorded'}{t.via_library ? ` · through ${t.via_library}` : ''}{/snippet}
      </Row>
    {/each}
  </ul>
{/if}

{#if unresolved.length}
  <h2 class="amb"><Icon name="unres" /><span class="ht">Unresolved</span><span class="n">{unresolved.length}</span></h2>
  <ul class="list">
    {#each unresolved as row (`${row.function}|${row.span.start_line}:${row.span.start_col}`)}
      <Row icon="unres" tone="amb" title="{name(row.function)} → {(row.target ?? '').replace('::', '.')}" line={row.span.start_line} onsource={source(row)}>
        {#snippet meta()}<span class="amb">{row.unresolved_reason === 'dynamic_address' ? 'the call names no declaration' : `no project contract implements ${splitRef(row.target ?? '').contract}`}</span>{/snippet}
      </Row>
    {/each}
  </ul>
{/if}

{#if !outgoing.length && !incoming.length && !self.length && !unresolved.length}
  <p class="note">No calls to or from other contracts.</p>
{/if}
