
import { postCommand, type ScenarioAction } from '$lib/api/session';
import { notifyError } from '$lib/components/ui/notifications.svelte';

export async function dispatchScenarioAction(
  action: ScenarioAction,
  contract: string | undefined,
  label: string,
): Promise<void> {
  try {
    await postCommand({ Scenario: { sub: action } }, contract);
  } catch (e) {
    notifyError(`Scenario ${label}`, e);
  }
}
