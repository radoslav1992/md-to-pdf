import { useState } from 'react';

/**
 * Download-only backup card. Triggers GET /api/v1/admin/backup, which
 * streams a `VACUUM INTO` snapshot. Restoring is operator-driven
 * (stop the containers, drop the file into the data volume, restart)
 * because doing it live across the running pool is too fragile to ship.
 */
export default function AdminBackup() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const download = async () => {
    setBusy(true);
    setError(null);
    try {
      const res = await fetch('/api/v1/admin/backup', { credentials: 'include' });
      if (!res.ok) {
        throw new Error(`HTTP ${res.status}: ${await res.text()}`);
      }
      // Extract the filename from Content-Disposition if present;
      // fall back to a timestamped default.
      const cd = res.headers.get('content-disposition') ?? '';
      const match = /filename="([^"]+)"/.exec(cd);
      const filename = match?.[1] ?? `udc-backup-${Date.now()}.db`;
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = filename;
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="card">
      <h2 className="text-lg font-semibold text-stone-900">SQLite snapshot</h2>
      <p className="mt-1 text-sm text-stone-600">
        Streams a consistent snapshot via <code>VACUUM INTO</code>. Safe to
        run while the API is live. To restore: stop the containers, drop
        the file into <code>/data/converter.db</code>, restart.
      </p>
      {error && (
        <div className="mt-3 text-sm text-danger-700 bg-danger-50 border border-danger-100 rounded px-3 py-2">
          {error}
        </div>
      )}
      <button
        type="button"
        className="btn-primary mt-4"
        onClick={download}
        disabled={busy}
      >
        {busy ? 'Snapshotting…' : 'Download backup'}
      </button>
    </div>
  );
}
