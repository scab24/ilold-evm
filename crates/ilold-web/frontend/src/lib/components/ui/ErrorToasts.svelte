<script lang="ts">
  import { getErrorNotices, dismissError } from './notifications.svelte';

  const notices = $derived(getErrorNotices());
</script>

{#if notices.length > 0}
  <div class="fixed top-14 left-1/2 -translate-x-1/2 z-[60] flex flex-col gap-2 w-[420px] max-w-[90vw]" role="alert" aria-live="assertive">
    {#each notices as notice (notice.id)}
      <div
        class="error-toast flex items-start gap-2 px-3 py-2 text-[12px] text-text"
        style="
          border-radius: 8px;
          border: 1px solid color-mix(in srgb, var(--color-danger) 60%, transparent);
          background: color-mix(in srgb, var(--color-danger) 14%, rgba(20, 20, 26, 0.95));
        "
      >
        <div class="flex-1 min-w-0">
          <div class="text-danger font-semibold">{notice.label} failed</div>
          <div class="break-words">{notice.message}</div>
        </div>
        <button
          class="bg-transparent border-none text-text-muted cursor-pointer hover:text-text leading-none"
          aria-label="Dismiss error"
          onclick={() => dismissError(notice.id)}
        >✕</button>
      </div>
    {/each}
  </div>
{/if}
