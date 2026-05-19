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

      <details open className="border border-stone-200 rounded-lg bg-white">
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

      <details className="border border-stone-200 rounded-lg bg-white">
        <summary className="cursor-pointer px-3 py-2 text-sm font-medium text-stone-700 select-none">
          Share links {shares ? `(${shares.length})` : ''} {isPremium ? '' : '· Premium'}
        </summary>
        <div className={`p-3 border-t border-stone-200 ${isPremium ? '' : 'opacity-60 pointer-events-none'}`}>
          <div className="grid grid-cols-2 gap-2">
            <select
              value={shareFormat}
              onChange={(e) => setShareFormat(e.target.value as 'html' | 'pdf')}
              className="border border-stone-300 rounded-md px-2 py-1 text-xs bg-white"
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
              className="border border-stone-300 rounded-md px-2 py-1 text-xs bg-white"
            />
          </div>
          <input
            type="text"
            value={sharePassword}
            onChange={(e) => setSharePassword(e.target.value)}
            placeholder="Password (optional)"
            className="mt-2 w-full border border-stone-300 rounded-md px-2 py-1 text-xs bg-white"
          />
          <button
            type="button"
            onClick={onCreateShare}
            className="mt-2 w-full bg-brand-600 hover:bg-brand-700 text-white text-xs px-3 py-1.5 rounded-md"
          >
            Create share link
          </button>

          {newShare && (
            <div className="mt-3 bg-warn-50 border border-warn-100 rounded p-2">
              <p className="text-xs text-warn-800 font-medium">Link created — copy it now</p>
              <code className="block mt-1 text-xs font-mono break-all">{newShare.url}</code>
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
      </details>
    </div>
  );
}
