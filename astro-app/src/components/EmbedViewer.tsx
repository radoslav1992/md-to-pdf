import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';

/**
 * Chrome-less version of {@link ShareViewer}. Reads the share token
 * from `?t=<token>`, optionally accepts a password via `?p=<password>`
 * (caveat: query-string auth has the usual referer / log leak issues —
 * the embedder should weigh that), and fills the viewport with a
 * sandboxed iframe containing the rendered HTML.
 *
 * The outer `embed.astro` wrapper has no nav / footer, and Caddy is
 * configured to allow framing this page cross-origin, so embedding
 * Just Works (™).
 */
export default function EmbedViewer() {
  const [token, setToken] = useState('');
  const [view, setView] = useState<{ title: string; html: string } | null>(null);
  const [requiresPassword, setRequiresPassword] = useState<boolean | null>(null);
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    setToken(params.get('t') ?? '');
    // Query-string password is convenient for embedders but optional;
    // the password form below is the recommended path for human visitors.
    const p = params.get('p');
    if (p) setPassword(p);
  }, []);

  const load = useCallback(
    async (pw?: string) => {
      if (!token) return;
      setLoading(true);
      setError(null);
      try {
        const res = await api.shareView(token, pw);
        setView({
          title: res.title,
          html: res.rendered_html ?? '<p><em>No rendered content stored for this document.</em></p>',
        });
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setLoading(false);
      }
    },
    [token],
  );

  useEffect(() => {
    if (!token) return;
    let cancelled = false;
    api.shareMeta(token).then(
      (meta) => {
        if (cancelled) return;
        setRequiresPassword(meta.requires_password);
        if (!meta.requires_password) {
          void load();
        } else if (password) {
          // Auto-submit if the embedder passed `?p=...`.
          void load(password);
        }
      },
      (err) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
      },
    );
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token]);

  if (!token) {
    return (
      <div style={{ padding: '2rem', color: '#7c2d12' }}>
        Missing token. Expected URL like <code>/embed?t=&lt;token&gt;</code>.
      </div>
    );
  }

  if (error && !view) {
    return (
      <div style={{ padding: '2rem', color: '#b91c1c' }}>{error}</div>
    );
  }

  if (requiresPassword === null) {
    return <div style={{ padding: '2rem', color: '#78716c' }}>Loading…</div>;
  }

  if (!view) {
    return (
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void load(password);
        }}
        style={{
          padding: '1.5rem',
          maxWidth: 360,
          margin: '4rem auto',
          fontFamily: 'system-ui, sans-serif',
        }}
      >
        <h2 style={{ fontSize: '1rem', margin: '0 0 0.75rem' }}>
          Password required
        </h2>
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          autoFocus
          style={{
            width: '100%',
            padding: '0.5rem 0.75rem',
            border: '1px solid #d6d3d1',
            borderRadius: 6,
            fontSize: '0.9rem',
            boxSizing: 'border-box',
          }}
        />
        <button
          type="submit"
          disabled={loading}
          style={{
            marginTop: 8,
            padding: '0.5rem 1rem',
            background: '#0d9488',
            color: 'white',
            border: 0,
            borderRadius: 6,
            cursor: 'pointer',
            fontSize: '0.9rem',
          }}
        >
          {loading ? 'Loading…' : 'View'}
        </button>
        {error && (
          <div
            style={{
              marginTop: 12,
              padding: '0.5rem 0.75rem',
              border: '1px solid #fecaca',
              background: '#fef2f2',
              color: '#b91c1c',
              borderRadius: 6,
              fontSize: '0.85rem',
            }}
          >
            {error}
          </div>
        )}
      </form>
    );
  }

  return (
    <iframe
      title={view.title}
      srcDoc={view.html}
      sandbox="allow-same-origin"
      style={{
        width: '100%',
        height: '100vh',
        border: 0,
        display: 'block',
      }}
    />
  );
}
