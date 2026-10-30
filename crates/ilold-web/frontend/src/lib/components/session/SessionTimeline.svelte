<script lang="ts">
  import { getSteps, getHighlightedFunction } from '$lib/stores/session.svelte';
  import { getStepNarrative, postCommand } from '$lib/api/session';
  import { formatAccess } from '$lib/utils/access';
  import { notifyError } from '$lib/components/ui/notifications.svelte';
  import type { AccessLevel, FunctionNarrative, TransitiveEffect } from '$lib/api/types';

  interface Props {
    contract: string;
  }

  let { contract }: Props = $props();

  let expandedStep = $state<number | null>(null);
  let narrative = $state<FunctionNarrative | null>(null);
  let narrativeError = $state<string | null>(null);
  let loadingNarrative = $state(false);
  let backBusy = $state(false);
  let narrativeGen = 0;
  let scrollContainer: HTMLDivElement | undefined = $state();

  const steps = $derived(getSteps());
  const highlighted = $derived(getHighlightedFunction());

  let prevLength = 0;
  $effect(() => {
    const len = steps.length;
    if (len > prevLength && scrollContainer) {
      requestAnimationFrame(() => {
        scrollContainer!.scrollTop = scrollContainer!.scrollHeight;
      });
    }
    prevLength = len;
  });

  function accessBadge(access: AccessLevel): { text: string; cls: string } {
    const label = formatAccess(access);
    if (label === 'Public') return { text: label, cls: 'badge-public' };
    if (label === 'Internal') return { text: label, cls: 'badge-internal' };
    if (label.startsWith('Restricted')) return { text: label, cls: 'badge-restricted' };
    if (label.startsWith('Special')) return { text: label, cls: 'badge-special' };
    return { text: label, cls: 'badge-internal' };
  }

  async function toggleNarrative(stepIndex: number) {
    if (expandedStep === stepIndex) {
      expandedStep = null;
      narrative = null;
      narrativeError = null;
      return;
    }
    expandedStep = stepIndex;
    narrative = null;
    narrativeError = null;
    loadingNarrative = true;
    const gen = ++narrativeGen;
    try {
      const res = await getStepNarrative(stepIndex, contract);
      if (gen !== narrativeGen) return; // stale
      narrative = res;
    } catch (e: unknown) {
      if (gen !== narrativeGen) return;
      narrativeError = e instanceof Error ? e.message : String(e);
    } finally {
      if (gen === narrativeGen) loadingNarrative = false;
    }
  }

  interface NarrativeSection {
    label: string;
    tone: 'muted' | 'write' | 'call' | 'event';
    items: string[];
  }

  function viaLabel(effects: TransitiveEffect[] | undefined): string[] {
    return (effects ?? []).map((e) => `${e.item}  via ${e.via.join(' → ')}`);
  }

  // Non-empty narrative sections in reading order: what the step changes first.
  function sections(n: FunctionNarrative): NarrativeSection[] {
    const all: NarrativeSection[] = [
      { label: 'Writes', tone: 'write', items: n.state_writes },
      { label: 'Writes via calls', tone: 'write', items: viaLabel(n.transitive_state_writes) },
      { label: 'External calls', tone: 'call', items: [...n.external_calls, ...viaLabel(n.transitive_external_calls)] },
      { label: 'Internal calls', tone: 'muted', items: n.internal_calls ?? [] },
      { label: 'Reads', tone: 'muted', items: [...n.state_reads, ...viaLabel(n.transitive_state_reads)] },
      { label: 'Events', tone: 'event', items: [...(n.events ?? []), ...viaLabel(n.transitive_events)] },
    ];
    return all.filter((s) => s.items.length > 0);
  }

  async function goBack() {
    if (backBusy) return;
    backBusy = true;
    try {
      await postCommand('Back', contract);
    } catch (e) {
      notifyError('session back', e);
    } finally {
      backBusy = false;
    }
  }
</script>

<div class="flex flex-col max-h-full h-full overflow-hidden" style="background: transparent;">
  <!-- Header — glass effect -->
  <div
    class="flex items-center justify-between px-3 py-2.5 flex-shrink-0"
    style="
      border-bottom: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);
      background: linear-gradient(180deg, rgba(30, 30, 40, 0.85) 0%, rgba(24, 24, 30, 0.9) 100%);
      backdrop-filter: blur(16px) saturate(1.8);
      -webkit-backdrop-filter: blur(16px) saturate(1.8);
    "
  >
    <span class="text-[10px] text-text-muted uppercase tracking-wider font-semibold">Session Timeline</span>
    <span
      class="text-[10px] text-text-dim font-mono px-2 py-0.5"
      style="border-radius: 8px; background: color-mix(in srgb, var(--color-border) 30%, transparent);"
    >{steps.length}</span>
  </div>

  <!-- Body -->
  <div class="timeline-body flex-1 overflow-y-auto py-1.5" bind:this={scrollContainer}>
    {#if steps.length === 0}
      <div class="py-8 px-4 text-text-dim text-[11px] text-center leading-relaxed" style="font-style: italic;">
        No steps yet. Click a function in the sidebar or run `c &lt;name&gt;` in the terminal.
      </div>
    {:else}
      <ol class="list-none m-0 p-0 px-1">
        {#each steps as step (step.step_index)}
          {@const badge = accessBadge(step.access)}
          {@const isHighlighted = highlighted === step.function}
          {@const isExpanded = expandedStep === step.step_index}
          <li
            class="mb-0.5 transition-all duration-150"
            style="
              border-radius: 8px;
              border-left: 3px solid {isHighlighted ? 'var(--color-accent)' : 'transparent'};
              background: {isExpanded ? 'color-mix(in srgb, var(--color-accent) 5%, transparent)' : 'transparent'};
            "
          >
            <button
              class="flex items-center gap-1.5 w-full px-2.5 py-2 bg-transparent border-none text-text text-[11px] font-mono cursor-pointer text-left transition-colors duration-150 hover:text-accent-hover"
              style="border-radius: 8px;"
              onclick={() => toggleNarrative(step.step_index)}
            >
              <span class="text-text-dim text-[9px] min-w-[16px] text-right">{step.step_index}</span>
              <span class="flex-1 overflow-hidden text-ellipsis whitespace-nowrap">{step.function}</span>
              <span class="badge {badge.cls}">{badge.text}</span>
              <span class="text-text-dim text-[9px] flex-shrink-0">{isExpanded ? '▾' : '▸'}</span>
            </button>

            {#if isExpanded}
              <div
                class="py-2 pr-3 pl-7"
                style="
                  border-top: 1px solid color-mix(in srgb, var(--color-border) 25%, transparent);
                  background: color-mix(in srgb, var(--color-surface) 40%, transparent);
                  border-radius: 0 0 8px 8px;
                "
              >
                {#if loadingNarrative}
                  <span class="text-text-dim text-[10px] italic">Loading...</span>
                {:else if narrativeError}
                  <span class="text-danger text-[11px]">{narrativeError}</span>
                {:else if narrative}
                  <div class="narrative text-[11px] leading-relaxed" data-testid="step-narrative">
                    <div class="text-text">
                      <span class="font-mono">{narrative.name}</span>
                      <span class="text-text-muted">· {formatAccess(narrative.access)}</span>
                      {#if narrative.modifiers.length > 0}
                        <span class="text-text-dim">· {narrative.modifiers.join(', ')}</span>
                      {/if}
                    </div>
                    <div class="text-text-muted">
                      {narrative.total_paths} path(s):
                      <span class="text-success">{narrative.happy_paths} return</span>,
                      <span class="text-danger">{narrative.revert_paths} revert</span>
                    </div>
                    {#each sections(narrative) as section}
                      <div class="narr-label">{section.label}</div>
                      <ul class="narr-list tone-{section.tone}">
                        {#each section.items as item}
                          <li class="font-mono break-words">{item}</li>
                        {/each}
                      </ul>
                    {/each}
                    {#if narrative.observations.length > 0}
                      <div class="narr-label">Observations</div>
                      <ul class="narr-list tone-call">
                        {#each narrative.observations as o}
                          <li>{o.description}</li>
                        {/each}
                      </ul>
                    {/if}
                    {#if sections(narrative).length === 0 && narrative.observations.length === 0}
                      <div class="text-text-dim italic">No state, call or event effects.</div>
                    {/if}
                  </div>
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ol>
    {/if}
  </div>

  <!-- Footer -->
  {#if steps.length > 0}
    <div class="px-2.5 py-2 flex-shrink-0" style="border-top: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);">
      <button
        class="w-full py-1.5 border text-text-muted text-[11px] font-mono cursor-pointer transition-colors duration-150 hover:text-text hover:border-accent"
        style="
          border-radius: 8px;
          border-color: color-mix(in srgb, var(--color-border) 50%, transparent);
          background: color-mix(in srgb, var(--color-surface) 30%, transparent);
          backdrop-filter: blur(8px);
        "
        onclick={goBack}
        disabled={backBusy}
      >
        ← Back
      </button>
    </div>
  {/if}
</div>

<style>
  .badge {
    font-size: 9px;
    padding: 2px 6px;
    border-radius: 8px;
    font-family: system-ui, sans-serif;
    flex-shrink: 0;
    text-transform: lowercase;
  }

  .badge-public {
    color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 12%, transparent);
  }

  .badge-internal {
    color: var(--color-text-muted);
    background: color-mix(in srgb, var(--color-text-muted) 12%, transparent);
  }

  .badge-restricted {
    color: var(--color-orange);
    background: color-mix(in srgb, var(--color-orange) 12%, transparent);
  }

  .badge-special {
    color: var(--color-purple);
    background: color-mix(in srgb, var(--color-purple) 12%, transparent);
  }

  .narr-label {
    margin-top: 6px;
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-dim);
  }
  .narr-list { list-style: none; margin: 2px 0 0; padding: 0; }
  .narr-list li { padding-left: 8px; border-left: 2px solid var(--color-border); }
  .tone-muted li { color: var(--color-text-muted); }
  .tone-write li { color: var(--color-accent); border-color: var(--color-accent); }
  .tone-call li { color: var(--color-danger); border-color: var(--color-danger); }
  .tone-event li { color: var(--color-success); border-color: var(--color-success); }

  .timeline-body::-webkit-scrollbar { width: 4px; }
  .timeline-body::-webkit-scrollbar-track { background: transparent; }
  .timeline-body::-webkit-scrollbar-thumb { background: var(--color-border); border-radius: 2px; }
</style>
