import type {
  AiAskRequest,
  AiResponse,
  CasebookRecord,
  CreateCasebookRequest,
  CreateIntegratorRequest,
  DebugRequest,
  DebugResponse,
  IntegratorRecord,
  ProviderStatus,
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

export async function listIntegrators(): Promise<IntegratorRecord[]> {
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
  const response = await fetch(`${API_BASE}/api/providers`);
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<ProviderStatus>;
}
