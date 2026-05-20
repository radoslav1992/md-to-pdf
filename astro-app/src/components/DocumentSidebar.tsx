import { useCallback, useEffect, useState } from 'react';
import {
  api,
  type DocumentVersionSummary,
  type ShareLink,
} from '../lib/api';

interface Props {
  documentId: number;
  isPremium: boolean;
  /** Called when a restore succeeds so the editor can reload the document. */
  onRestored: () => void;
}

/**
 * Build the absolute URL an embedder needs in their `<iframe src="…">`.
 * We use the current origin so the snippet works whether the user is
 * on localhost, a Hetzner IP, or a custom domain — the operator never
 * has to configure anything.
 */
function absoluteEmbedUrl(token: string): string {
  if (typeof window === 'undefined') return `/embed?t=${token}`;
  return `${window.location.origin}/embed?t=${token}`;
}

export default function DocumentSidebar({ documentId, isPremium, onRestored }: Props) {
  const [versions, setVersions] = useState<DocumentVersionSummary[] | null>(null);
  const [shares, setShares] = useState<ShareLink[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [newShare, setNewShare] = useState<{ token: string; url: string } | null>(null);

  // Share form
  const [shareFormat, setShareFormat] = useState<'html' | 'pdf'>('html');
  const [sharePassword, setSharePassword] = useState('');
  const [shareExpiresDays, setShareExpiresDays] = useState<string>('');

  const loadVersions = useCallback(async () => {
    try {
      const res = await api.listVersions(documentId);
      setVersions(res.items);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [documentId]);

  const loadShares = useCallback(async () => {
    if (!isPremium) {
      setShares([]);
      return;
    }
    try {
      const res = await api.listShares(documentId);
      setShares(res.items);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [documentId, isPremium]);

  useEffect(() => {
    void loadVersions();
    void loadShares();
  }, [loadVersions, loadShares]);

  const onRestore = useCallback(
    async (vid: number) => {
      if (!confirm('Restore this version? The current state will be snapshotted first.')) return;
      setError(null);
      setInfo(null);
      try {
        await api.restoreVersion(documentId, vid);
        setInfo('Version restored.');
        await loadVersions();
        onRestored();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      }
    },
    [documentId, loadVersions, onRestored],
  );

  const onCreateShare = useCallback(async () => {
    setError(null);
    setInfo(null);
    setNewShare(null);
    try {
      const days = parseInt(shareExpiresDays, 10);
      const res = await api.createShare(documentId, {
        format: shareFormat,
        password: sharePassword.trim() || null,
        expires_in_seconds: !isNaN(days) && days > 0 ? days * 86400 : null,
      });
      setNewShare({ token: res.token, url: `/s?t=${res.token}` });
      setSharePassword('');
      setShareExpiresDays('');
      await loadShares();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [documentId, shareFormat, sharePassword, shareExpiresDays, loadShares]);

  const onRevokeShare = useCallback(
    async (id: number) => {
      if (!confirm('Revoke this share link? Anyone with the URL will lose access.')) return;
      try {
        await api.revokeShare(id);
        await loadShares();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      }
    },
    [loadShares],
  );

  return (
    <div className="space-y-4">
      {error && (
        <div className="text-xs text-danger-700 bg-danger-50 border border-danger-100 rounded px-3 py-2">
          {error}
        </div>
      )}
      {info && !error && (
        <div className="text-xs text-success-700 bg-success-50 border border-success-100 rounded px-3 py-2">
          {info}
        </div>
      )}

      <details open className="border border-stone-200 rounded-lg bg-white dark:bg-stone-900 dark:border-stone-800">
        <summary className="cursor-pointer px-3 py-2 text-sm font-medium text-stone-700 select-none">
          Version history {versions ? `(${versions.length})` : ''}
        </summary>
        <div className="p-3 border-t border-stone-200">
          {versions === null ? (
            <p className="text-xs text-stone-500">Loading…</p>
          ) : versions.length === 0 ? (
            <p className="text-xs text-stone-500">
              No prior versions yet. Each update snapshots the previous state here.
            </p>
          ) : (
            <ul className="space-y-1.5 max-h-[300px] overflow-y-auto">
              {versions.map((v) => (
                <li key={v.id} className="flex items-center justify-between gap-2 text-xs">
                  <div>
                    <span className="font-mono">v{v.version}</span>
                    <span className="ml-2 text-stone-500">
                      {new Date(v.created_at * 1000).toLocaleString()}
                    </span>
                    <span className="ml-2 text-stone-400">({v.content_bytes} bytes)</span>
                  </div>
                  <button
                    type="button"
                    onClick={() => void onRestore(v.id)}
                    className="text-brand-600 hover:text-brand-700"
                  >
                    Restore
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </details>

      {/* Share is the most-asked-for action after saving — open by default so
          users find it without hunting through accordions. */}
      <details open className="border border-stone-200 rounded-lg bg-white dark:bg-stone-900 dark:border-stone-800">
        <summary className="cursor-pointer px-3 py-2 text-sm font-medium text-stone-700 select-none dark:text-stone-200">
          Share links {shares ? `(${shares.length})` : ''} {isPremium ? '' : '· Premium'}
        </summary>
        {!isPremium ? (
          /* Premium-only feature. Previously we dimmed the controls with
             `pointer-events-none` and gave no explanation — users saw a
             greyed-out share button and assumed it was broken. Show an
             explicit upgrade card instead. */
          <div className="p-3 border-t border-stone-200 dark:border-stone-800 text-xs text-stone-600 dark:text-stone-300 space-y-2">
            <p>
              Public share links and embeddable iframes are a premium feature.
            </p>
            <a
              href="/pricing"
              className="inline-block bg-brand-600 hover:bg-brand-700 text-white px-3 py-1.5 rounded-md font-medium"
            >
              Upgrade to share →
            </a>
          </div>
        ) : (
        <div className="p-3 border-t border-stone-200 dark:border-stone-800">
          <div className="grid grid-cols-2 gap-2">
            <select
              value={shareFormat}
              onChange={(e) => setShareFormat(e.target.value as 'html' | 'pdf')}
              className="border border-stone-300 rounded-md px-2 py-1 text-xs bg-white dark:bg-stone-800 dark:border-stone-700 dark:text-stone-100"
            >
              <option value="html">HTML</option>
              <option value="pdf">PDF</option>
            </select>
            <input
              type="number"
              min={1}
              value={shareExpiresDays}
              onChange={(e) => setShareExpiresDays(e.target.value)}
              placeholder="Expires (days)"
              className="border border-stone-300 rounded-md px-2 py-1 text-xs bg-white dark:bg-stone-800 dark:border-stone-700 dark:text-stone-100 dark:placeholder:text-stone-500"
            />
          </div>
          <input
            type="text"
            value={sharePassword}
            onChange={(e) => setSharePassword(e.target.value)}
            placeholder="Password (optional)"
            className="mt-2 w-full border border-stone-300 rounded-md px-2 py-1 text-xs bg-white dark:bg-stone-800 dark:border-stone-700 dark:text-stone-100 dark:placeholder:text-stone-500"
          />
          <button
            type="button"
            onClick={onCreateShare}
            className="mt-2 w-full bg-brand-600 hover:bg-brand-700 text-white text-xs px-3 py-1.5 rounded-md"
          >
            Create share link
          </button>

          {newShare && (
            /* "Link created" panel uses the warn accent so it grabs the
               eye after a click. In dark mode we shift the panel to a
               dark amber tint so the body text (which the global rule
               flips to stone-100) stays legible against it. */
            <div className="mt-3 bg-warn-50 border border-warn-100 rounded p-2 space-y-2 dark:bg-amber-950/40 dark:border-amber-900/60">
              <div>
                <p className="text-xs text-warn-800 font-medium dark:text-amber-200">Link created — copy it now</p>
                <code className="block mt-1 text-xs font-mono break-all">{newShare.url}</code>
              </div>
              <div>
                <p className="text-[11px] uppercase tracking-wide font-semibold text-warn-800/80 dark:text-amber-200/80">
                  Embed
                </p>
                <code className="block mt-1 text-xs font-mono break-all">
                  {`<iframe src="${absoluteEmbedUrl(newShare.token)}" width="100%" height="600" frameborder="0" sandbox="allow-same-origin allow-scripts"></iframe>`}
                </code>
                <button
                  type="button"
                  className="mt-1 text-[11px] text-warn-800 hover:underline dark:text-amber-200"
                  onClick={() =>
                    navigator.clipboard.writeText(
                      `<iframe src="${absoluteEmbedUrl(newShare.token)}" width="100%" height="600" frameborder="0" sandbox="allow-same-origin allow-scripts"></iframe>`,
                    )
                  }
                >
                  Copy embed code
                </button>
              </div>
            </div>
          )}

          {shares && shares.length > 0 && (
            <ul className="mt-3 space-y-1.5 max-h-[200px] overflow-y-auto">
              {shares.map((s) => (
                <li key={s.id} className="flex items-center justify-between gap-2 text-xs">
                  <div>
                    <span className="font-mono">{s.prefix}…</span>
                    <span className="ml-2 text-stone-500">{s.format}</span>
                    <span className="ml-2 text-stone-400">{s.view_count} views</span>
                    {s.expires_at && (
                      <span className="ml-2 text-stone-400">
                        exp {new Date(s.expires_at * 1000).toLocaleDateString()}
                      </span>
                    )}
                  </div>
                  <button
                    type="button"
                    onClick={() => void onRevokeShare(s.id)}
                    className="text-danger-600 hover:text-danger-700"
                  >
                    Revoke
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
        )}
      </details>
    </div>
  );
}
