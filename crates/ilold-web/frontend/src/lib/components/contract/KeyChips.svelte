<script lang="ts">
  import type { AccessKey } from '$lib/api/rest';
  import { KEY_KIND_LABEL } from '$lib/utils/keys';

  let { keys }: { keys: AccessKey[] } = $props();
</script>

{#each keys as key}
  <span class="key" title={key.ref ?? ''} data-testid="access-key">
    <span class="key-text">{key.text}</span>
    <span class="key-kind kind-{key.kind}">{KEY_KIND_LABEL[key.kind]}</span>
  </span>
{/each}

<style>
  .key { display: inline-flex; align-items: center; gap: 3px; margin-right: 4px; }
  .key-text { font-family: 'JetBrains Mono', monospace; color: var(--color-text); }
  .key-kind { font-size: 9px; padding: 0 4px; border-radius: 6px; background: color-mix(in srgb, var(--color-text-dim) 15%, transparent); color: var(--color-text-muted); }
  .kind-msg_sender { background: color-mix(in srgb, var(--color-success) 15%, transparent); color: var(--color-success); }
  .kind-parameter { background: color-mix(in srgb, var(--color-accent) 15%, transparent); color: var(--color-accent-hover); }
  .kind-state_var, .kind-this, .kind-literal { background: color-mix(in srgb, var(--color-warning) 15%, transparent); color: var(--color-warning); }
</style>
