import { activeInvestigation, deliverInvestigationEvent } from './investigation-recovery';
import type {
  AiAskRequest,
  AiResponse,
  CasebookRecord,
  CreateCasebookRequest,
  CreateIntegratorRequest,
  DebugRequest,
  DebugResponse,
  DiagnosticResponse,
  IntegratorRecord,
  InvestigationEvent,
  InvestigationLookup,
  InvestigationRequest,
  InvestigationResult,
  ProviderStatus,
  RecentObservationSummary,
  SaveSignatureRequest,
} from './types';

const API_BASE = (import.meta.env.VITE_DEBUGGER_API_BASE ?? '').replace(/\/+$/, '');

function isTauri(): boolean {
  return typeof window !== 'undefined' && window.__TAURI_INTERNALS__ !== undefined;
}

async function invokeCommand<T>(command: string, payload: unknown): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, payload as Record<string, unknown>);
}

let sessionToken: string | null = null;

async function apiToken(): Promise<string> {
  if (sessionToken) return sessionToken;
  const response = await fetch(`${API_BASE}/api/session`);
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  const payload = (await response.json()) as { api_token?: string };
  if (!payload.api_token) {
    throw new Error('Local API session did not provide a token');
  }
  sessionToken = payload.api_token;
  return sessionToken;
}

async function postJson<T>(path: string, body: unknown): Promise<T> {
  const token = await apiToken();
  const response = await fetch(`${API_BASE}${path}`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'x-raydium-debugger-token': token,
    },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      const payload = (await response.json()) as {
        error?: string;
        error_kind?: string;
        hintless_details?: string;
      };
      const kind = payload.error_kind ? `[${payload.error_kind}] ` : '';
      const details =
        payload.hintless_details && payload.hintless_details !== payload.error
          ? `\n${payload.hintless_details}`
          : '';
      message = `${kind}${payload.error ?? message}${details}`;
    } catch {
      /* use status text */
    }
    throw new Error(message);
  }
  return response.json() as Promise<T>;
}

async function getJson<T>(path: string): Promise<T> {
  const token = await apiToken();
  const response = await fetch(`${API_BASE}${path}`, {
    headers: {
      'x-raydium-debugger-token': token,
    },
  });
  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      const payload = (await response.json()) as {
        error?: string;
        error_kind?: string;
        hintless_details?: string;
      };
      const kind = payload.error_kind ? `[${payload.error_kind}] ` : '';
      message = `${kind}${payload.error ?? message}`;
    } catch {
      /* use status text */
    }
    throw new Error(message);
  }
  return response.json() as Promise<T>;
}

export async function debugTransaction(request: DebugRequest): Promise<DebugResponse> {
  if (isTauri()) {
    return invokeCommand<DebugResponse>('debug_transaction_cmd', { request });
  }
  return postJson<DebugResponse>('/api/debug', request);
}

export async function diagnoseTransaction(request: DebugRequest): Promise<DiagnosticResponse> {
  if (isTauri()) {
    const legacy = await invokeCommand<DebugResponse>('debug_transaction_cmd', { request });
    return {
      observation: {
        status: 'landed',
        cluster: legacy.info.provider.cluster,
        providers_queried: [legacy.info.provider.rpc_endpoint_redacted].filter(Boolean),
        evidence: [`Transaction was fetched at slot ${legacy.info.slot_exact}.`],
        hypotheses: [],
      },
      diagnosis: {
        title: legacy.info.failure?.plain_title ?? legacy.info.experience.headline,
        explanation: legacy.info.failure?.plain_explanation ?? legacy.info.experience.message,
        primary_action: legacy.info.failure?.primary_action ?? legacy.info.experience.next_step,
        evidence: legacy.info.failure?.evidence_summary.length
          ? legacy.info.failure.evidence_summary
          : legacy.info.root_cause.evidence,
        confidence: legacy.info.failure?.confidence ?? 'medium',
        category: legacy.info.failure?.category ?? legacy.info.root_cause.category,
        copy_markdown: legacy.formatted_text,
      },
      transaction: legacy.info,
      formatted_text: legacy.formatted_text,
    };
  }
  return postJson<DiagnosticResponse>('/api/diagnose', request);
}

export async function investigate(
  request: InvestigationRequest,
  onEvent: (event: InvestigationEvent) => void,
): Promise<InvestigationResult> {
  if (isTauri()) {
    const { Channel } = await import('@tauri-apps/api/core');
    const onProgress = new Channel<InvestigationEvent>();
    onProgress.onmessage = (event) => deliverInvestigationEvent(event, onEvent);
    return invokeCommand<InvestigationResult>('investigate_cmd', { request, onProgress });
  }

  const token = await apiToken();
  const response = await fetch(`${API_BASE}/api/investigate`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'x-raydium-debugger-token': token,
      accept: 'text/event-stream',
    },
    body: JSON.stringify(request),
  });
  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      const payload = (await response.json()) as { error?: string; error_kind?: string };
      message = `${payload.error_kind ? `[${payload.error_kind}] ` : ''}${payload.error ?? message}`;
    } catch {
      /* use status text */
    }
    throw new Error(message);
  }
  return consumeInvestigationStream(response, onEvent);
}

async function consumeInvestigationStream(response: Response, onEvent: (event: InvestigationEvent) => void): Promise<InvestigationResult> {
  if (!response.body) throw new Error('Investigation stream is unavailable');
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let result: InvestigationResult | null = null;
  let investigationId: string | null = null;
  let streamError: unknown = null;
  try {
    while (true) {
      const chunk = await reader.read();
      buffer += decoder.decode(chunk.value, { stream: !chunk.done });
      buffer = buffer.replace(/\r\n/g, '\n');
      let boundary = buffer.indexOf('\n\n');
      while (boundary >= 0) {
        const block = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const data = block.split('\n').filter((line) => line.startsWith('data:')).map((line) => line.slice(5).trimStart()).join('\n');
        if (data) {
          const event = JSON.parse(data) as InvestigationEvent;
          investigationId = event.investigation_id;
          deliverInvestigationEvent(event, onEvent);
          if (event.result) result = event.result;
          if (event.error) throw new Error(event.error);
        }
        boundary = buffer.indexOf('\n\n');
      }
      if (result || chunk.done) break;
    }
  } catch (error) { streamError = error; }
  finally { await reader.cancel().catch(() => undefined); }
  if (result) return result;
  if (investigationId) {
    const lookup = await lookupInvestigation(investigationId).catch(() => null);
    if (lookup?.result) return lookup.result;
    if (lookup?.status === 'running') return resumeInvestigation(investigationId, onEvent);
  }
  if (streamError instanceof Error) throw streamError;
  throw new Error('Investigation stream ended before a result was received');
}

export async function lookupInvestigation(id: string): Promise<InvestigationLookup | null> {
  if (isTauri()) return invokeCommand('investigation_lookup_cmd', { id });
  return getJson(`/api/investigations/${encodeURIComponent(id)}`);
}

/** Resume persisted progress; reconnecting never starts another RPC job. */
export async function resumeInvestigation(id: string, onEvent: (event: InvestigationEvent) => void): Promise<InvestigationResult> {
  const lookup = await lookupInvestigation(id);
  if (lookup?.result) return lookup.result;
  if (!lookup || lookup.status !== 'running') throw new Error(`Investigation ${lookup?.status ?? 'unavailable'}. Retry explicitly to start a new investigation.`);
  if (isTauri()) {
    while (true) {
      const after = activeInvestigation()?.id === id ? activeInvestigation()!.after : '0';
      const events = await invokeCommand<InvestigationEvent[]>('investigation_events_cmd', { id, after });
      for (const event of events) {
        deliverInvestigationEvent(event, onEvent);
        if (event.result) return event.result;
        if (event.error) throw new Error(event.error);
      }
      const current = await lookupInvestigation(id);
      if (current?.result) return current.result;
      if (!current || current.status !== 'running') throw new Error(`Investigation ${current?.status ?? 'unavailable'}. Retry explicitly.`);
      if (events.length < 128) await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  const after = activeInvestigation()?.id === id ? activeInvestigation()!.after : '0';
  const response = await fetch(`${API_BASE}/api/investigations/${encodeURIComponent(id)}/events?after=${encodeURIComponent(after)}`, {
    headers: { 'x-raydium-debugger-token': await apiToken(), accept: 'text/event-stream' },
  });
  if (!response.ok) throw new Error(`Progress recovery failed: ${response.status}`);
  return consumeInvestigationStream(response, onEvent);
}

/** Explicit operator retry, with a new durable identity and acceptance time. */
export async function retryInvestigation(id: string, onEvent: (event: InvestigationEvent) => void): Promise<InvestigationResult> {
  if (isTauri()) {
    const { Channel } = await import('@tauri-apps/api/core');
    const onProgress = new Channel<InvestigationEvent>();
    onProgress.onmessage = (event) => deliverInvestigationEvent(event, onEvent);
    return invokeCommand('investigation_retry_cmd', { id, onProgress });
  }
  const response = await fetch(`${API_BASE}/api/investigations/${encodeURIComponent(id)}/retry`, {
    method: 'POST', headers: { 'x-raydium-debugger-token': await apiToken(), accept: 'text/event-stream' },
  });
  if (!response.ok) throw new Error(`Retry failed: ${response.status}`);
  return consumeInvestigationStream(response, onEvent);
}

export async function listIntegrators(): Promise<IntegratorRecord[]> {
  if (isTauri()) return [];
  return getJson<IntegratorRecord[]>('/api/integrators');
}

export async function createIntegrator(request: CreateIntegratorRequest): Promise<IntegratorRecord> {
  return postJson<IntegratorRecord>('/api/integrators', request);
}

export async function saveIntegratorSignature(
  integratorId: string,
  request: SaveSignatureRequest,
): Promise<IntegratorRecord> {
  return postJson<IntegratorRecord>(`/api/integrators/${encodeURIComponent(integratorId)}/signatures`, request);
}

export async function listCasebooks(integratorId: string): Promise<CasebookRecord[]> {
  return getJson<CasebookRecord[]>(`/api/integrators/${encodeURIComponent(integratorId)}/casebooks`);
}

export async function createCasebook(
  integratorId: string,
  request: CreateCasebookRequest,
): Promise<CasebookRecord> {
  return postJson<CasebookRecord>(`/api/integrators/${encodeURIComponent(integratorId)}/casebooks`, request);
}

export async function saveCasebookSignature(
  casebookId: string,
  request: SaveSignatureRequest,
): Promise<CasebookRecord> {
  return postJson<CasebookRecord>(`/api/casebooks/${encodeURIComponent(casebookId)}/signatures`, request);
}

export async function askAi(request: AiAskRequest): Promise<AiResponse> {
  if (isTauri()) {
    return invokeCommand<AiResponse>('ask_ai_cmd', { request });
  }
  return postJson<AiResponse>('/api/ask', request);
}

export async function getProviderStatus(): Promise<ProviderStatus> {
  if (isTauri()) return invokeCommand("providers_cmd", {});
  const response = await fetch(`${API_BASE}/api/providers`);
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<ProviderStatus>;
}

export async function listRecentGroups(cluster: 'devnet' | 'mainnet'): Promise<RecentObservationSummary[]> {
  if (isTauri()) return invokeCommand('recent_groups_cmd', { cluster });
  return getJson(`/api/observations/groups?cluster=${encodeURIComponent(cluster)}`);
}
