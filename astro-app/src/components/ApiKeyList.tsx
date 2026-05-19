import { useCallback, useEffect, useState } from 'react';
import { api, type ApiKey, type UsageSummary } from '../lib/api';

export default function ApiKeyList() {
  const [items, setItems] = useState<ApiKey[] | null>(null);
  const [usage, setUsage] = useState<UsageSummary | null>(null);
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [revealed, setRevealed] = useState<{ name: string; plaintext: string } | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [keys, u] = await Promise.all([api.listKeys(), api.usage()]);
      setItems(keys.items);
      setUsage(u.usage);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const onCreate = async () => {
    if (!name.trim()) {
      setError('Name is required.');
      return;
    }
    setCreating(true);
    setError(null);
    try {
      const res = await api.createKey(name.trim());
      setRevealed({ name: res.key.name, plaintext: res.plaintext });
      setName('');
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setCreating(false);
    }
  };

  const onRevoke = async (id: number) => {
    if (!confirm('Revoke this API key? Any callers using it will start failing.')) return;
    try {
      await api.revokeKey(id);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div className="space-y-6">
      {usage && (
        <div className="border border-stone-200 rounded-lg p-4 bg-stone-50">
          <div className="flex items-baseline justify-between">
            <h3 className="text-sm font-semibold text-stone-700">Conversions this period</h3>
            <span className="text-sm text-stone-600">
              {usage.used.toLocaleString()} / {usage.limit.toLocaleString()}
            </span>
          </div>
          <div className="mt-2 h-2 bg-white border border-stone-200 rounded overflow-hidden">
            <div
              className={`h-full ${usage.used / usage.limit > 0.8 ? 'bg-warn-500' : 'bg-success-500'}`}
              style={{ width: `${Math.min(100, (usage.used / usage.limit) * 100)}%` }}
            />
          </div>
          <p className="mt-2 text-xs text-stone-500">
            Rolling 30-day window. Conversions via API keys count the same as in-app conversions.
          </p>
        </div>
      )}

      {revealed && (
        <div className="border-2 border-warn-100 rounded-lg p-4 bg-warn-50">
          <div className="flex items-start justify-between">
            <div>
              <h3 className="font-semibold text-warn-800">Your new key — copy it now</h3>
              <p className="text-sm text-warn-800 mt-1">
                This is the only time we'll show "{revealed.name}" in full. Store it somewhere safe.
              </p>
            </div>
            <button
              type="button"
              onClick={() => setRevealed(null)}
              className="text-warn-700 hover:text-warn-800 text-sm"
            >
              Dismiss
            </button>
          </div>
          <code className="mt-3 block bg-white border border-warn-100 rounded p-3 text-sm font-mono break-all">
            {revealed.plaintext}
          </code>
        </div>
      )}

      <div className="border border-stone-200 rounded-lg p-4 bg-white">
        <h3 className="font-semibold text-stone-800">Create an API key</h3>
        <p className="text-xs text-stone-500 mt-1">
          Use these for headless integrations: CI pipelines, scripts, third-party tools. Pass as{' '}
          <code className="font-mono bg-stone-100 px-1 rounded">Authorization: Bearer &lt;key&gt;</code>.
        </p>
        <div className="mt-3 flex gap-2">
          <input
            type="text"
            placeholder="e.g. ci-pipeline, my-laptop"
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="flex-1 border border-stone-300 rounded-md px-3 py-1.5 text-sm bg-white"
          />
          <button
            type="button"
            onClick={onCreate}
            disabled={creating}
            className="bg-brand-600 hover:bg-brand-700 disabled:opacity-50 text-white px-4 py-1.5 rounded-md text-sm"
          >
            {creating ? 'Creating…' : 'Create'}
          </button>
        </div>
      </div>

      {error && (
        <div className="text-sm text-danger-700 bg-danger-50 border border-danger-100 rounded px-3 py-2">
          {error}
        </div>
      )}

      <div>
        <h3 className="font-semibold text-stone-800 mb-3">Active keys</h3>
        {items === null ? (
          <p className="text-sm text-stone-500">Loading…</p>
        ) : items.length === 0 ? (
          <p className="text-sm text-stone-500">No keys yet.</p>
        ) : (
          <ul className="space-y-2">
            {items.map((k) => (
              <li
                key={k.id}
                className="border border-stone-200 rounded-lg p-3 flex items-center justify-between"
              >
                <div>
                  <div className="font-medium text-stone-900">{k.name}</div>
                  <div className="text-xs text-stone-500 mt-0.5 font-mono">
                    {k.prefix}… · created {new Date(k.created_at * 1000).toLocaleDateString()}
                    {k.last_used_at
                      ? ` · last used ${new Date(k.last_used_at * 1000).toLocaleString()}`
                      : ' · never used'}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => onRevoke(k.id)}
                  className="text-xs text-danger-600 hover:text-danger-700"
                >
                  Revoke
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
