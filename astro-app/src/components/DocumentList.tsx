import { useCallback, useEffect, useState } from 'react';
import { api, type DocumentSummary, type SavedDocument } from '../lib/api';

export default function DocumentList() {
  const [items, setItems] = useState<DocumentSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [openDoc, setOpenDoc] = useState<SavedDocument | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setError(null);
    try {
      const res = await api.listDocuments();
      setItems(res.items);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const onOpen = useCallback(async (id: number) => {
    setBusy(true);
    setError(null);
    try {
      const res = await api.getDocument(id);
      setOpenDoc(res.document);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, []);

  const onDelete = useCallback(
    async (id: number) => {
      if (!confirm('Delete this document?')) return;
      setBusy(true);
      setError(null);
      try {
        await api.deleteDocument(id);
        if (openDoc?.id === id) setOpenDoc(null);
        await load();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [openDoc, load],
  );

  if (items === null && !error) {
    return <p className="text-stone-500 text-sm">Loading…</p>;
  }

  return (
    <div className="grid lg:grid-cols-2 gap-6">
      <div>
        {error && (
          <div className="text-sm text-danger-700 bg-danger-50 border border-danger-100 rounded-xl px-3 py-2 mb-3">
            {error}
          </div>
        )}
        {items && items.length === 0 ? (
          <div className="card p-8 text-center">
            <div className="text-4xl">📄</div>
            <p className="mt-3 text-sm text-stone-600">
              You haven't saved any documents yet.
            </p>
            <a href="/editor" className="btn-primary mt-4 inline-flex">
              Open the editor
            </a>
          </div>
        ) : (
          <ul className="space-y-2">
            {items?.map((doc) => (
              <li
                key={doc.id}
                className={`card p-3 transition cursor-pointer ${
                  openDoc?.id === doc.id
                    ? 'border-brand-500 ring-2 ring-brand-200 bg-brand-50/40'
                    : 'hover:border-stone-300 hover:shadow-glow/40'
                }`}
              >
                <div className="flex items-center justify-between gap-2">
                  <button
                    type="button"
                    onClick={() => onOpen(doc.id)}
                    className="text-left flex-1 min-w-0"
                  >
                    <div className="font-medium text-stone-900 truncate flex items-center gap-1.5">
                      {doc.title}
                      {doc.is_encrypted && (
                        <span title="Encrypted at rest" className="text-peach-600 text-xs">🔒</span>
                      )}
                    </div>
                    <div className="text-xs text-stone-500 mt-0.5">
                      <span className="font-mono">{doc.input_type}</span> →{' '}
                      <span className="font-mono">{doc.output_type}</span> ·{' '}
                      {new Date(doc.updated_at * 1000).toLocaleString()}
                    </div>
                  </button>
                  <div className="flex items-center gap-3 shrink-0">
                    <a
                      href={`/editor?id=${doc.id}`}
                      className="text-xs text-brand-700 hover:text-brand-800 hover:underline"
                    >
                      Edit
                    </a>
                    <button
                      type="button"
                      onClick={() => onDelete(doc.id)}
                      disabled={busy}
                      className="text-xs text-danger-600 hover:text-danger-700"
                    >
                      Delete
                    </button>
                  </div>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="card p-4">
        {openDoc ? (
          <article>
            <header className="mb-3">
              <h3 className="font-semibold text-stone-900">{openDoc.title}</h3>
              <p className="text-xs text-stone-500">
                <span className="font-mono">{openDoc.input_type}</span> · saved{' '}
                {new Date(openDoc.created_at * 1000).toLocaleString()}
              </p>
            </header>
            <h4 className="text-xs font-semibold uppercase tracking-wider text-stone-500">Source</h4>
            <pre className="mt-1 text-xs bg-cream-50 border border-stone-200 rounded-xl p-3 max-h-[200px] overflow-auto">
              {openDoc.content}
            </pre>
            {openDoc.rendered_html && (
              <>
                <h4 className="mt-4 text-xs font-semibold uppercase tracking-wider text-stone-500">
                  Rendered preview
                </h4>
                <iframe
                  title="Saved document preview"
                  srcDoc={openDoc.rendered_html}
                  sandbox="allow-same-origin"
                  className="mt-1 w-full h-[260px] border border-stone-200 rounded-xl bg-white"
                />
              </>
            )}
          </article>
        ) : (
          <p className="text-sm text-stone-500">Select a document to preview it.</p>
        )}
      </div>
    </div>
  );
}
