import { useCallback, useEffect, useState } from 'react';
import { api, type JobView } from '../lib/api';

const STATUS_COLORS: Record<JobView['status'], string> = {
  queued: 'bg-slate-100 text-slate-700',
  running: 'bg-blue-100 text-blue-800',
  done: 'bg-emerald-100 text-emerald-800',
  failed: 'bg-red-100 text-red-800',
  canceled: 'bg-amber-100 text-amber-800',
};

export default function JobList() {
  const [items, setItems] = useState<JobView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<JobView | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      const res = await api.listJobs();
      setItems(res.items);
      if (selected) {
        const fresh = res.items.find((j) => j.id === selected.id);
        if (fresh) setSelected(fresh);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [selected]);

  useEffect(() => {
    void load();
    const t = setInterval(() => {
      void load();
    }, 3000);
    return () => clearInterval(t);
  }, [load]);

  const cancel = useCallback(
    async (id: number) => {
      if (!confirm('Cancel this queued job?')) return;
      try {
        await api.cancelJob(id);
        await load();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      }
    },
    [load],
  );

  return (
    <div className="grid lg:grid-cols-2 gap-6">
      <div>
        {error && (
          <div className="text-sm text-red-700 bg-red-50 border border-red-200 rounded px-3 py-2 mb-3">
            {error}
          </div>
        )}
        {items === null ? (
          <p className="text-sm text-slate-500">Loading…</p>
        ) : items.length === 0 ? (
          <p className="text-sm text-slate-500">
            No jobs yet. Submit a long-running batch from the editor or via{' '}
            <code className="font-mono text-xs bg-slate-100 px-1 rounded">
              POST /api/jobs/batch
            </code>
            .
          </p>
        ) : (
          <ul className="space-y-2">
            {items.map((j) => (
              <li
                key={j.id}
                onClick={() => setSelected(j)}
                className={`cursor-pointer border rounded-lg p-3 transition ${
                  selected?.id === j.id
                    ? 'border-brand-500 bg-brand-50'
                    : 'border-slate-200 hover:border-slate-300'
                }`}
              >
                <div className="flex items-center justify-between">
                  <div>
                    <div className="font-medium text-slate-900">
                      #{j.id} · {j.kind}
                    </div>
                    <div className="text-xs text-slate-500 mt-0.5">
                      created {new Date(j.created_at * 1000).toLocaleString()}
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <span
                      className={`text-[10px] uppercase tracking-wide font-semibold px-2 py-0.5 rounded ${STATUS_COLORS[j.status]}`}
                    >
                      {j.status}
                    </span>
                    {j.status === 'queued' && (
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          void cancel(j.id);
                        }}
                        className="text-xs text-red-600 hover:text-red-700"
                      >
                        Cancel
                      </button>
                    )}
                  </div>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="border border-slate-200 rounded-lg p-4 bg-white">
        {!selected ? (
          <p className="text-sm text-slate-500">Select a job for details.</p>
        ) : (
          <div className="space-y-3 text-sm">
            <h3 className="font-semibold text-slate-900">
              Job #{selected.id} ({selected.kind})
            </h3>
            <dl className="grid grid-cols-[120px_1fr] gap-y-1 text-xs">
              <dt className="text-slate-500">Status</dt>
              <dd className="font-medium">{selected.status}</dd>
              <dt className="text-slate-500">Created</dt>
              <dd>{new Date(selected.created_at * 1000).toLocaleString()}</dd>
              {selected.started_at && (
                <>
                  <dt className="text-slate-500">Started</dt>
                  <dd>{new Date(selected.started_at * 1000).toLocaleString()}</dd>
                </>
              )}
              {selected.finished_at && (
                <>
                  <dt className="text-slate-500">Finished</dt>
                  <dd>{new Date(selected.finished_at * 1000).toLocaleString()}</dd>
                </>
              )}
            </dl>
            {selected.error_message && (
              <div className="text-xs text-red-700 bg-red-50 border border-red-200 rounded p-2">
                {selected.error_message}
              </div>
            )}
            {selected.result != null && (
              <>
                <h4 className="text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Result
                </h4>
                <pre className="text-xs bg-slate-50 border border-slate-200 rounded p-2 max-h-[260px] overflow-auto">
                  {JSON.stringify(selected.result, null, 2).slice(0, 4000)}
                </pre>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
