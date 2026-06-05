
export const SCENARIO_NAME_REGEX = /^[a-z][a-z0-9_-]{0,31}$/;

const DEFAULT_LABEL = 'New scenario name (lowercase, a-z 0-9 _ -, max 32):';

export function promptScenarioName(label: string = DEFAULT_LABEL): string | null {
  const raw = prompt(label);
  if (raw === null) return null;
  const name = raw.trim();
  if (!SCENARIO_NAME_REGEX.test(name)) {
    alert(`Invalid scenario name "${name}".\n\nMust be lowercase a-z, numbers, _ or -. Start with a letter. Max 32 chars.`);
    return null;
  }
  return name;
}
