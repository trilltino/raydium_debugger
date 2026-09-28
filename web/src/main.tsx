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
  debugTransaction,
  getProviderStatus,
  listCasebooks,
  listIntegrators,
  saveCasebookSignature,
  saveIntegratorSignature,
} from './api';
import { dateTime, exactNumber, lamports, shortAddress, signed } from './format';
import type {
  AccountEvidence,
  CasebookRecord,
  CpiFrame,
  DebugResponse,
  InstructionDebugInfo,
  IntegratorRecord,
  ProviderStatus,
  SavedSignature,
  StandardizedFailure,
} from './types';
import './styles.css';

type Tab = 'summary' | 'instructions' | 'accounts' | 'logs' | 'raw';

function App() {
  const [signature, setSignature] = React.useState('');
  const [cluster, setCluster] = React.useState<'devnet' | 'mainnet'>('devnet');
  const [providers, setProviders] = React.useState<ProviderStatus | null>(null);
  const [activeTab, setActiveTab] = React.useState<Tab>('summary');
  const [response, setResponse] = React.useState<DebugResponse | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [question, setQuestion] = React.useState('');
  const [aiModel, setAiModel] = React.useState('');
  const [aiAnswer, setAiAnswer] = React.useState<string | null>(null);
  const [asking, setAsking] = React.useState(false);
  const [integrators, setIntegrators] = React.useState<IntegratorRecord[]>([]);
  const [selectedIntegratorId, setSelectedIntegratorId] = React.useState('');
  const [casebooks, setCasebooks] = React.useState<CasebookRecord[]>([]);
  const [selectedCasebookId, setSelectedCasebookId] = React.useState('');
  const [casebookFilter, setCasebookFilter] = React.useState('all');
  const [newIntegratorName, setNewIntegratorName] = React.useState('');
  const [newCasebookName, setNewCasebookName] = React.useState('');
  const [savingSignature, setSavingSignature] = React.useState(false);

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
        setSelectedCasebookId((current) => current || records[0]?.id || '');
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
    try {
      const next = await debugTransaction({
        signature: signature.trim(),
        cluster,
        data_mode: 'auto',
      });
      setResponse(next);
      setActiveTab('summary');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  async function runAsk(event: React.FormEvent) {
    event.preventDefault();
    if (!response || !question.trim()) return;
    setAsking(true);
    setError(null);
    try {
      const answer = await askAi({
        info: response.info,
        question: question.trim(),
        model: aiModel.trim() || null,
      });
      setAiAnswer(answer.answer);
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
    setSavingSignature(true);
    setError(null);
    try {
      const request = {
        signature: signature.trim(),
        cluster,
        label: signatureLabel(response),
        reason: signatureReason(response),
        outcome: response ? (response.info.success ? 'success' : 'failed') : null,
        product: response?.info.raydium_context?.product ?? response?.info.raydium_product?.product ?? null,
        failure_category: response?.info.failure?.category ?? null,
        failure_code: response?.info.failure?.code_hex ?? null,
        tags: signatureTags(response),
        pinned: response ? !response.info.success : false,
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
    setAiAnswer(null);
    setActiveTab('summary');
  }

  const info = response?.info ?? null;
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
          <span>Debug</span>
          <span>Casebooks</span>
          <span>Evidence</span>
        </nav>
      </header>

      <main className="shell">
        <section className="swap-console" aria-label="Transaction debugger console">
          <form className="query" onSubmit={runDebug}>
            <label className="field field--wide">
              <span>Transaction signature</span>
              <input
                value={signature}
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

        {!info || !debug ? (
          <EmptyState />
        ) : (
          <>
            <StatusStrip response={debug} />
            <div className="layout">
              <section className="content">
                <Tabs active={activeTab} onChange={setActiveTab} />
                {activeTab === 'summary' && <Summary info={info} onAsk={runAsk} question={question} setQuestion={setQuestion} aiModel={aiModel} setAiModel={setAiModel} asking={asking} answer={aiAnswer} />}
                {activeTab === 'instructions' && <Instructions instructions={info.outer_instructions} cpi={info.cpi_tree} />}
                {activeTab === 'accounts' && <Accounts accounts={info.accounts} />}
                {activeTab === 'logs' && <Logs logs={info.logs} />}
                {activeTab === 'raw' && <Raw text={debug.formatted_text} json={info} />}
              </section>
              <aside className="side">
                <Recommendations info={info} />
              </aside>
            </div>
          </>
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

function signatureLabel(response: DebugResponse | null): string {
  if (!response) return 'Saved transaction';
  return response.info.failure?.plain_title ?? response.info.failure?.title ?? response.info.experience.headline;
}

function signatureReason(response: DebugResponse | null): string | null {
  if (!response) return null;
  return response.info.failure?.primary_action ?? response.info.experience.next_step;
}

function signatureTags(response: DebugResponse | null): string[] {
  if (!response) return [];
  return [
    response.info.provider.cluster,
    response.info.raydium_context?.product ?? response.info.raydium_product?.product,
    response.info.failure?.category,
    response.info.failure?.missing_artifact ? 'needs-idl' : null,
    response.info.success ? 'success' : 'failed',
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

function StatusStrip({ response }: { response: DebugResponse }) {
  const { info } = response;
  const failure = info.failure;
  const tone = experienceTone(info.experience.tone);
  return (
    <section className="stats">
      <Stat label="Status" value={info.experience.status_label} tone={tone} hint={info.experience.headline} icon={info.success ? <CheckCircle2 /> : <XCircle />} />
      <Stat label="Diagnosis" value={failure?.name ?? info.root_cause.category} hint={failure?.title ?? info.experience.message} tone={failure ? 'warn' : tone} icon={<ShieldAlert />} />
      <Stat label="Slot" value={exactNumber(info.slot_exact ?? info.slot)} hint={info.freshness.note} icon={<Gauge />} />
      <Stat label="Compute" value={exactNumber(info.compute_units_consumed_exact ?? info.compute_units_consumed)} hint={`Fee ${lamports(info.fee_paid_exact ?? info.fee_paid)}`} icon={<Cpu />} />
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
    ['instructions', 'Instructions'],
    ['accounts', 'Accounts'],
    ['logs', 'Logs'],
    ['raw', 'Raw'],
  ];
  return (
    <div className="tabs">
      {tabs.map(([key, label]) => (
        <button key={key} type="button" className={active === key ? 'is-active' : ''} onClick={() => onChange(key)}>
          {label}
        </button>
      ))}
    </div>
  );
}

function Summary(props: {
  info: DebugResponse['info'];
  onAsk: (event: React.FormEvent) => void;
  question: string;
  setQuestion: (value: string) => void;
  aiModel: string;
  setAiModel: (value: string) => void;
  asking: boolean;
  answer: string | null;
}) {
  const { info } = props;
  const failure = info.failure;
  const headline = failure?.plain_title ?? info.experience.headline;
  const explanation = failure?.plain_explanation ?? info.experience.message;
  const primaryAction = failure?.primary_action ?? info.experience.next_step;
  return (
    <div className="stack">
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
        {props.answer && <pre className="answer">{props.answer}</pre>}
      </Panel>
    </div>
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

function Instructions({ instructions, cpi }: { instructions: InstructionDebugInfo[]; cpi: CpiFrame[] }) {
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
      <Panel title="CPI Tree" icon={<Terminal />}>
        <div className="frames">
          {cpi.map((frame, index) => (
            <Frame frame={frame} key={`${frame.message}-${index}`} />
          ))}
        </div>
      </Panel>
    </div>
  );
}

function Frame({ frame }: { frame: CpiFrame }) {
  return (
    <div className={`frame frame--${frame.status}`} style={{ paddingLeft: `${frame.depth * 18 + 10}px` }}>
      <span>{frame.status}</span>
      <code>{frame.program_label}</code>
      <small>{frame.message}</small>
      {frame.token_instruction && (
        <div className="token-cpi" aria-label="Token instruction parameters">
          <strong>{tokenInstructionLabel(frame.token_instruction.instruction_type)}</strong>
          {frame.token_instruction.parameters.map((parameter) => (
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
            <span>{account.index}</span>
            <code title={account.pubkey}>{shortAddress(account.pubkey)}</code>
            <span>{account.owner_label ?? shortAddress(account.owner ?? '-')}</span>
            <span>{[account.signer && 'signer', account.writable && 'writable', account.executable && 'exec'].filter(Boolean).join(', ') || '-'}</span>
            <strong>{signed(account.lamports_change_exact)}</strong>
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
