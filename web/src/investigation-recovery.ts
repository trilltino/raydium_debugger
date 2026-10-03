/** Cursor state is local to this operator and API origin. IDs remain decimal strings. */
import type { InvestigationEvent } from './types';

export interface ActiveInvestigation {
  id: string;
  after: string;
}

const KEY = `raydium-investigation:${import.meta.env.VITE_DEBUGGER_API_BASE ?? 'local'}`;

export function activeInvestigation(): ActiveInvestigation | null {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? 'null') as ActiveInvestigation | null;
    return value && typeof value.id === 'string' && /^\d+$/.test(value.after) ? value : null;
  } catch { return null; }
}

/** Persist the cursor only when an event is delivered, and ignore duplicate replay. */
export function deliverInvestigationEvent(event: InvestigationEvent, listener: (event: InvestigationEvent) => void): void {
  const previous = activeInvestigation();
  if (event.event_id && previous?.id === event.investigation_id && BigInt(event.event_id) <= BigInt(previous.after)) return;
  listener(event);
  try {
    localStorage.setItem(KEY, JSON.stringify({ id: event.investigation_id, after: event.event_id ?? '0' }));
  } catch { /* Storage may be disabled; durable server replay remains available. */ }
}
