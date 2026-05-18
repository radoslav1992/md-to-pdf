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
    return <p className="text-slate-500 text-sm">Loading…</p>;
  }

  return (
    <div className="grid lg:grid-cols-2 gap-6">
      <div>
        {error && (
          <div className="text-sm text-red-700 bg-red-50 border border-red-200 rounded px-3 py-2 mb-3">
            {error}
          </div>
        )}
        {items && items.length === 0 ? (
          <p className="text-slate-500 text-sm">
            You haven't saved any documents yet. Head to the{' '}
            <a href="/editor" className="text-brand-600 hover:text-brand-700 underline">
              editor
            </a>
            .
          </p>
        ) : (
          <ul className="space-y-2">
            {items?.map((doc) => (
              <li
                key={doc.id}
                className={`border rounded-lg p-3 transition ${
                  openDoc?.id === doc.id
                    ? 'border-brand-500 bg-brand-50'
                    : 'border-slate-200 hover:border-slate-300'
                }`}
              >
                <div className="flex items-center justify-between gap-2">
                  <button
                    type="button"
                    onClick={() => onOpen(doc.id)}
                    className="text-left flex-1"
                  >
                    <div className="font-medium text-slate-900">{doc.title}</div>
                    <div className="text-xs text-slate-500 mt-0.5">
                      {doc.input_type} → {doc.output_type} ·{' '}
                      {new Date(doc.updated_at * 1000).toLocaleString()}
                    </div>
                  </button>
                  <div className="flex items-center gap-3">
                    <a
                      href={`/editor?id=${doc.id}`}
                      className="text-xs text-brand-600 hover:text-brand-700"
                    >
                      Edit
                    </a>
                    <button
                      type="button"
                      onClick={() => onDelete(doc.id)}
                      disabled={busy}
                      className="text-xs text-red-600 hover:text-red-700"
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

      <div className="border border-slate-200 rounded-lg p-4 bg-white">
        {openDoc ? (
          <article>
            <header className="mb-3">
              <h3 className="font-semibold text-slate-900">{openDoc.title}</h3>
              <p className="text-xs text-slate-500">
                {openDoc.input_type} · saved {new Date(openDoc.created_at * 1000).toLocaleString()}
              </p>
            </header>
            <h4 className="text-xs font-semibold uppercase tracking-wide text-slate-500">Source</h4>
            <pre className="mt-1 text-xs bg-slate-50 border border-slate-200 rounded p-2 max-h-[200px] overflow-auto">
              {openDoc.content}
            </pre>
            {openDoc.rendered_html && (
              <>
                <h4 className="mt-4 text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Rendered HTML preview
                </h4>
                <iframe
                  title="Saved document preview"
                  srcDoc={openDoc.rendered_html}
                  sandbox="allow-same-origin"
                  className="mt-1 w-full h-[260px] border border-slate-200 rounded"
                />
              </>
            )}
          </article>
        ) : (
          <p className="text-sm text-slate-500">Select a document to preview it.</p>
        )}
      </div>
    </div>
  );
}
