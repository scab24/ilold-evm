export interface ErrorNotice {
  id: number;
  label: string;
  message: string;
}

const AUTO_DISMISS_MS = 8000;

let notices = $state<ErrorNotice[]>([]);
let nextId = 0;

export function getErrorNotices(): ErrorNotice[] {
  return notices;
}

export function dismissError(id: number): void {
  notices = notices.filter((n) => n.id !== id);
}

/** Show a failed action to the user (non-blocking) and log it. */
export function notifyError(label: string, e: unknown): void {
  const message = e instanceof Error ? e.message : String(e);
  console.warn(`${label} failed:`, e);
  const id = ++nextId;
  notices = [...notices, { id, label, message }];
  setTimeout(() => dismissError(id), AUTO_DISMISS_MS);
}
