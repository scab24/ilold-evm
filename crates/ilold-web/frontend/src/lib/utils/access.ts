import type { AccessLevel, GuardFact } from '$lib/api/types';

export function formatAccess(access: AccessLevel): string {
  if (typeof access === 'string') return access;
  if ('Restricted' in access) return `Restricted(${access.Restricted.role ?? 'unknown'})`;
  if ('Special' in access) return `Special(${access.Special.kind ?? 'unknown'})`;
  return 'Unknown';
}

// Pause, reentrancy and initializer guards restrict when a function runs, not who calls it.
export function isAccessGuard(guard: GuardFact): boolean {
  return guard.kind === 'CallerEquals' || guard.kind === 'RoleCheck' || guard.kind === 'TxOrigin' || guard.kind === 'AccessByName';
}

export function isConditional(guard: GuardFact): boolean {
  return guard.scope === 'Conditional';
}

/** Only an access check on every path that must hold restricts who may call, as the analysis decides access. */
export function restricts(guard: GuardFact): boolean {
  return (guard.requires_caller ?? isAccessGuard(guard)) && !isConditional(guard);
}

export function describeGuard(guard: GuardFact): string {
  return `${guard.kind}${isConditional(guard) ? ' (conditional)' : ''}: ${guard.condition}`;
}
