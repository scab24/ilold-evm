
import { postCommand, type ScenarioAction } from '$lib/api/session';
import { notifyError } from '$lib/components/ui/notifications.svelte';

export async function dispatchScenarioAction(
  action: ScenarioAction,
  contract: string | undefined,
  label: string,
): Promise<void> {
  // Deleting drops the scenario's steps, so it asks first like Clear does.
  if (typeof action === 'object' && 'Delete' in action
    && !confirm(`Delete scenario "${action.Delete.name}" and its steps?`)) return;
  try {
    await postCommand({ Scenario: { sub: action } }, contract);
  } catch (e) {
    notifyError(`Scenario ${label}`, e);
  }
}
