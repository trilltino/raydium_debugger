import { activeInvestigation } from './investigation-recovery';
import React from 'react';
import ReactDOM from 'react-dom/client';
import {
  AlertTriangle,
  BookmarkPlus,
  Bot,
  CheckCircle2,
  ChevronRight,
  Copy,
  Cpu,
  Database,
  ExternalLink,
  FileText,
  FolderPlus,
  Gauge,
  Layers3,
  Loader2,
  Play,
  Search,
  ShieldAlert,
  Sparkles,
  Terminal,
  XCircle,
} from 'lucide-react';

import {
  askAi,
  createCasebook,
  createIntegrator,
  diagnoseTransaction,
  getProviderStatus,
  investigate,
  resumeInvestigation,
  retryInvestigation,
  listCasebooks,
  listIntegrators,
  listRecentGroups,
  saveCasebookSignature,
  saveIntegratorSignature,
} from './api';
import { dateTime, exactNumber, lamports, shortAddress, signed } from './format';
import type {
  AiResponse,
  AccountEvidence,
  CasebookRecord,
  DebugResponse,
  DecodedInstruction,
  DiagnosticResponse,
  ExecutionNode,
  InstructionDebugInfo,
  IntegratorRecord,
  InvestigationEvent,
  InvestigationResult,
  ProviderStatus,
  RecentObservationSummary,
  SavedSignature,
  StandardizedFailure,
} from './types';
import './styles.css';
import { Help } from './help/Help';
import { Review } from './review/Review';

type Tab = 'summary' | 'instructions' | 'compute' | 'accounts' | 'logs' | 'raw';

function App() {
  const [helpVisible, setHelpVisible] = React.useState(() => window.location.hash.startsWith('#help'));
  const [reviewVisible, setReviewVisible] = React.useState(() => window.location.hash.startsWith('#review'));
  React.useEffect(() => {
    const navigate = () => {
      setHelpVisible(window.location.hash.startsWith('#help'));
      setReviewVisible(window.location.hash.startsWith('#review'));
    };
    window.addEventListener('hashchange', navigate);
    return () => window.removeEventListener('hashchange', navigate);
  }, []);
  const [signature, setSignature] = React.useState('');
  const [symptom, setSymptom] = React.useState('');
  const [recentGroups, setRecentGroups] = React.useState<RecentObservationSummary[]>([]);
  const [groupsLoading, setGroupsLoading] = React.useState(false);
  const [groupsLoaded, setGroupsLoaded] = React.useState(false);
  const [cluster, setCluster] = React.useState<'devnet' | 'mainnet'>('devnet');
  React.useEffect(() => { setRecentGroups([]); setGroupsLoaded(false); }, [cluster]);
  const [providers, setProviders] = React.useState<ProviderStatus | null>(null);
  const [activeTab, setActiveTab] = React.useState<Tab>('summary');
  const [response, setResponse] = React.useState<DiagnosticResponse | null>(null);
  const [investigation, setInvestigation] = React.useState<InvestigationResult | null>(null);
  const [investigationProgress, setInvestigationProgress] = React.useState('');
  const [recoverableId, setRecoverableId] = React.useState<string | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [investigating, setInvestigating] = React.useState(false);
  const [question, setQuestion] = React.useState('');
  const [aiModel, setAiModel] = React.useState('');
  const [aiAnswer, setAiAnswer] = React.useState<AiResponse | null>(null);
  const [asking, setAsking] = React.useState(false);
  const [integrators, setIntegrators] = React.useState<IntegratorRecord[]>([]);
  const [selectedIntegratorId, setSelectedIntegratorId] = React.useState('');
  const [casebooks, setCasebooks] = React.useState<CasebookRecord[]>([]);
  const [selectedCasebookId, setSelectedCasebookId] = React.useState('');
  const [casebookFilter, setCasebookFilter] = React.useState('all');
  const [newIntegratorName, setNewIntegratorName] = React.useState('');
  const [newCasebookName, setNewCasebookName] = React.useState('');
  const [savingSignature, setSavingSignature] = React.useState(false);
  const resultsRef = React.useRef<HTMLElement>(null);

  React.useEffect(() => {
    if ((!response && !investigation) || helpVisible || reviewVisible || !window.matchMedia('(max-width: 720px)').matches) return;
    resultsRef.current?.focus({ preventScroll: true });
    resultsRef.current?.scrollIntoView({ behavior: 'instant', block: 'start' });
  }, [response, investigation, helpVisible, reviewVisible]);

  React.useEffect(() => {
    const active = activeInvestigation();
    if (!active) return;
    let cancelled = false;
    setInvestigating(true);
    resumeInvestigation(active.id, (event) => {
      if (!cancelled) setInvestigationProgress(event.message ?? event.stage ?? 'Recovering progress');
    }).then((result) => {
      if (cancelled) return;
      setInvestigation(result); setResponse(result.transaction_diagnosis); setInvestigationProgress('Investigation recovered');
    }).catch((failure: unknown) => {
      if (cancelled) return;
      setError(failure instanceof Error ? failure.message : String(failure)); setRecoverableId(active.id);
    }).finally(() => { if (!cancelled) setInvestigating(false); });
    return () => { cancelled = true; };
  }, []);

  async function retryActiveInvestigation() {
    if (!recoverableId) return;
    setInvestigating(true); setError(null);
    try {
      const result = await retryInvestigation(recoverableId, (event) => setInvestigationProgress(event.message ?? event.stage ?? 'Working'));
      setInvestigation(result); setResponse(result.transaction_diagnosis); setRecoverableId(null);
    } catch (failure) { setError(failure instanceof Error ? failure.message : String(failure)); }
    finally { setInvestigating(false); }
  }

  React.useEffect(() => {
    let cancelled = false;
    getProviderStatus()
      .then((status) => {
        if (cancelled) return;
        setProviders(status);
        setCluster((current) => preferredCluster(current, status));
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      });
    listIntegrators()
      .then((records) => {
        if (cancelled) return;
        setIntegrators(records);
        setSelectedIntegratorId((current) => current || records[0]?.id || '');
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  React.useEffect(() => {
    if (!selectedIntegratorId) {
      setCasebooks([]);
      setSelectedCasebookId('');
      return;
    }
    let cancelled = false;
    listCasebooks(selectedIntegratorId)
      .then((records) => {
        if (cancelled) return;
        setCasebooks(records);
        setSelectedCasebookId(records[0]?.id || '');
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selectedIntegratorId]);

  async function runDebug(event: React.FormEvent) {
    event.preventDefault();
    const configError = clusterConfigError(cluster, providers);
    if (configError) {
      setError(configError);
      return;
    }
    setLoading(true);
    setError(null);
    setAiAnswer(null);
    setInvestigation(null);
    setRecoverableId(null);
    try {
      const diagnosis = await diagnoseTransaction({
        signature: signature.trim(),
        cluster,
        data_mode: 'auto',
      });
      setResponse(diagnosis);
      setActiveTab('summary');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  async function runInvestigation(event: React.FormEvent | null, fingerprint?: string) {
    event?.preventDefault();
    const signatureValue = fingerprint ? "" : signature.trim();
    const symptomValue = fingerprint ? "" : symptom.trim();
    if (!signatureValue && !symptomValue && !fingerprint) {
      setError('Enter a transaction signature, describe the symptom, or provide both.');
      return;
    }
    if (signatureValue) {
      const configError = clusterConfigError(cluster, providers);
      if (configError) {
        setError(configError);
        return;
      }
    }
    setInvestigating(true);
    setError(null);
    setInvestigation(null);
    setResponse(null);
    setInvestigationProgress('Starting investigation');
    try {
      const result = await investigate(
        {
          signature: signatureValue || null,
          symptom: symptomValue || null,
          recent_fingerprint: fingerprint ?? null,
          cluster,
        },
        (eventUpdate: InvestigationEvent) => {
          if (eventUpdate.event_type === 'progress') {
            setInvestigationProgress(eventUpdate.message ?? eventUpdate.stage ?? 'Working');
          }
        },
      );
      setInvestigation(result);
      setResponse(result.transaction_diagnosis);
      setActiveTab('summary');
      setInvestigationProgress('Investigation complete');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setInvestigating(false);
    }
  }

  async function runAsk(event: React.FormEvent) {
    event.preventDefault();
    if (!response?.transaction || !question.trim()) return;
    setAsking(true);
    setAiAnswer(null);
    setError(null);
    try {
      const answer = await askAi({
        info: response.transaction,
        question: question.trim(),
        model: aiModel.trim() || null,
      });
      setAiAnswer(answer);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setAsking(false);
    }
  }

  async function addIntegrator(event: React.FormEvent) {
    event.preventDefault();
    const name = newIntegratorName.trim();
    if (!name) return;
    setError(null);
    try {
      const created = await createIntegrator({ name });
      setIntegrators((current) => [...current, created]);
      setSelectedIntegratorId(created.id);
      setNewIntegratorName('');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  async function saveCurrentSignature() {
    if (!selectedIntegratorId || !signature.trim()) return;
    const currentResponse = response;
    setSavingSignature(true);
    setError(null);
    try {
      const request = {
        signature: signature.trim(),
        cluster,
        label: signatureLabel(currentResponse),
        reason: signatureReason(currentResponse),
        outcome: currentResponse?.transaction
          ? currentResponse.transaction.success
            ? 'success'
            : 'failed'
          : currentResponse?.observation.status ?? null,
        product:
          currentResponse?.transaction?.raydium_context?.product ??
          currentResponse?.transaction?.raydium_product?.product ??
          null,
        failure_category: currentResponse?.transaction?.failure?.category ?? currentResponse?.diagnosis.category ?? null,
        failure_code: currentResponse?.transaction?.failure?.code_hex ?? null,
        tags: signatureTags(currentResponse),
        pinned: currentResponse?.transaction ? !currentResponse.transaction.success : Boolean(currentResponse),
      };
      const updated = selectedCasebookId
        ? await saveCasebookSignature(selectedCasebookId, request)
        : await saveIntegratorSignature(selectedIntegratorId, request);
      if ('integrator_id' in updated) {
        setCasebooks((current) => current.map((record) => (record.id === updated.id ? updated : record)));
        const integratorsNext = await listIntegrators();
        setIntegrators(integratorsNext);
      } else {
        setIntegrators((current) => current.map((record) => (record.id === updated.id ? updated : record)));
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSavingSignature(false);
    }
  }

  async function addCasebook(event: React.FormEvent) {
    event.preventDefault();
    const name = newCasebookName.trim();
    if (!name || !selectedIntegratorId) return;
    setError(null);
    try {
      const created = await createCasebook(selectedIntegratorId, { name });
      setCasebooks((current) => [...current, created]);
      setSelectedCasebookId(created.id);
      setNewCasebookName('');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  function selectSavedSignature(savedId: string) {
    const saved = [...casebooks.flatMap((casebook) => casebook.signatures), ...integrators.flatMap((integrator) => integrator.signatures)]
      .find((candidate) => candidate.id === savedId);
    if (!saved) return;
    setSignature(saved.signature);
    setCluster(saved.cluster);
    setResponse(null);
    setInvestigation(null);
    setAiAnswer(null);
    setActiveTab('summary');
  }

  const info = response?.transaction ?? null;
  const debug = response;
  const selectedIntegrator = integrators.find((record) => record.id === selectedIntegratorId) ?? null;
  const selectedCasebook = casebooks.find((record) => record.id === selectedCasebookId) ?? null;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <RaydiumMark />
          <div>
            <h1>Raydium Debugger</h1>
          </div>
        </div>
        <nav className="topnav" aria-label="Debugger sections">
          <a href="#debug" aria-current={!helpVisible && !reviewVisible ? 'page' : undefined}>Debug</a>
          <a href="#help" aria-current={helpVisible ? 'page' : undefined}>Updates</a>
          <a href="#review" aria-current={reviewVisible ? 'page' : undefined}>Knowledge review</a>
        </nav>
      </header>

      {helpVisible && <Help />}
      {reviewVisible && <Review />}
      <div className="debug-intro" hidden={helpVisible || reviewVisible}>
        <div className="debug-intro__content">
          <span className="debug-intro__eyebrow">Raydium / Developer tools</span>
          <h2>Debug transactions with context.</h2>
          <p>Trace a Solana signature, investigate a support symptom, and review the evidence behind each answer.</p>
        </div>
      </div>
      <main className="shell debug-view" hidden={helpVisible || reviewVisible}>
        <section className="swap-console" aria-label="Transaction debugger console">
          <form className="query" onSubmit={runDebug}>
            <label className="field field--wide">
              <span>Transaction signature</span>
              <input
                value={signature}
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
                onChange={(event) => setSignature(event.target.value)}
                placeholder="Paste a Solana transaction signature"
                required
              />
            </label>
            <label className="field">
              <span>Cluster</span>
              <select value={cluster} onChange={(event) => setCluster(event.target.value as 'devnet' | 'mainnet')}>
                <option value="devnet" disabled={providers ? !providers.triton.devnet_configured : false}>
                  Devnet{providers && !providers.triton.devnet_configured ? ' - not configured' : ''}
                </option>
                <option value="mainnet" disabled={providers ? !providers.triton.mainnet_configured : false}>
                  Mainnet{providers && !providers.triton.mainnet_configured ? ' - not configured' : ''}
                </option>
              </select>
            </label>
            <button className="primary" type="submit" disabled={loading || Boolean(clusterConfigError(cluster, providers))}>
              {loading ? <Loader2 className="spin" size={17} /> : <Search size={17} />}
              Debug
            </button>
          </form>

          {recoverableId && <button type="button" disabled={investigating} onClick={() => void retryActiveInvestigation()}>Retry investigation</button>}
          <form className="investigation-query" onSubmit={(event) => void runInvestigation(event)}>
            <label className="field">
              <span>Support symptom</span>
              <input
                value={symptom}
                onChange={(event) => setSymptom(event.target.value)}
                placeholder="Describe what went wrong (optional with a signature)"
              />
            </label>
            <button
              className="secondary"
              type="submit"
              disabled={investigating || (!signature.trim() && !symptom.trim())}
            >
              {investigating ? <Loader2 className="spin" size={17} /> : <Search size={17} />}
              Investigate
            </button>
          </form>
          <section aria-label="Recent execution groups" className="recent-groups">
            <button type="button" className="secondary" disabled={groupsLoading || investigating} onClick={async () => {
              setGroupsLoading(true); setError(null);
              try { setRecentGroups(await listRecentGroups(cluster)); setGroupsLoaded(true); }
              catch (error) { setError(error instanceof Error ? error.message : String(error)); }
              finally { setGroupsLoading(false); }
            }}>{groupsLoading ? 'Loading observations…' : 'Browse recent observations'}</button>
            {groupsLoaded && recentGroups.length === 0 && <p className="muted">No recent observations for this cluster.</p>}
            {recentGroups.filter((group) => group.cluster === cluster).map((group) => <button type="button" className="secondary" key={`${group.cluster}:${group.source}:${group.fingerprint}`} disabled={investigating}
              onClick={() => void runInvestigation(null, group.fingerprint)}>
              Investigate {group.source} · {group.instruction ?? 'execution'}{group.error_code ? ` · error ${group.error_code}` : ''} · {new Date(group.observed_at * 1000).toLocaleString()}
            </button>)}
          </section>

          <IntegratorLibrary
            integrators={integrators}
            selectedIntegrator={selectedIntegrator}
            selectedIntegratorId={selectedIntegratorId}
            casebooks={casebooks}
            selectedCasebook={selectedCasebook}
            selectedCasebookId={selectedCasebookId}
            casebookFilter={casebookFilter}
            newIntegratorName={newIntegratorName}
            newCasebookName={newCasebookName}
            currentSignature={signature}
            saving={savingSignature}
            onSelectIntegrator={setSelectedIntegratorId}
            onSelectCasebook={setSelectedCasebookId}
            onCasebookFilter={setCasebookFilter}
            onSelectSignature={selectSavedSignature}
            onNewIntegratorName={setNewIntegratorName}
            onNewCasebookName={setNewCasebookName}
            onAddIntegrator={addIntegrator}
            onAddCasebook={addCasebook}
            onSaveSignature={saveCurrentSignature}
          />
        </section>

        {error && (
          <div className="alert">
            <AlertTriangle size={18} />
            <span>{error}</span>
          </div>
        )}

        {!debug && !investigation ? (
          <EmptyState />
        ) : (
          <section ref={resultsRef} tabIndex={-1} aria-label="Diagnostic results">
            {investigation && <InvestigationPanel result={investigation} progress={investigationProgress} />}
            {debug && <StatusStrip response={debug} />}
            {debug && !info ? (
              <section className="content content--single">
                <DiagnosisPanel response={debug} />
                <ObservationPanel response={debug} />
              </section>
            ) : debug && info ? (
            <div className="layout">
              <section className="content">
                <Tabs active={activeTab} onChange={setActiveTab} />
                {activeTab === 'summary' && <Summary response={debug} info={info} onAsk={runAsk} question={question} setQuestion={setQuestion} aiModel={aiModel} setAiModel={setAiModel} asking={asking} answer={aiAnswer} />}
                {activeTab === 'instructions' && <Instructions instructions={info.outer_instructions} executionTree={info.execution_tree} decoded={info.decoded_instructions} />}
                {activeTab === 'compute' && <ComputePanel info={info} />}
                {activeTab === 'accounts' && <Accounts accounts={info.accounts} />}
                {activeTab === 'logs' && <Logs logs={info.logs} />}
                {activeTab === 'raw' && <Raw text={debug.formatted_text} json={debug} />}
              </section>
              <aside className="side">
                <Recommendations info={info} />
              </aside>
            </div>
            ) : null}
          </section>
        )}
      </main>
    </div>
  );
}

function IntegratorLibrary({
  integrators,
  selectedIntegrator,
  selectedIntegratorId,
  casebooks,
  selectedCasebook,
  selectedCasebookId,
  casebookFilter,
  newIntegratorName,
  newCasebookName,
  currentSignature,
  saving,
  onSelectIntegrator,
  onSelectCasebook,
  onCasebookFilter,
  onSelectSignature,
  onNewIntegratorName,
  onNewCasebookName,
  onAddIntegrator,
  onAddCasebook,
  onSaveSignature,
}: {
  integrators: IntegratorRecord[];
  selectedIntegrator: IntegratorRecord | null;
  selectedIntegratorId: string;
  casebooks: CasebookRecord[];
  selectedCasebook: CasebookRecord | null;
  selectedCasebookId: string;
  casebookFilter: string;
  newIntegratorName: string;
  newCasebookName: string;
  currentSignature: string;
  saving: boolean;
  onSelectIntegrator: (id: string) => void;
  onSelectCasebook: (id: string) => void;
  onCasebookFilter: (filter: string) => void;
  onSelectSignature: (id: string) => void;
  onNewIntegratorName: (name: string) => void;
  onNewCasebookName: (name: string) => void;
  onAddIntegrator: (event: React.FormEvent) => void;
  onAddCasebook: (event: React.FormEvent) => void;
  onSaveSignature: () => void;
}) {
  const allSignatures = selectedCasebook?.signatures ?? selectedIntegrator?.signatures ?? [];
  const signatures = allSignatures.filter((saved) => matchesSignatureFilter(saved, casebookFilter));
  return (
    <section className="library" aria-label="Integrator Library">
      <div className="library__head">
        <div>
          <Database size={17} />
          <h2>Integrator Library</h2>
        </div>
        <span>{integrators.length} integrators</span>
      </div>
      <div className="library__grid">
        <label className="field">
          <span>Integrator</span>
          <select
            aria-label="Integrator select"
            value={selectedIntegratorId}
            onChange={(event) => onSelectIntegrator(event.target.value)}
          >
            <option value="">Choose integrator</option>
            {integrators.map((integrator) => (
              <option key={integrator.id} value={integrator.id}>
                {integrator.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Casebook</span>
          <select
            aria-label="Casebook select"
            value={selectedCasebookId}
            onChange={(event) => onSelectCasebook(event.target.value)}
            disabled={!casebooks.length}
          >
            <option value="">{casebooks.length ? 'Choose casebook' : 'No casebooks'}</option>
            {casebooks.map((casebook) => (
              <option key={casebook.id} value={casebook.id}>
                {casebook.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Saved signatures</span>
          <select
            aria-label="Saved signature select"
            value=""
            onChange={(event) => onSelectSignature(event.target.value)}
            disabled={!signatures.length}
          >
            <option value="">{signatures.length ? 'Pick saved transaction' : 'No saved signatures'}</option>
            {signatures.map((saved) => (
              <option key={saved.id} value={saved.id}>
                {savedSignatureLabel(saved)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>Filter</span>
          <select aria-label="Casebook filter" value={casebookFilter} onChange={(event) => onCasebookFilter(event.target.value)}>
            <option value="all">All</option>
            <option value="failed">Failed</option>
            <option value="success">Success</option>
            <option value="regression">Regression</option>
            <option value="needs-idl">Needs IDL</option>
            <option value="raydium">Raydium</option>
          </select>
        </label>
        <form className="library__create" onSubmit={onAddIntegrator}>
          <label className="field">
            <span>New integrator</span>
            <input
              value={newIntegratorName}
              onChange={(event) => onNewIntegratorName(event.target.value)}
              placeholder="Integrator name"
            />
          </label>
          <button className="secondary" type="submit" disabled={!newIntegratorName.trim()}>
            <FolderPlus size={16} />
            Add integrator
          </button>
        </form>
        <form className="library__create" onSubmit={onAddCasebook}>
          <label className="field">
            <span>New casebook</span>
            <input
              value={newCasebookName}
              onChange={(event) => onNewCasebookName(event.target.value)}
              placeholder="Casebook name"
              disabled={!selectedIntegratorId}
            />
          </label>
          <button className="secondary" type="submit" disabled={!selectedIntegratorId || !newCasebookName.trim()}>
            <FolderPlus size={16} />
            Add casebook
          </button>
        </form>
        <button
          className="secondary library__save"
          type="button"
          disabled={!selectedIntegratorId || !currentSignature.trim() || saving}
          onClick={onSaveSignature}
        >
          {saving ? <Loader2 className="spin" size={16} /> : <BookmarkPlus size={16} />}
          Save signature
        </button>
      </div>
    </section>
  );
}

function matchesSignatureFilter(saved: SavedSignature, filter: string): boolean {
  if (filter === 'all') return true;
  if (filter === 'failed') return saved.outcome === 'failed';
  if (filter === 'success') return saved.outcome === 'success';
  if (filter === 'regression') return saved.outcome === 'regression' || saved.tags.includes('regression');
  if (filter === 'needs-idl') return saved.tags.includes('needs-idl') || saved.failure_category === 'program_rejection';
  if (filter === 'raydium') return Boolean(saved.product?.includes('amm') || saved.product?.includes('cpmm') || saved.product?.includes('clmm') || saved.product?.includes('launch'));
  return true;
}

function savedSignatureLabel(saved: SavedSignature): string {
  const label = saved.label ? `${saved.label} - ` : '';
  const reason = saved.reason ? ` - ${saved.reason}` : '';
  return `${label}${saved.signature} (${saved.cluster})${reason}`;
}

function signatureLabel(response: DiagnosticResponse | null): string {
  if (!response) return 'Saved transaction';
  return response.transaction?.failure?.plain_title ?? response.transaction?.failure?.title ?? response.transaction?.experience.headline ?? response.diagnosis.title;
}

function signatureReason(response: DiagnosticResponse | null): string | null {
  if (!response) return null;
  return response.transaction?.failure?.primary_action ?? response.transaction?.experience.next_step ?? response.diagnosis.primary_action;
}

function signatureTags(response: DiagnosticResponse | null): string[] {
  if (!response) return [];
  const info = response.transaction;
  return [
    info?.provider.cluster ?? response.observation.cluster,
    info?.raydium_context?.product ?? info?.raydium_product?.product,
    info?.failure?.category ?? response.diagnosis.category,
    info?.failure?.missing_artifact ? 'needs-idl' : null,
    info ? (info.success ? 'success' : 'failed') : response.observation.status,
  ].filter((tag): tag is string => Boolean(tag));
}

function RaydiumMark() {
  return (
    <svg className="raydium-mark" viewBox="0 0 29 33" role="img" aria-label="Raydium">
      <defs>
        <linearGradient id="raydiumGradient" x1="28.3168" x2="-1.73336" y1="8.19162" y2="20.2086" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#c200fb" />
          <stop offset="0.489658" stopColor="#3772ff" />
          <stop offset="1" stopColor="#5ac4be" />
        </linearGradient>
      </defs>
      <g fill="url(#raydiumGradient)">
        <path d="m26.8625 12.281v11.4104l-12.6916 7.3261-12.69859-7.3261v-14.65937l12.69859-7.33322 9.7541 5.63441 1.4723-.84941-11.2264-6.48381-14.1709 8.18262v16.35818l14.1709 8.1826 14.171-8.1826v-13.1092z" />
        <path d="m10.6176 23.6985h-2.12353v-7.1209h7.07843c.6697-.0074 1.3095-.2782 1.7811-.7538.4716-.4755.737-1.1176.7388-1.7874.0038-.3311-.0601-.6596-.1879-.9651-.1279-.3056-.3168-.5817-.5554-.8115-.2308-.2372-.5071-.4253-.8124-.553-.3053-.1278-.6333-.1925-.9642-.1903h-7.07843v-2.16595h7.08543c1.2405.00743 2.4281.50351 3.3053 1.38065.8771.8772 1.3732 2.0648 1.3806 3.3052.0076.9496-.2819 1.8777-.8281 2.6544-.5027.7432-1.2111 1.3237-2.0386 1.6705-.8194.2599-1.6745.3889-2.5341.3823h-4.247z" />
        <path d="m20.2159 23.5215h-2.4775l-1.9111-3.3339c.7561-.0463 1.5019-.1988 2.2155-.453z" />
        <path d="m25.3831 9.90975 1.4652.81405 1.4653-.81405v-1.72005l-1.4653-.84941-1.4652.84941z" />
      </g>
    </svg>
  );
}

function preferredCluster(current: 'devnet' | 'mainnet', providers: ProviderStatus): 'devnet' | 'mainnet' {
  if (clusterConfigured(current, providers)) return current;
  if (providers.triton.devnet_configured) return 'devnet';
  if (providers.triton.mainnet_configured) return 'mainnet';
  return current;
}

function clusterConfigured(cluster: 'devnet' | 'mainnet', providers: ProviderStatus | null): boolean {
  if (!providers) return true;
  return cluster === 'mainnet' ? providers.triton.mainnet_configured : providers.triton.devnet_configured;
}

function clusterConfigError(cluster: 'devnet' | 'mainnet', providers: ProviderStatus | null): string | null {
  if (!providers || clusterConfigured(cluster, providers)) return null;
  return `${cluster === 'mainnet' ? 'Mainnet' : 'Devnet'} is not configured on the server. Add the cluster endpoint in the server environment and restart.`;
}

function EmptyState() {
  return (
    <section className="empty">
      <Terminal size={38} />
    </section>
  );
}

function InvestigationPanel({
  result,
  progress,
}: {
  result: InvestigationResult;
  progress: string;
}) {
  return (
    <section className="investigation-result" aria-label="Investigation result">
      <header className="investigation-result__header">
        <div>
          <span>Investigation · {result.status}</span>
          {result.symptom && <h2>{result.symptom}</h2>}
          {result.signature && <code>{result.signature}</code>}
          {!result.symptom && !result.signature && <h2>Recent execution investigation</h2>}
        </div>
        <p aria-live="polite">{progress}</p>
      </header>

      {result.transaction_error && (
        <p className="investigation-note investigation-note--warn">
          Transaction evidence unavailable: {result.transaction_error}
        </p>
      )}

      <section className="investigation-section" aria-label="Approved incidents">
        <h3>Reviewed incidents</h3>
        {result.related_incidents.length === 0 ? (
          <p className="muted">No approved historical incident matched this input.</p>
        ) : (
          <div className="investigation-list">
            {result.related_incidents.map((incident) => (
              <article className="investigation-item" key={incident.id}>
                <div className="investigation-item__meta">
                  {incident.product && <span>{incident.product}</span>}
                  {incident.failure_domain && <span>{incident.failure_domain}</span>}
                  <span>{incident.evidence_message_count} reviewed messages</span>
                </div>
                <h4>{incident.summary}</h4>
                <p>{incident.resolution}</p>
                {result.incident_matches?.filter((match) => match.incident_id === incident.id).map((match) => <div key={match.incident_id}>
                  <p>{match.strength} historical match · {match.reasons.join('; ')}</p>
                  {match.missing_signals.length > 0 && <p className="muted">Missing evidence: {match.missing_signals.join('; ')}</p>}
                </div>)}
                {incident.symptom_tags.length > 0 && (
                  <div className="investigation-tags">
                    {incident.symptom_tags.map((tag) => <span key={tag}>{tag}</span>)}
                  </div>
                )}
              </article>
            ))}
          </div>
        )}
      </section>

      {result.incident_matches?.some((match) => match.contradictions.length > 0) && <section className="investigation-section" aria-label="Rejected incidents">
        <h3>Contradictory incidents excluded</h3>
        {result.incident_matches.filter((match) => match.contradictions.length > 0).map((match) => <p key={match.incident_id}>{match.incident_id}: {match.contradictions.join('; ')}</p>)}
      </section>}

      <section className="investigation-section" aria-label="Recent observations">
        <h3>Recent observations</h3>
        {result.recent_observations.length === 0 ? (
          <p className="muted">No matching recent operational observation was found.</p>
        ) : (
          <div className="investigation-list">
            {result.recent_observations.map((observation) => (
              <div className="investigation-observation" key={`${observation.cluster}:${observation.source}:${observation.fingerprint}`}>
                <strong>{observation.source} · {observation.cluster}</strong>
                <span>{observation.instruction ?? 'execution observation'}</span>
                {observation.error_code && <code>error {observation.error_code}</code>}
                {observation.slot !== null && <code>slot {observation.slot}</code>}
                <time dateTime={new Date(observation.observed_at * 1000).toISOString()}>
                  {new Date(observation.observed_at * 1000).toLocaleString()}
                </time>
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="investigation-section" aria-label="Evidence ledger">
        <h3>Evidence</h3>
        {result.evidence.length === 0 ? (
          <p className="muted">No evidence records are available yet.</p>
        ) : (
          <ol className="investigation-evidence">
            {result.evidence.map((item) => (
              <li key={item.evidence_id}>
                <span>{item.evidence_type.replace(/_/g, ' ')}</span>
                <p>{item.summary}</p>
                <code>{item.source_reference}</code>
              </li>
            ))}
          </ol>
        )}
      </section>

      {result.unknowns.length > 0 && (
        <section className="investigation-section" aria-label="Unknowns">
          <h3>Unknowns</h3>
          <ul className="list warn">
            {result.unknowns.map((unknown) => <li key={unknown}>{unknown}</li>)}
          </ul>
        </section>
      )}
    </section>
  );
}

function StatusStrip({ response }: { response: DiagnosticResponse }) {
  const info = response.transaction;
  const failure = info?.failure;
  const tone = info ? experienceTone(info.experience.tone) : 'warn';
  return (
    <section className="stats">
      <Stat label="Status" value={info?.experience.status_label ?? response.observation.status} tone={tone} hint={response.diagnosis.title} icon={info?.success ? <CheckCircle2 /> : <XCircle />} />
      <Stat label="Diagnosis" value={failure?.name ?? response.diagnosis.category} hint={response.diagnosis.explanation} tone={failure ? 'warn' : tone} icon={<ShieldAlert />} />
      <Stat label="Slot" value={info ? exactNumber(info.slot_exact ?? info.slot) : '-'} hint={info?.freshness.note ?? response.observation.cluster ?? undefined} icon={<Gauge />} />
      <Stat label="Compute" value={info ? exactNumber(info.compute_units_consumed_exact ?? info.compute_units_consumed) : '-'} hint={info ? `Fee ${lamports(info.fee_paid_exact ?? info.fee_paid)}` : 'Not landed'} icon={<Cpu />} />
    </section>
  );
}

function experienceTone(tone: string): 'default' | 'ok' | 'warn' | 'bad' {
  if (tone === 'success') return 'ok';
  if (tone === 'warning') return 'warn';
  if (tone === 'danger') return 'bad';
  return 'default';
}

function Stat({ label, value, hint, tone = 'default', icon }: { label: string; value: string; hint?: string; tone?: 'default' | 'ok' | 'warn' | 'bad'; icon: React.ReactElement }) {
  return (
    <article className={`stat stat--${tone}`}>
      {React.cloneElement(icon, { size: 18 })}
      <span>{label}</span>
      <strong>{value}</strong>
      {hint && <small>{hint}</small>}
    </article>
  );
}

function Tabs({ active, onChange }: { active: Tab; onChange: (tab: Tab) => void }) {
  const tabs: Array<[Tab, string]> = [
    ['summary', 'Summary'],
    ['instructions', 'Execution Tree'],
    ['compute', 'Compute + Fees'],
    ['accounts', 'Accounts'],
    ['logs', 'Logs'],
    ['raw', 'Raw'],
  ];
  return (
    <div className="tabs" role="group" aria-label="Transaction evidence views">
      {tabs.map(([key, label]) => (
        <button key={key} type="button" aria-pressed={active === key} className={active === key ? 'is-active' : ''} onClick={() => onChange(key)}>
          {label}
        </button>
      ))}
    </div>
  );
}

function Summary(props: {
  response: DiagnosticResponse;
  info: DebugResponse['info'];
  onAsk: (event: React.FormEvent) => void;
  question: string;
  setQuestion: (value: string) => void;
  aiModel: string;
  setAiModel: (value: string) => void;
  asking: boolean;
  answer: AiResponse | null;
}) {
  const { info } = props;
  const failure = info.failure;
  const headline = failure?.plain_title ?? info.experience.headline;
  const explanation = failure?.plain_explanation ?? info.experience.message;
  const primaryAction = failure?.primary_action ?? info.experience.next_step;
  return (
    <div className="stack">
      <DiagnosisPanel response={props.response} />
      <ObservationPanel response={props.response} />
      <section className={`result result--${info.experience.tone}`}>
        <div className="result__icon">
          {info.success ? <CheckCircle2 size={22} /> : <XCircle size={22} />}
        </div>
        <div>
          <span>{info.experience.status_label}</span>
          <h2>{headline}</h2>
          <p>{explanation}</p>
          <strong>Primary action: {primaryAction}</strong>
        </div>
      </section>

      {info.success && <SuccessSnapshot info={info} />}
      {info.raydium_context && <RaydiumDiagnosis info={info} />}
      {failure && isUnknownCustomFailure(failure) && <DecodeStatusCallout info={info} failure={failure} />}

      <Panel title="Decoded Result" icon={<ShieldAlert />}>
        <p className="lead">{failure?.plain_explanation ?? failure?.user_message ?? info.root_cause.summary}</p>
        <div className="chips">
          <Chip label="Category" value={failure?.category ?? info.root_cause.category} />
          <Chip label="Decode status" value={failure ? decodeStatusLabel(failure.decode_status) : 'Runtime summary'} />
          <Chip label="Confidence" value={failure?.confidence ?? 'unknown'} />
          <Chip label="Program" value={failure?.program_label ?? info.failing_instruction?.program_label ?? '-'} />
          <Chip label="Code" value={failure?.code_hex ?? '-'} />
        </div>
        {info.experience.detail_badges.length > 0 && (
          <div className="badges" aria-label="Result context">
            {info.experience.detail_badges.map((badge) => (
              <span key={badge}>{badge}</span>
            ))}
          </div>
        )}
      </Panel>

      <Panel title="Transaction Context" icon={<FileText />}>
        <div className="kv">
          <Row label="Signature" value={info.signature} copy />
          <Row label="Time" value={dateTime(info.timestamp)} />
          <Row label="Cluster" value={info.provider.cluster ?? '-'} />
          <Row label="Version" value={info.metadata.transaction_version} />
          <Row label="Payer" value={info.metadata.payer ?? '-'} copy={Boolean(info.metadata.payer)} />
          <Row label="Raydium" value={info.raydium_product ? `${info.raydium_product.product}${info.raydium_product.phase ? ` / ${info.raydium_product.phase}` : ''}` : '-'} />
        </div>
      </Panel>

      <Panel title="AI Q&A" icon={<Bot />}>
        <form className="ask" onSubmit={props.onAsk}>
          <input value={props.question} onChange={(event) => props.setQuestion(event.target.value)} placeholder="Ask about this transaction context" />
          <input value={props.aiModel} onChange={(event) => props.setAiModel(event.target.value)} placeholder="Model (optional)" />
          <button type="submit" disabled={props.asking || !props.question.trim()}>
            {props.asking ? <Loader2 className="spin" size={16} /> : <Sparkles size={16} />}
            Ask
          </button>
        </form>
        {props.answer && <>
          {props.answer.knowledge && <div aria-label="Reviewed knowledge">
            <p>{({
              unavailable: 'Reviewed knowledge is unavailable. The answer uses transaction evidence only.',
              empty: 'No reviewed historical incidents have been published yet. Current guidance may still be available below.',
              no_match: 'No reviewed incident matched this transaction and question.',
              matched: `${props.answer.knowledge.incidents.length} historical ${props.answer.knowledge.incidents.length === 1 ? 'match' : 'matches'} from ${props.answer.knowledge.incident_count} reviewed incidents.`,
            })[props.answer.knowledge.status]}</p>
            {props.answer.knowledge.incidents.map((incident) => <details key={incident.incident_id}>
              <summary>{incident.incident_id} · {incident.strength} match</summary>
              <p>{incident.summary}</p>
              <p>{incident.resolution}</p>
              <ul className="list">{incident.reasons.map((reason) => <li key={reason}>{reason}</li>)}</ul>
              {incident.missing_signals.length > 0 && <p>Still unknown: {incident.missing_signals.join('; ')}</p>}
            </details>)}
            {(props.answer.knowledge.guidance?.length ?? 0) > 0 && <section aria-label="Reviewed current guidance"><p>{props.answer.knowledge.guidance?.length} current guidance matches from {props.answer.knowledge.guidance_count} reviewed entries. These are advice, not confirmed historical fixes.</p>
              {props.answer.knowledge.guidance?.map((item) => <details key={item.guidance_id}><summary>{item.guidance_id} · current guidance</summary><p>{item.summary}</p><p>{item.guidance}</p><small>Matched: {item.matched_terms.join(', ')}</small></details>)}</section>}
          </div>}
          {props.answer.updates?.status === 'matched' && <div aria-label="Upgrade notices">
            <p>{props.answer.updates.updates.length} dated upgrade {props.answer.updates.updates.length === 1 ? 'notice' : 'notices'} matched. Planned notices do not establish deployment.</p>
            {props.answer.updates.updates.map((update) => <details key={update.update_id}>
              <summary>{update.date} · {update.status} · {update.summary}</summary>
              <p>{update.excerpt}</p>
              {update.reference_excerpt && <p>{update.reference_excerpt}</p>}
              <p><a href={update.source_url} target="_blank" rel="noreferrer">Source announcement</a>{update.reference_repo && update.reference_commit && update.reference_path ? ` · ${update.reference_repo}@${update.reference_commit.slice(0, 8)}: ${update.reference_path}` : ''}</p>
            </details>)}
          </div>}
          <pre className="answer">{props.answer.answer}</pre>
        </>}
      </Panel>
    </div>
  );
}

function DiagnosisPanel({ response }: { response: DiagnosticResponse }) {
  const diagnosis = response.diagnosis;
  return (
    <Panel title="Diagnosis" icon={<ShieldAlert />}>
      <div className="diagnosis-hero">
        <div>
          <span>{diagnosis.category} · {diagnosis.confidence}</span>
          <h2>{diagnosis.title}</h2>
          <p>{diagnosis.explanation}</p>
          <strong>Primary action: {diagnosis.primary_action}</strong>
        </div>
        <button type="button" onClick={() => navigator.clipboard.writeText(diagnosis.copy_markdown)}>
          <Copy size={14} />
          Copy diagnosis
        </button>
      </div>
      {diagnosis.evidence.length > 0 && (
        <ul className="list">
          {diagnosis.evidence.map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      )}
    </Panel>
  );
}

function ObservationPanel({ response }: { response: DiagnosticResponse }) {
  const observation = response.observation;
  return (
    <Panel title="Observation" icon={<Database />}>
      <div className="chips">
        <Chip label="Status" value={observation.status} />
        <Chip label="Cluster" value={observation.cluster ?? '-'} />
        <Chip label="Providers" value={`${observation.providers_queried.length}`} />
      </div>
      {observation.evidence.length > 0 && (
        <ul className="list">
          {observation.evidence.map((item) => (
            <li key={item}>{item}</li>
          ))}
        </ul>
      )}
      {observation.hypotheses.length > 0 && (
        <>
          <p className="muted">Possible causes, not proven from the signature alone:</p>
          <ul className="list warn">
            {observation.hypotheses.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
        </>
      )}
    </Panel>
  );
}

function SuccessSnapshot({ info }: { info: DebugResponse['info'] }) {
  return (
    <Panel title="Success Summary" icon={<CheckCircle2 />}>
      <div className="success-grid">
        <Metric label="Cluster" value={info.provider.cluster ?? '-'} />
        <Metric label="Slot" value={exactNumber(info.slot_exact ?? info.slot)} />
        <Metric label="Fee" value={lamports(info.fee_paid_exact ?? info.fee_paid)} />
        <Metric label="Raydium" value={info.raydium_product ? `${info.raydium_product.product}${info.raydium_product.phase ? ` / ${info.raydium_product.phase}` : ''}` : 'Not detected'} />
        <Metric label="Warnings" value={`${info.metadata.fetch_warnings.length + info.provider.warnings.length}`} />
      </div>
    </Panel>
  );
}

function RaydiumDiagnosis({ info }: { info: DebugResponse['info'] }) {
  const context = info.raydium_context;
  if (!context) return null;
  const summary = context.swap_summary;
  const roles = context.account_roles.slice(0, 12);
  const movements = context.token_movements.slice(0, 8);
  const warnings = visibleRaydiumWarnings(context.warnings);
  return (
    <Panel title="Raydium Diagnosis" icon={<Gauge />}>
      <div className="success-grid">
        <Metric label="Product" value={context.product ?? info.raydium_product?.product ?? 'Not proven'} />
        <Metric label="Phase" value={context.phase ?? info.raydium_product?.phase ?? 'Not proven'} />
        <Metric label="Route" value={summary?.route_kind ?? 'Not proven'} />
        <Metric label="Slippage" value={summary?.slippage_result ? 'See note' : 'Not available'} />
      </div>
      {summary?.slippage_result && <p className="lead lead--compact">{summary.slippage_result}</p>}
      {movements.length > 0 ? (
        <div className="diagnosis-table" aria-label="Token movements">
          <strong>Token movements</strong>
          {movements.map((movement) => (
            <div className="diagnosis-row" key={`${movement.account_index}-${movement.mint}`}>
              <span>#{movement.account_index}</span>
              <code title={movement.mint}>{shortAddress(movement.mint, 6, 6)}</code>
              <span>{movement.delta_raw}</span>
            </div>
          ))}
        </div>
      ) : (
        <p className="muted">Token movement was not available from RPC metadata.</p>
      )}
      {roles.length > 0 ? (
        <div className="diagnosis-table" aria-label="Raydium account roles">
          <strong>Role-labeled accounts</strong>
          {roles.map((role) => (
            <div className="diagnosis-row" key={`${role.instruction_index}-${role.account_index}-${role.role}`}>
              <span>#{role.instruction_index}.{role.account_index}</span>
              <span>{role.role}</span>
              <code title={role.pubkey}>{shortAddress(role.pubkey, 6, 6)}</code>
              <span>{role.confidence}</span>
            </div>
          ))}
        </div>
      ) : null}
      {warnings.length > 0 && (
        <ul className="list warn">
          {warnings.map((warning) => (
            <li key={warning}>{warning}</li>
          ))}
        </ul>
      )}
    </Panel>
  );
}

function visibleRaydiumWarnings(warnings: string[]) {
  return uniqueStrings(warnings).filter((warning) => {
    const normalized = warning.toLocaleLowerCase();
    return (
      !normalized.includes('vault/token role could not be proven') &&
      !normalized.includes('vault and pool account roles could not be proven') &&
      !normalized.includes('may apply transfer fees or hooks')
    );
  });
}

function uniqueStrings(values: string[]) {
  return values.filter((value, index) => values.indexOf(value) === index);
}

function DecodeStatusCallout({ info, failure }: { info: DebugResponse['info']; failure: StandardizedFailure }) {
  const message = integratorMessage(info, failure);
  return (
    <section className="decode-callout" aria-label="Decode status">
      <div className="decode-callout__head">
        <AlertTriangle size={20} />
        <div>
          <span>{decodeStatusLabel(failure.decode_status)}</span>
          <h2>This is a program-specific custom error</h2>
        </div>
      </div>
      <p>
        The transaction did expose a real custom code, but the meaning belongs to the failing program. The debugger will not guess a name without that program's IDL, source error enum, SDK error map, or docs.
      </p>
      <div className="decode-columns">
        <div>
          <h3>Known</h3>
          <ul>
            <li>Program: <code>{failure.program_id ?? '-'}</code></li>
            <li>Code: <code>{failure.code_decimal ?? '-'}</code> / <code>{failure.code_hex ?? '-'}</code></li>
            <li>Instruction: <code>{failure.instruction_index ?? info.failing_instruction?.index ?? '-'}</code></li>
          </ul>
        </div>
        <div>
          <h3>Tried</h3>
          <ul>
            {(failure.decode_explanation ? [failure.decode_explanation] : failure.decode_attempts).map((attempt) => (
              <li key={attempt}>{attempt}</li>
            ))}
          </ul>
        </div>
        <div>
          <h3>Needed</h3>
          <ul>
            <li>Anchor IDL JSON</li>
            <li>Source error enum</li>
            <li>SDK error map or docs</li>
          </ul>
        </div>
      </div>
      <div className="handoff">
        <strong>For integrator</strong>
        <pre>{message}</pre>
        <button type="button" onClick={() => navigator.clipboard.writeText(message)}>
          <Copy size={14} />
          Copy handoff
        </button>
      </div>
    </section>
  );
}

function Recommendations({ info }: { info: DebugResponse['info'] }) {
  const evidence = info.failure?.evidence_summary.length
    ? info.failure.evidence_summary
    : info.failure?.evidence ?? info.root_cause.evidence;
  const failure = info.failure;
  const primaryAction = failure?.primary_action ?? info.experience.next_step;
  const actions = removeRepeatedPrimaryAction(
    failure?.action_checklist.length ? failure.action_checklist : info.recommended_actions,
    primaryAction,
  );
  return (
    <div className="stack">
      <Panel title="What To Do Next" icon={<Play />}>
        <p className="lead lead--compact">{primaryAction}</p>
        {actions.length > 0 && (
          <ol className="list steps">
            {actions.map((action) => (
              <li key={action}>{action}</li>
            ))}
          </ol>
        )}
      </Panel>
      {failure && (
        <Panel title="How We Decoded This" icon={<Search />}>
          <ul className="list">
            {(failure.decode_explanation ? [failure.decode_explanation] : failure.decode_attempts).map((attempt) => (
              <li key={attempt}>{attempt}</li>
            ))}
            {failure.missing_artifact && <li>Missing artifact: {artifactLabel(failure.missing_artifact)}</li>}
          </ul>
        </Panel>
      )}
      {failure && isUnknownCustomFailure(failure) && (
        <Panel title="Copy Details" icon={<Copy />}>
          <div className="copy-stack">
            <CopyLine label="Program" value={failure.program_id ?? '-'} />
            <CopyLine label="Code" value={`${failure.code_decimal ?? '-'} / ${failure.code_hex ?? '-'}`} />
            <CopyLine label="Signature" value={info.signature} />
          </div>
        </Panel>
      )}
      <Panel title="Evidence Summary" icon={<Layers3 />}>
        <ul className="list">
          {evidence.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      </Panel>
      {info.metadata.fetch_warnings.length > 0 && (
        <Panel title="Fetch Warnings" icon={<AlertTriangle />}>
          <ul className="list warn">
            {info.metadata.fetch_warnings.map((warning) => (
              <li key={warning}>{warning}</li>
            ))}
          </ul>
        </Panel>
      )}
      {info.provider.warnings.length > 0 && (
        <Panel title="Connection Warnings" icon={<AlertTriangle />}>
          <ul className="list warn">
            {info.provider.warnings.map((warning) => (
              <li key={warning}>{warning}</li>
            ))}
          </ul>
        </Panel>
      )}
    </div>
  );
}

function removeRepeatedPrimaryAction(actions: string[], primaryAction: string) {
  const primary = normalizeActionText(primaryAction);
  return actions.filter((action) => normalizeActionText(action) !== primary);
}

function normalizeActionText(value: string) {
  return value.trim().replace(/\s+/g, ' ').replace(/\.$/, '').toLocaleLowerCase();
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function ComputePanel({ info }: { info: DebugResponse['info'] }) {
  const compute = info.resource_usage.execution_compute;
  const accountData = info.resource_usage.loaded_account_data;
  const size = info.resource_usage.transaction_size;
  return (
    <div className="stack">
      <Panel title="Compute + Fees" icon={<Cpu />}>
        <div className="success-grid">
          <Metric label="CU consumed" value={exactNumber(compute?.consumed_exact ?? info.compute_units_consumed_exact ?? info.compute_units_consumed)} />
          <Metric label="CU limit" value={exactNumber(compute?.limit_exact ?? info.compute_budget.compute_unit_limit_exact ?? info.compute_budget.compute_unit_limit)} />
          <Metric label="CU price" value={exactNumber(compute?.price_micro_lamports_exact ?? info.compute_budget.compute_unit_price_micro_lamports_exact ?? info.compute_budget.compute_unit_price_micro_lamports)} />
          <Metric label="Fee" value={lamports(info.fee_paid_exact ?? info.fee_paid)} />
          <Metric label="Heap frame" value={exactNumber(info.compute_budget.heap_frame_bytes_exact ?? info.compute_budget.heap_frame_bytes)} />
          <Metric label="Loaded data limit" value={exactNumber(accountData?.limit_exact ?? info.compute_budget.loaded_accounts_data_size_limit_exact ?? info.compute_budget.loaded_accounts_data_size_limit)} />
          <Metric label="Current fetched account-data bytes" value={exactNumber(accountData?.observed_account_data_bytes_exact ?? accountData?.observed_account_data_bytes)} />
          <Metric label="Serialized transaction size" value={exactNumber(size?.serialized_size_bytes_exact ?? size?.serialized_size_bytes)} />
        </div>
        {size?.note && <p className="muted">{size.note}</p>}
      </Panel>
      <Panel title="Per-Invocation Compute Evidence" icon={<Gauge />}>
        {info.compute_attribution.length > 0 ? (
          <div className="diagnosis-table">
            {info.compute_attribution.map((item, index) => (
              <div className="diagnosis-row" key={`${item.program_id}-${index}-${item.consumed_exact}`}>
                <span>{item.program_label}</span>
                <span>{exactNumber(item.consumed_exact)} / {exactNumber(item.limit_exact)} CU</span>
                <code title={item.program_id}>{shortAddress(item.program_id, 6, 6)}</code>
              </div>
            ))}
          </div>
        ) : (
          <p className="muted">Runtime compute logs were not available.</p>
        )}
      </Panel>
    </div>
  );
}

function CopyLine({ label, value }: { label: string; value: string }) {
  return (
    <div className="copy-line">
      <span>{label}</span>
      <code title={value}>{value.length > 30 ? shortAddress(value, 10, 10) : value}</code>
      <button type="button" aria-label={`Copy ${label}`} onClick={() => navigator.clipboard.writeText(value)}>
        <Copy size={14} />
      </button>
    </div>
  );
}

function isUnknownCustomFailure(failure: StandardizedFailure): boolean {
  return failure.code_decimal !== null && failure.name === null && failure.confidence === 'low';
}

function decodeStatusLabel(status: string): string {
  const labels: Record<string, string> = {
    decoded_registry: 'Decoded from registry',
    decoded_onchain_anchor_idl: 'Decoded from on-chain IDL',
    missing_onchain_idl: 'Needs program artifact',
    missing_registry: 'Needs program artifact',
    runtime_heuristic: 'Runtime heuristic',
  };
  return labels[status] ?? status.split('_').join(' ');
}

function artifactLabel(artifact: string): string {
  const labels: Record<string, string> = {
    program_idl_source_or_docs: 'Program IDL, source error enum, SDK error map, or docs',
    anchor_idl_source_enum_sdk_or_docs: 'Anchor IDL, source error enum, SDK error map, or docs',
  };
  return labels[artifact] ?? artifact.split('_').join(' ');
}

function integratorMessage(info: DebugResponse['info'], failure: StandardizedFailure): string {
  return [
    'Please provide the program IDL or error enum for this custom Solana error.',
    `Signature: ${info.signature}`,
    `Program: ${failure.program_label ?? 'Unknown'} (${failure.program_id ?? 'unknown'})`,
    `Instruction: ${failure.instruction_index ?? info.failing_instruction?.index ?? 'unknown'}`,
    `Custom code: ${failure.code_decimal ?? 'unknown'} (${failure.code_hex ?? 'unknown'})`,
    `Decode status: ${decodeStatusLabel(failure.decode_status)}`,
  ].join('\n');
}

function Instructions({ instructions, executionTree, decoded }: { instructions: InstructionDebugInfo[]; executionTree: ExecutionNode[]; decoded: DecodedInstruction[] }) {
  const decodedById = new Map(decoded.map((instruction) => [instruction.id, instruction]));
  return (
    <div className="stack">
      <Panel title="Outer Instructions" icon={<Layers3 />}>
        <div className="timeline">
          {instructions.map((ix) => (
            <article className={`step ${ix.error ? 'step--bad' : ''}`} key={ix.index}>
              <span className="step__index">#{ix.index}</span>
              <div>
                <strong>{ix.program_label}</strong>
                <code>{ix.program_id}</code>
                <small>{ix.accounts.length} accounts - discriminator {ix.discriminator ?? 'n/a'}</small>
                {ix.error && <p>{ix.error}</p>}
              </div>
              <ChevronRight size={16} />
            </article>
          ))}
        </div>
      </Panel>
      <Panel title="Execution Tree" icon={<Terminal />}>
        <div className="frames">
          {executionTree.map((node) => (
            <ExecutionFrame node={node} decoded={node.decoded_instruction_id ? decodedById.get(node.decoded_instruction_id) ?? null : null} key={node.id} />
          ))}
        </div>
      </Panel>
    </div>
  );
}

function ExecutionFrame({ node, decoded }: { node: ExecutionNode; decoded: DecodedInstruction | null }) {
  return (
    <div className={`frame frame--${node.failed ? 'failed' : node.status}`} style={{ '--frame-indent': `${node.depth * 18 + 10}px` } as React.CSSProperties}>
      <span>{node.failed ? 'failed' : node.status}</span>
      <code>{node.program_label}</code>
      <small>
        {decoded?.semantic_decode?.instruction_name ?? decoded?.id ?? node.program_id}
        {node.outer_instruction_index !== null ? ` · outer #${node.outer_instruction_index}` : ''}
        {node.inner_instruction_index !== null ? ` · inner #${node.inner_instruction_index}` : ''}
        {decoded?.stack_height ? ` · stack ${decoded.stack_height}` : ''}
      </small>
      {node.compute && (
        <small>
          Compute: {exactNumber(node.compute.consumed_exact)} / {exactNumber(node.compute.limit_exact)} CU
        </small>
      )}
      {node.token_instruction && (
        <div className="token-cpi" aria-label="Token instruction parameters">
          <strong>{tokenInstructionLabel(node.token_instruction.instruction_type)}</strong>
          {node.token_instruction.parameters.map((parameter) => (
            <span key={`${parameter.name}-${parameter.value}`}>
              {parameter.name}: <code title={parameter.value}>{formatTokenParam(parameter.value)}</code>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function tokenInstructionLabel(value: string) {
  return value.split('_').map(capitalize).join(' ');
}

function capitalize(value: string) {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

function formatTokenParam(value: string) {
  return value.length > 36 && /^[1-9A-HJ-NP-Za-km-z]+$/.test(value) ? shortAddress(value, 6, 6) : value;
}

function Accounts({ accounts }: { accounts: AccountEvidence[] }) {
  return (
    <Panel title="Account Evidence" icon={<Layers3 />}>
      <div className="table">
        <div className="thead">
          <span>#</span><span>Account</span><span>Owner</span><span>Flags</span><span>Delta</span>
        </div>
        {accounts.slice(0, 80).map((account) => (
          <div className="trow" key={`${account.index}-${account.pubkey}`}>
            <span data-label="Index">{account.index}</span>
            <code data-label="Account" title={account.pubkey}>{shortAddress(account.pubkey)}</code>
            <span data-label="Owner">{account.owner_label ?? shortAddress(account.owner ?? '-')}</span>
            <span data-label="Flags">{[account.signer && 'signer', account.writable && 'writable', account.executable && 'exec'].filter(Boolean).join(', ') || '-'}</span>
            <strong data-label="Delta">{signed(account.lamports_change_exact)}</strong>
          </div>
        ))}
      </div>
    </Panel>
  );
}

function Logs({ logs }: { logs: string[] }) {
  return (
    <Panel title="Program Logs" icon={<Terminal />}>
      <pre className="logs">{logs.map((log) => `  ${log}`).join('\n')}</pre>
    </Panel>
  );
}

function Raw({ text, json }: { text: string; json: unknown }) {
  return (
    <div className="stack">
      <Panel title="Text Report" icon={<FileText />}>
        <pre className="logs">{text}</pre>
      </Panel>
      <Panel title="JSON" icon={<Terminal />}>
        <pre className="logs">{JSON.stringify(json, null, 2)}</pre>
      </Panel>
    </div>
  );
}

function Panel({ title, icon, children }: { title: string; icon: React.ReactElement; children: React.ReactNode }) {
  return (
    <section className="panel">
      <div className="panel__head">
        {React.cloneElement(icon, { size: 17 })}
        <h2>{title}</h2>
      </div>
      {children}
    </section>
  );
}

function Chip({ label, value }: { label: string; value: string }) {
  return (
    <span className="chip">
      <small>{label}</small>
      <strong>{value}</strong>
    </span>
  );
}

function Row({ label, value, copy = false }: { label: string; value: string; copy?: boolean }) {
  return (
    <div className="row">
      <span>{label}</span>
      <code title={value}>{value.length > 38 ? shortAddress(value, 12, 12) : value}</code>
      {copy && (
        <button type="button" aria-label={`Copy ${label}`} onClick={() => navigator.clipboard.writeText(value)}>
          <Copy size={14} />
        </button>
      )}
      {value.startsWith('http') && <ExternalLink size={14} />}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
