import type { CatchClauseFact, CatchKind, TryCatchFact } from '$lib/api/rest';

export const SWALLOWED = 'failure swallowed';

const CATCH_LABEL: Record<CatchKind, string> = {
  Error: 'catch Error',
  Panic: 'catch Panic',
  bytes: 'catch (bytes)',
  all: 'catch (all)',
};

export function catchLabel(clause: CatchClauseFact): string {
  return clause.kind ? CATCH_LABEL[clause.kind] : 'catch ?';
}

/** The try statement and the clause each CFG block plays a part in, by block id. */
export interface TryRoles {
  guards: Map<string, TryCatchFact>;
  clauses: Map<string, CatchClauseFact>;
  /** `source->target` of each branch, with its label. */
  branches: Map<string, string>;
}

// the CFG labels a bare catch and a bytes catch alike; the fact tells them apart
export function tryRoles(facts: TryCatchFact[] = []): TryRoles {
  const roles: TryRoles = { guards: new Map(), clauses: new Map(), branches: new Map() };
  for (const fact of facts) {
    const from = `b${fact.block}`;
    roles.guards.set(from, fact);
    roles.branches.set(`${from}->b${fact.success_block}`, 'success');
    for (const clause of fact.catch_clauses) {
      roles.clauses.set(`b${clause.block}`, clause);
      roles.branches.set(`${from}->b${clause.block}`, catchLabel(clause));
    }
  }
  return roles;
}

/** One line per try statement, for a tooltip. */
export function describeTries(facts: TryCatchFact[]): string {
  return facts
    .map((f) => {
      const clauses = f.catch_clauses.map((c) => (c.empty ? `${catchLabel(c)} (${SWALLOWED})` : catchLabel(c))).join(', ');
      return `try ${f.guarded_call?.text ?? '?'}: ${clauses}`;
    })
    .join('\n');
}
