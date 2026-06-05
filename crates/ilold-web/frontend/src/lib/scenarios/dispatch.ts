
import { postCommand, type ScenarioAction } from '$lib/api/session';

export async function dispatchScenarioAction(
  action: ScenarioAction,
  contract: string | undefined,
  label: string,
): Promise<void> {
  try {
    await postCommand({ Scenario: { sub: action } }, contract);
  } catch (e) {
    const reason = e instanceof Error ? e.message : String(e);
    console.warn(`scenario ${label} failed:`, e);
    alert(`Scenario ${label} failed:\n\n${reason}`);
  }
}
