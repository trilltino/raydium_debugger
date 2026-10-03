import React from 'react';
import { ArrowUpRight, BookOpen, Search } from 'lucide-react';
import { glossary, updates, type Product } from './updates';
import './help.css';

const products: Product[] = ['CLMM', 'CPMM', 'AMM v4', 'Stable AMM', 'LaunchLab'];
function date(value: string) {
  return new Date(`${value}T12:00:00Z`).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' });
}

export function Help() {
  const [query, setQuery] = React.useState('');
  const [product, setProduct] = React.useState('all');
  const [status, setStatus] = React.useState('all');
  const normalized = query.trim().toLowerCase();
  const shown = updates.filter((update) =>
    (product === 'all' || update.products.includes(product as Product)) &&
    (status === 'all' || update.status === status) &&
    [update.title, update.summary, update.audience, update.action, update.compatibility, update.check, ...update.products, ...update.technical].join(' ').toLowerCase().includes(normalized),
  );
  const filtered = Boolean(query || product !== 'all' || status !== 'all');
  function reset() { setQuery(''); setProduct('all'); setStatus('all'); }

  return (
    <main className="help shell" id="help-content">
      <header className="help__intro">
        <div className="help__eyebrow"><BookOpen size={16} /> Updates / Release notes explained</div>
        <h2>Developer updates</h2>
        <p>What changed in Raydium, who it affects, and what to check when debugging.</p>
        <div className="help__scope"><span className="help__snapshot">Snapshot</span> Public announcements through <strong>28 September 2026</strong>. This is a saved export, not a live release feed. “Announcement only” means deployment was not confirmed in this export.</div>
      </header>

      <section className="help__tools" aria-label="Filter developer updates">
        <label className="field help__search"><span>Search updates</span><div><Search size={17} aria-hidden="true" /><input aria-label="Search updates" type="search" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Try frozen NFT, CPI, fees…" /></div></label>
        <label className="field"><span>Product</span><select aria-label="Product" value={product} onChange={(event) => setProduct(event.target.value)}><option value="all">All products</option>{products.map((item) => <option key={item}>{item}</option>)}</select></label>
        <label className="field"><span>Deployment evidence</span><select aria-label="Deployment evidence" value={status} onChange={(event) => setStatus(event.target.value)}><option value="all">All announcements</option><option>Confirmed deployed</option><option>Announcement only</option></select></label>
        <button className="secondary help__reset" onClick={reset} disabled={!filtered}>Reset filters</button>
      </section>

      <div className="help__results"><p role="status">{shown.length} of {updates.length} updates{filtered ? ' match your filters' : ' · newest first'}</p><a href="#help-glossary" onClick={(event) => { event.preventDefault(); document.getElementById('help-glossary')?.scrollIntoView({ behavior: 'smooth' }); document.getElementById('help-glossary')?.focus(); }}>New to these terms? <span aria-hidden="true">↓</span></a></div>

      <section className="help__timeline" aria-label="Developer update summaries">
        {!shown.length && <div className="help__empty"><h3>No matching updates</h3><p>Try a broader term or choose another product.</p><button className="secondary" onClick={reset}>Show all updates</button></div>}
        {shown.map((update) => <article className="help__update" key={update.id} id={`update-${update.id}`}>
          <div className="help__date"><time dateTime={update.announced}>{date(update.announced)}</time><span>Announced</span></div>
          <div className="help__body">
            <div className="help__tags">{update.products.map((item) => <span className="help__product" key={item}>{item}</span>)}<span className={`help__status ${update.confirmed ? 'help__status--confirmed' : ''}`}>{update.status}</span></div>
            <h3>{update.title}</h3>
            <p className="help__summary">{update.summary}</p>
            {update.confirmed && <p className="help__confirmed">Deployment confirmation: <time dateTime={update.confirmed}>{date(update.confirmed)}</time></p>}
            <dl className="help__facts"><div><dt>Who this affects</dt><dd>{update.audience}</dd></div><div><dt>Compatibility</dt><dd>{update.compatibility}</dd></div><div><dt>What to do</dt><dd>{update.action}</dd></div></dl>
            <details className="help__detail"><summary>Technical details & debugger checks</summary><div><h4>Under the hood</h4>{update.technical.map((paragraph) => <p key={paragraph}>{paragraph}</p>)}<div className="help__check"><h4>What to check in the debugger</h4><p>{update.check}</p></div></div></details>
            <footer className="help__sources"><span>Sources</span>{update.messages.map((id) => <a key={id} href={`https://t.me/RaydiumDeveloperUpdates/${id}`} target="_blank" rel="noreferrer">Announcement #{id}<ArrowUpRight size={13} aria-hidden="true" /></a>)}{update.changelogs.map((url, index) => <a key={url} href={url} target="_blank" rel="noreferrer">{url.includes('github.com') ? 'Code / reference' : url.includes('rust-cpi') ? 'Rust CPI guide' : url.includes('curve-rules#') ? 'Devnet guide' : update.changelogs.length > 1 ? `Docs ${index + 1}` : 'Changelog'}<ArrowUpRight size={13} aria-hidden="true" /></a>)}</footer>
          </div>
        </article>)}
      </section>

      <section className="help__glossary" id="help-glossary" tabIndex={-1} aria-labelledby="glossary-title"><h3 id="glossary-title">A few terms, in plain English</h3><p>Quick definitions for reading the updates and transaction evidence.</p><dl>{glossary.map(([term, definition]) => <div key={term}><dt>{term}</dt><dd>{definition}</dd></div>)}</dl></section>
    </main>
  );
}

