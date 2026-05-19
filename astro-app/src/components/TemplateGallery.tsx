import { useCallback, useEffect, useState } from 'react';
import { api, type GalleryTemplate } from '../lib/api';
import { useUser } from '../lib/useUser';

export default function TemplateGallery() {
  const userState = useUser();
  const [items, setItems] = useState<GalleryTemplate[] | null>(null);
  const [cloning, setCloning] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      const res = await api.listGalleryTemplates();
      setItems(res.items);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const isPremium =
    userState.status === 'authed' &&
    (userState.user.role === 'premium' || userState.user.role === 'admin');

  const onClone = async (id: number) => {
    setCloning(id);
    setError(null);
    setInfo(null);
    try {
      const res = await api.cloneGalleryTemplate(id);
      setInfo(`Cloned as "${res.template.name}". Open /templates to use it.`);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setCloning(null);
    }
  };

  if (error && !items) {
    return (
      <div className="text-sm text-danger-700 bg-danger-50 border border-danger-100 rounded px-3 py-2">
        {error}
      </div>
    );
  }
  if (items === null) {
    return <p className="text-sm text-stone-500">Loading gallery…</p>;
  }
  if (items.length === 0) {
    return (
      <div className="border border-dashed border-stone-300 rounded-lg p-8 text-center text-sm text-stone-500">
        <p className="mb-1">The gallery is empty.</p>
        <p>
          Be the first to publish: create a template at{' '}
          <a className="text-brand-700 hover:underline" href="/templates">/templates</a>{' '}
          and hit <strong>Publish</strong>.
        </p>
      </div>
    );
  }

  return (
    <div>
      {error && (
        <div className="mb-3 text-sm text-danger-700 bg-danger-50 border border-danger-100 rounded px-3 py-2">
          {error}
        </div>
      )}
      {info && (
        <div className="mb-3 text-sm text-success-800 bg-success-50 border border-success-100 rounded px-3 py-2">
          {info}
        </div>
      )}
      <ul className="grid sm:grid-cols-2 lg:grid-cols-3 gap-4">
        {items.map((t) => {
          const pageSize = parsePageSize(t.pdf_options);
          return (
            <li
              key={t.id}
              className="border border-stone-200 rounded-lg p-4 bg-white flex flex-col"
            >
              <div className="flex-1">
                <h3 className="font-semibold text-stone-900 text-base">{t.name}</h3>
                <dl className="mt-2 text-xs text-stone-600 space-y-0.5">
                  <div>
                    <dt className="inline text-stone-500">Theme: </dt>
                    <dd className="inline font-mono">{t.theme ?? 'default'}</dd>
                  </div>
                  {pageSize && (
                    <div>
                      <dt className="inline text-stone-500">Page: </dt>
                      <dd className="inline font-mono">{pageSize}</dd>
                    </div>
                  )}
                  <div>
                    <dt className="inline text-stone-500">Updated: </dt>
                    <dd className="inline">
                      {new Date(t.updated_at * 1000).toLocaleDateString()}
                    </dd>
                  </div>
                </dl>
              </div>
              <div className="mt-4 flex items-center justify-between">
                {isPremium ? (
                  <button
                    type="button"
                    className="btn-primary text-xs"
                    onClick={() => onClone(t.id)}
                    disabled={cloning === t.id}
                  >
                    {cloning === t.id ? 'Cloning…' : 'Use this template'}
                  </button>
                ) : userState.status === 'authed' ? (
                  <span className="text-xs text-stone-500">
                    Premium required to clone
                  </span>
                ) : (
                  <a href="/login" className="text-xs text-brand-700 hover:underline">
                    Sign in to clone
                  </a>
                )}
                <span className="text-[10px] text-stone-400">#{t.id}</span>
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/**
 * `pdf_options` is stored as a JSON blob on the server; parsing failures
 * are fine — the gallery card just hides the page-size hint in that case.
 */
function parsePageSize(s: string | null): string | null {
  if (!s) return null;
  try {
    const j = JSON.parse(s);
    return typeof j.page_size === 'string' ? j.page_size : null;
  } catch {
    return null;
  }
}
