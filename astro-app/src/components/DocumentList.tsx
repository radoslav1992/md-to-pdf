import { useCallback, useEffect, useMemo, useState } from 'react';
import { api, type DocumentSummary, type SavedDocument } from '../lib/api';

type FolderFilter = '__all__' | '__root__' | string;

export default function DocumentList() {
  const [items, setItems] = useState<DocumentSummary[] | null>(null);
  const [folders, setFolders] = useState<string[]>([]);
  const [tags, setTags] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [openDoc, setOpenDoc] = useState<SavedDocument | null>(null);
  const [busy, setBusy] = useState(false);

  const [query, setQuery] = useState('');
  const [folderFilter, setFolderFilter] = useState<FolderFilter>('__all__');
  const [tagFilter, setTagFilter] = useState<string | null>(null);

  // Debounce the search query so each keystroke doesn't fire a request.
  const [debouncedQuery, setDebouncedQuery] = useState('');
  useEffect(() => {
    const h = window.setTimeout(() => setDebouncedQuery(query.trim()), 250);
    return () => window.clearTimeout(h);
  }, [query]);

  const load = useCallback(async () => {
    setError(null);
    try {
      const folderParam =
        folderFilter === '__all__'
          ? undefined
          : folderFilter === '__root__'
            ? ''
            : folderFilter;
      const res = await api.listDocuments({
        q: debouncedQuery || undefined,
        folder: folderParam,
        tag: tagFilter ?? undefined,
      });
      setItems(res.items);
      setFolders(res.folders);
      setTags(res.tags);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [debouncedQuery, folderFilter, tagFilter]);

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

  const activeFilters = useMemo(() => {
    const parts: string[] = [];
    if (debouncedQuery) parts.push(`“${debouncedQuery}”`);
    if (folderFilter === '__root__') parts.push('top level');
    else if (folderFilter !== '__all__') parts.push(folderFilter);
    if (tagFilter) parts.push(`#${tagFilter}`);
    return parts;
  }, [debouncedQuery, folderFilter, tagFilter]);

  if (items === null && !error) {
    return <p className="text-stone-500 text-sm">Loading…</p>;
  }

  return (
    <div className="grid lg:grid-cols-2 gap-6">
      <div className="space-y-3">
        <div className="card p-3 space-y-3">
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search title or content…"
            className="input"
          />
          <div className="flex flex-wrap items-center gap-2">
            <label className="text-xs font-medium text-stone-600">Folder</label>
            <select
              value={folderFilter}
              onChange={(e) => setFolderFilter(e.target.value as FolderFilter)}
              className="border border-stone-200 rounded-lg px-2 py-1 text-sm bg-white"
            >
              <option value="__all__">All folders</option>
              <option value="__root__">Top level only</option>
              {folders.map((f) => (
                <option key={f} value={f}>
                  {f}
                </option>
              ))}
            </select>
            {(activeFilters.length > 0 || folderFilter !== '__all__' || tagFilter) && (
              <button
                type="button"
                onClick={() => {
                  setQuery('');
                  setFolderFilter('__all__');
                  setTagFilter(null);
                }}
                className="text-xs text-stone-500 hover:text-brand-700 underline"
              >
                Clear filters
              </button>
            )}
          </div>
          {tags.length > 0 && (
            <div className="flex flex-wrap items-center gap-1">
              <span className="text-xs font-medium text-stone-600 mr-1">Tags</span>
              {tags.map((t) => (
                <button
                  key={t}
                  type="button"
                  onClick={() => setTagFilter((cur) => (cur === t ? null : t))}
                  className={
                    'text-xs px-2 py-0.5 rounded-full border transition ' +
                    (tagFilter === t
                      ? 'bg-brand-600 text-white border-brand-600'
                      : 'bg-white text-stone-700 border-stone-200 hover:border-brand-300')
                  }
                >
                  #{t}
                </button>
              ))}
            </div>
          )}
        </div>

        {error && (
          <div className="text-sm text-danger-700 bg-danger-50 border border-danger-100 rounded-xl px-3 py-2">
            {error}
          </div>
        )}
        {items && items.length === 0 ? (
          <div className="card p-8 text-center">
            <div className="text-4xl">📄</div>
            <p className="mt-3 text-sm text-stone-600">
              {activeFilters.length > 0
                ? `No documents match ${activeFilters.join(', ')}.`
                : "You haven't saved any documents yet."}
            </p>
            {activeFilters.length === 0 && (
              <a href="/editor" className="btn-primary mt-4 inline-flex">
                Open the editor
              </a>
            )}
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
                    {(doc.folder || doc.tags.length > 0) && (
                      <div className="mt-1 flex flex-wrap items-center gap-1 text-[11px]">
                        {doc.folder && (
                          <span className="text-stone-500">
                            📁 <span className="font-mono">{doc.folder}</span>
                          </span>
                        )}
                        {doc.tags.map((t) => (
                          <span
                            key={t}
                            className="text-brand-700 bg-brand-50 border border-brand-100 rounded-full px-1.5 py-0.5"
                          >
                            #{t}
                          </span>
                        ))}
                      </div>
                    )}
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
              {(openDoc.folder || openDoc.tags.length > 0) && (
                <div className="mt-1 flex flex-wrap items-center gap-1 text-[11px]">
                  {openDoc.folder && (
                    <span className="text-stone-500">
                      📁 <span className="font-mono">{openDoc.folder}</span>
                    </span>
                  )}
                  {openDoc.tags.map((t) => (
                    <span
                      key={t}
                      className="text-brand-700 bg-brand-50 border border-brand-100 rounded-full px-1.5 py-0.5"
                    >
                      #{t}
                    </span>
                  ))}
                </div>
              )}
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
