import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';

export default function ShareViewer() {
  const [token, setToken] = useState<string>('');

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const t = params.get('t');
    if (t) setToken(t);
  }, []);

  if (!token) {
    return <ShareViewerBody token="" missing />;
  }
  return <ShareViewerBody token={token} />;
}

interface BodyProps {
  token: string;
  missing?: boolean;
}

function ShareViewerBody({ token, missing }: BodyProps) {
  const [requiresPassword, setRequiresPassword] = useState<boolean | null>(null);
  const [password, setPassword] = useState('');
  const [view, setView] = useState<{ title: string; html: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [expiresAt, setExpiresAt] = useState<number | null>(null);

  useEffect(() => {
    if (!token) return;
    let cancelled = false;
    api.shareMeta(token).then(
      (meta) => {
        if (cancelled) return;
        setRequiresPassword(meta.requires_password);
        setExpiresAt(meta.expires_at);
        if (!meta.requires_password) void load();
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

  const load = useCallback(
    async (pw?: string) => {
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

  if (missing) {
    return (
      <div className="max-w-xl mx-auto py-20 text-center">
        <div className="bg-amber-50 border border-amber-200 text-amber-800 px-4 py-3 rounded-lg">
          Missing share token. Expected URL like <code>/s?t=&lt;token&gt;</code>.
        </div>
      </div>
    );
  }

  if (error && !view) {
    return (
      <div className="max-w-xl mx-auto py-20 text-center">
        <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg">
          {error}
        </div>
      </div>
    );
  }

  if (requiresPassword === null) {
    return <p className="text-center text-slate-500 py-20">Loading…</p>;
  }

  if (!view) {
    return (
      <div className="max-w-md mx-auto py-20">
        <h2 className="text-lg font-semibold text-slate-900 mb-3">This share is password-protected</h2>
        <p className="text-sm text-slate-600 mb-4">
          Enter the password the owner gave you.
        </p>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void load(password);
          }}
          className="flex gap-2"
        >
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="flex-1 border border-slate-300 rounded-md px-3 py-1.5 text-sm bg-white"
            placeholder="Password"
            autoFocus
          />
          <button
            type="submit"
            disabled={loading}
            className="bg-brand-600 hover:bg-brand-700 text-white px-4 py-1.5 rounded-md text-sm disabled:opacity-50"
          >
            {loading ? 'Loading…' : 'View'}
          </button>
        </form>
        {error && (
          <div className="mt-3 text-sm text-red-700 bg-red-50 border border-red-200 rounded px-3 py-2">
            {error}
          </div>
        )}
        {expiresAt && (
          <p className="mt-4 text-xs text-slate-500">
            Expires {new Date(expiresAt * 1000).toLocaleString()}.
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="max-w-5xl mx-auto py-6 px-4">
      <div className="mb-3 flex items-baseline justify-between">
        <h1 className="text-xl font-semibold text-slate-900">{view.title}</h1>
        {expiresAt && (
          <span className="text-xs text-slate-500">
            expires {new Date(expiresAt * 1000).toLocaleString()}
          </span>
        )}
      </div>
      <iframe
        title={view.title}
        srcDoc={view.html}
        sandbox="allow-same-origin"
        className="w-full h-[80vh] border border-slate-200 rounded-lg bg-white"
      />
    </div>
  );
}
