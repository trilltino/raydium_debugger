import React from 'react';
import { getCorpusMedia, getCorpusReview, listCorpusReviews, saveCorpusReview } from '../api';
import type { CorpusDecision, CorpusReviewDetail, CorpusReviewList } from '../types';
import './review.css';

const pageSize = 30;
const label = (value: string | null | undefined) => value?.replace(/_/g, ' ') ?? '';

function initialDecision(detail: CorpusReviewDetail): CorpusDecision {
  const draft = detail.draft;
  const saved = detail.decision;
  const tier = draft?.tier;
  return {
    evidence_fingerprint: detail.packet.evidence_fingerprint,
    value_status: saved?.value_status ?? 'uncertain',
    outcome: saved?.outcome ?? (tier === 'reporter_confirmed' ? 'confirmed' : tier === 'team_fixed' ? 'team_fixed' : tier === 'proposed_only' ? 'proposed' : 'unknown'),
    category: saved?.category ?? draft?.category ?? '',
    diagnosis: saved?.diagnosis ?? draft?.diagnosis ?? '',
    summary: saved?.summary ?? '',
    resolution: saved?.resolution ?? draft?.resolution ?? '',
    guidance: saved?.guidance ?? draft?.general_guidance ?? '',
    product: saved?.product ?? '',
    failure_domain: saved?.failure_domain ?? '',
    evidence_revision_ids: saved?.evidence_revision_ids ?? [...new Set(draft?.message_evidence?.map((item) => item.revision_id) ?? [])],
    reference_ids: saved?.reference_ids ?? draft?.reference_ids ?? [],
    rationale: saved?.rationale ?? '',
    reviewer: saved?.reviewer ?? localStorage.getItem('raydium-reviewer') ?? '',
    action: 'save',
  };
}

export function Review() {
  const [list, setList] = React.useState<CorpusReviewList | null>(null);
  const [detail, setDetail] = React.useState<CorpusReviewDetail | null>(null);
  const [decision, setDecision] = React.useState<CorpusDecision | null>(null);
  const [query, setQuery] = React.useState('');
  const [search, setSearch] = React.useState('');
  const [kind, setKind] = React.useState('all');
  const [status, setStatus] = React.useState('all');
  const [value, setValue] = React.useState('all');
  const [offset, setOffset] = React.useState(0);
  const [refresh, setRefresh] = React.useState(0);
  const [loading, setLoading] = React.useState(false);
  const [saving, setSaving] = React.useState(false);
  const [checked, setChecked] = React.useState(false);
  const [error, setError] = React.useState('');
  const [notice, setNotice] = React.useState('');
  const listRef = React.useRef<HTMLElement>(null);
  const detailRef = React.useRef<HTMLElement>(null);

  React.useEffect(() => {
    const timer = window.setInterval(() => setRefresh((current) => current + 1), 60_000);
    return () => window.clearInterval(timer);
  }, []);

  React.useEffect(() => {
    let active = true;
    setLoading(true);
    listCorpusReviews({ search, kind, status, value, offset, limit: pageSize })
      .then((result) => { if (active) setList(result); })
      .catch((failure: unknown) => { if (active) setError(String(failure)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [search, kind, status, value, offset, refresh]);

  async function open(id: string) {
    setError(''); setNotice(''); setChecked(false);
    try {
      const result = await getCorpusReview(id);
      setDetail(result); setDecision(initialDecision(result));
      if (window.matchMedia('(max-width: 600px)').matches) {
        window.requestAnimationFrame(() => detailRef.current?.scrollIntoView({ block: 'start', behavior: 'smooth' }));
      }
    } catch (failure) { setError(String(failure)); }
  }

  function close() {
    setDetail(null); setDecision(null);
    if (window.matchMedia('(max-width: 600px)').matches) {
      window.requestAnimationFrame(() => listRef.current?.scrollIntoView({ block: 'start', behavior: 'smooth' }));
    }
  }

  function edit<K extends keyof CorpusDecision>(key: K, value: CorpusDecision[K]) {
    setDecision((current) => current ? { ...current, [key]: value } : current);
    setChecked(false);
  }

  async function submit(action: CorpusDecision['action']) {
    if (!detail || !decision) return;
    if ((action === 'approve' || action === 'publish_guidance') && !checked) { setError('Check the complete source messages and confirm below before publication.'); return; }
    setSaving(true); setError(''); setNotice('');
    try {
      const next = await saveCorpusReview(detail.packet.case_id, { ...decision, action });
      localStorage.setItem('raydium-reviewer', decision.reviewer);
      setDetail(next); setDecision(initialDecision(next)); setChecked(false);
      setNotice(action === 'approve' ? 'Historical incident approved and published.' : action === 'publish_guidance' ? 'Current guidance reviewed and published.' : action === 'reject' ? 'Rejected and removed from published knowledge.' : 'Review saved privately.');
      setList(await listCorpusReviews({ search, kind, status, value, offset, limit: pageSize }));
    } catch (failure) { setError(String(failure)); }
    finally { setSaving(false); }
  }

  return <main className="review-page" aria-label="Support knowledge review">
    <header className="review-intro">
      <div><p className="review-eyebrow">PRIVATE CORPUS / HUMAN REVIEW</p><h2>Support knowledge</h2>
        <p>Read the thread, correct the AI draft, classify its value, and cite the messages behind your decision. Approved historical fixes become searchable guidance for the debugger.</p></div>
      <div className="review-coverage" aria-label="Corpus coverage">
        <strong>{list?.packet_count.toLocaleString() ?? '…'}</strong><span>archive records</span>
        <small>{list?.case_count.toLocaleString() ?? '…'} cases · {list?.orphan_count.toLocaleString() ?? '…'} ungrouped</small>
        <small>{list?.classified_count.toLocaleString() ?? '…'} AI drafts · {list?.invalid_count.toLocaleString() ?? '…'} need retry · {list?.reviewed_count.toLocaleString() ?? '…'} reviewed · {list?.approved_count.toLocaleString() ?? '…'} incidents · {list?.guidance_count.toLocaleString() ?? '…'} guidance</small>
      </div>
    </header>
    <div className={`review-workspace ${detail ? 'review-workspace--selected' : ''}`}>
      <section className="review-list" ref={listRef} aria-label="Archive records">
        {detail && <button className="review-back" type="button" onClick={close}>Browse archive records</button>}
        <form className="review-filters" onSubmit={(event) => { event.preventDefault(); setOffset(0); setSearch(query); }}>
          <label>Search source text or category<input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search all messages" /></label>
          <button type="submit">Search</button>
          <label>Kind<select value={kind} onChange={(event) => { setKind(event.target.value); setOffset(0); }}><option value="all">All records</option><option value="case">Grouped cases</option><option value="orphan">Ungrouped messages</option></select></label>
          <label>Review<select value={status} onChange={(event) => { setStatus(event.target.value); setOffset(0); }}><option value="all">All states</option><option value="unreviewed">Unreviewed</option><option value="reviewed">Reviewed</option><option value="approved">Approved incidents</option><option value="published_guidance">Published guidance</option><option value="rejected">Rejected</option><option value="stale">Needs recheck</option></select></label>
          <label>Value<select value={value} onChange={(event) => { setValue(event.target.value); setOffset(0); }}><option value="all">Any value</option><option value="valuable">Valuable</option><option value="uncertain">Uncertain</option><option value="not_useful">Not useful</option><option value="unset">Not assessed</option></select></label>
        </form>
        <div className="review-list-head"><strong>{list?.filtered_count.toLocaleString() ?? '…'} matching</strong><span>{loading ? 'Loading…' : `${offset + 1}–${Math.min(offset + pageSize, list?.filtered_count ?? 0)}`}</span><button type="button" disabled={loading} onClick={() => setRefresh((current) => current + 1)}>Refresh</button></div>
        <div className="review-results">
          {list?.items.map((item) => <button type="button" key={item.id} className={`review-result ${detail?.packet.case_id === item.id ? 'selected' : ''}`} onClick={() => void open(item.id)}>
            <span className="review-result-meta"><b>{item.kind === 'case' ? `${item.message_count} message case` : 'Ungrouped message'}</b><small>{item.review_status}</small></span>
            <strong>{item.category || item.preview.slice(0, 85) || 'No text in source message'}</strong>
            <span>{item.preview || 'Attachment or empty text'}</span>
            <small>{item.ai_status === 'invalid_output' ? 'AI output needs retry or manual classification' : item.ai_outcome ? `AI: ${label(item.ai_outcome)} · ${label(item.ai_tier)}` : 'Awaiting AI classification'}{item.value_status ? ` · ${label(item.value_status)}` : ''}</small>
          </button>)}
        </div>
        <div className="review-pager"><button type="button" disabled={offset === 0 || loading} onClick={() => setOffset(Math.max(0, offset - pageSize))}>Previous</button><button type="button" disabled={loading || offset + pageSize >= (list?.filtered_count ?? 0)} onClick={() => setOffset(offset + pageSize)}>Next</button></div>
      </section>
      <section className="review-detail" ref={detailRef} aria-label="Evidence and decision">
        {error && <p className="review-error" role="alert">{error}</p>}
        {notice && <p className="review-notice" role="status">{notice}</p>}
        {!detail || !decision ? <div className="review-placeholder"><h3>Select a record</h3><p>Every prepared case and ungrouped message appears in the archive list. The original messages and AI evidence will appear here.</p></div> : <>
          <div className="review-detail-head"><div><p className="review-eyebrow">{detail.packet.kind} / {detail.review_status}</p><h3>{decision.category || 'Classify this record'}</h3><code>{detail.packet.case_id}</code></div><button type="button" onClick={close}>Browse archive</button></div>
          <section className="review-draft"><h4>AI suggestion</h4>{detail.draft?.status === 'invalid_output' ? <p>The model output did not pass evidence validation: {detail.draft.error}. You can classify this record manually while the worker retries it.</p> : detail.draft ? <><p><b>{label(detail.draft.outcome)}</b> · {label(detail.draft.tier)} · confidence {detail.draft.confidence}</p><p>{detail.draft.diagnosis}</p><p><b>Suggested resolution:</b> {detail.draft.resolution || 'No historical resolution established'}</p>{detail.draft.general_guidance && <p><b>Current guidance:</b> {detail.draft.general_guidance}</p>}{Boolean(detail.draft.unanswered_questions?.length) && <p><b>Still unanswered:</b> {detail.draft.unanswered_questions?.join('; ')}</p>}</> : <p>Classification is still pending or its evidence has changed.</p>}</section>
          <section className="review-messages"><h4>Original conversation and evidence</h4>{detail.packet.messages.map((message) => <article key={message.revision_id} className="review-message">
            <label className="review-evidence"><input type="checkbox" checked={decision.evidence_revision_ids.includes(message.revision_id)} onChange={(event) => edit('evidence_revision_ids', event.target.checked ? [...decision.evidence_revision_ids, message.revision_id] : decision.evidence_revision_ids.filter((id) => id !== message.revision_id))} /><span>Cite revision {message.revision_id}</span></label>
            <small>{message.sender || 'Unknown sender'} · {message.date || 'Undated'} · source {message.source_message_id}</small>
            <p>{message.body || <em>No extracted text; inspect attachments.</em>}</p>
            {message.attachments?.map((attachment, index) => <details key={index}><summary>Attachment · {attachment.relative_path || `#${index + 1}`} · {attachment.status}{attachment.visual_review_needed ? ' · visual check needed' : ''}</summary><pre>{attachment.text || 'No extracted text'}</pre><MediaPreview id={detail.packet.case_id} revision={message.revision_id} index={index} /></details>)}
            {detail.draft?.message_evidence?.filter((evidence) => evidence.revision_id === message.revision_id).map((evidence, index) => <blockquote key={index}>AI cited: “{evidence.quote}”</blockquote>)}
            {detail.rule_signals.filter((signal) => signal.revision_id === message.revision_id).map((signal) => <small key={signal.signal} className="review-rule">Local rule: {label(signal.signal)}</small>)}
          </article>)}</section>
          {(detail.packet.references?.length > 0 || detail.packet.upgrade_context?.length > 0) && <details className="review-context"><summary>Documentation, code, and dated updates ({detail.packet.references?.length ?? 0} references, {detail.packet.upgrade_context?.length ?? 0} updates)</summary>
            {detail.packet.references?.map((reference) => <article key={reference.id}><label className="review-evidence"><input type="checkbox" checked={decision.reference_ids.includes(reference.id)} onChange={(event) => edit('reference_ids', event.target.checked ? [...decision.reference_ids, reference.id] : decision.reference_ids.filter((id) => id !== reference.id))} /><span>Cite this reference</span></label><b>{reference.repo} · {reference.path}:{reference.start_line}</b><small>Commit {reference.commit} · {reference.id}</small><pre>{reference.text}</pre></article>)}
            {detail.packet.upgrade_context?.map((update) => <article key={update.id}><b>{update.date} · {update.status} · {update.id}</b><p>{update.summary || update.body_excerpt}</p></article>)}
          </details>}
          <section className="review-editor"><h4>Your decision</h4>
            <div className="review-form-grid">
              <label>Value<select value={decision.value_status} onChange={(event) => edit('value_status', event.target.value as CorpusDecision['value_status'])}><option value="valuable">Valuable</option><option value="uncertain">Uncertain</option><option value="not_useful">Not useful</option></select></label>
              <label>Historical outcome<select value={decision.outcome} onChange={(event) => edit('outcome', event.target.value as CorpusDecision['outcome'])}><option value="confirmed">Reporter confirmed</option><option value="team_fixed">Team stated fix</option><option value="proposed">Proposed only</option><option value="unknown">Unknown</option></select></label>
              <label>Category<input value={decision.category} onChange={(event) => edit('category', event.target.value)} /></label>
              <label>Product<input value={decision.product} onChange={(event) => edit('product', event.target.value)} placeholder="e.g. CLMM" /></label>
              <label>Failure domain<input value={decision.failure_domain} onChange={(event) => edit('failure_domain', event.target.value)} placeholder="e.g. swap UI" /></label>
              <label>Reviewer<input value={decision.reviewer} onChange={(event) => edit('reviewer', event.target.value)} placeholder="Your name" /></label>
            </div>
            <label>Diagnosis<textarea value={decision.diagnosis} onChange={(event) => edit('diagnosis', event.target.value)} rows={3} /></label>
            <label>Sanitized problem summary<textarea value={decision.summary} onChange={(event) => edit('summary', event.target.value)} rows={2} maxLength={500} /></label>
            <label>Historical resolution<textarea value={decision.resolution} onChange={(event) => edit('resolution', event.target.value)} rows={3} maxLength={1000} /></label>
            <label>Current guidance or unresolved follow-up<textarea value={decision.guidance} onChange={(event) => edit('guidance', event.target.value)} rows={3} /></label>
            <label>Why this classification is supported<textarea value={decision.rationale} onChange={(event) => edit('rationale', event.target.value)} rows={3} placeholder="Explain what the cited messages establish, and what remains uncertain" /></label>
            <p className="review-boundary">Historical incidents require a grouped case, confirmed or explicit team fix, and cited messages. Current guidance can be published from any reviewed record with a cited problem message; it is labeled as advice rather than a past fix. Private drafts never reach the AI.</p>
            <label className="review-confirm"><input type="checkbox" checked={checked} onChange={(event) => setChecked(event.target.checked)} /> I checked the complete source messages and cited evidence for this decision.</label>
            <div className="review-actions"><button type="button" disabled={saving} onClick={() => void submit('save')}>Save classification</button>{detail.packet.kind === 'case' && <button type="button" disabled={saving} onClick={() => void submit('reject')}>Reject incident</button>}<button type="button" disabled={saving || !checked} onClick={() => void submit('publish_guidance')}>Publish current guidance</button><button className="primary" type="button" disabled={saving || detail.packet.kind !== 'case' || !checked} onClick={() => void submit('approve')}>Approve historical fix</button></div>
          </section>
        </>}
      </section>
    </div>
  </main>;
}

function MediaPreview({ id, revision, index }: { id: string; revision: number; index: number }) {
  const [url, setUrl] = React.useState('');
  const [mime, setMime] = React.useState('');
  const [error, setError] = React.useState('');
  const [loading, setLoading] = React.useState(false);
  React.useEffect(() => () => { if (url) URL.revokeObjectURL(url); }, [url]);
  async function load() {
    setLoading(true); setError('');
    try {
      const blob = await getCorpusMedia(id, revision, index);
      setMime(blob.type); setUrl(URL.createObjectURL(blob));
    } catch (failure) { setError(String(failure)); }
    finally { setLoading(false); }
  }
  return <div className="review-media">
    {!url && <button type="button" disabled={loading} onClick={() => void load()}>{loading ? 'Loading original…' : 'View verified original'}</button>}
    {error && <small role="alert">{error}</small>}
    {url && mime.startsWith('image/') && <img src={url} alt={`Original attachment for revision ${revision}`} />}
    {url && mime.startsWith('video/') && <video src={url} controls preload="metadata" />}
    {url && mime === 'application/pdf' && <a href={url} target="_blank" rel="noreferrer">Open original PDF</a>}
  </div>;
}
