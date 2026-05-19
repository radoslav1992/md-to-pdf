import { useCallback, useEffect, useState } from 'react';
import { api, type PdfOptions, type Template } from '../lib/api';

const THEMES = ['default', 'clean', 'academic', 'resume', 'letter', 'github'];

const EMPTY_DRAFT = {
  name: '',
  theme: 'default',
  custom_css: '',
  page_size: 'A4',
  orientation: 'portrait' as 'portrait' | 'landscape',
  margin_top: '1in',
  margin_right: '1in',
  margin_bottom: '1in',
  margin_left: '1in',
  page_numbers: false,
};

type Draft = typeof EMPTY_DRAFT;

function draftToPdfOptions(d: Draft): PdfOptions {
  return {
    page_size: d.page_size,
    orientation: d.orientation,
    margin: {
      top: d.margin_top,
      right: d.margin_right,
      bottom: d.margin_bottom,
      left: d.margin_left,
    },
    page_numbers: d.page_numbers,
  };
}

function templateToDraft(t: Template): Draft {
  let pdf: PdfOptions = {};
  if (t.pdf_options) {
    try {
      pdf = JSON.parse(t.pdf_options);
    } catch {
      pdf = {};
    }
  }
  return {
    name: t.name,
    theme: t.theme ?? 'default',
    custom_css: t.custom_css ?? '',
    page_size: (pdf.page_size as string) ?? 'A4',
    orientation: (pdf.orientation as 'portrait' | 'landscape') ?? 'portrait',
    margin_top: pdf.margin?.top ?? '1in',
    margin_right: pdf.margin?.right ?? '1in',
    margin_bottom: pdf.margin?.bottom ?? '1in',
    margin_left: pdf.margin?.left ?? '1in',
    page_numbers: pdf.page_numbers ?? false,
  };
}

export default function TemplateList() {
  const [items, setItems] = useState<Template[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [editId, setEditId] = useState<number | 'new' | null>(null);
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setError(null);
    try {
      const res = await api.listTemplates();
      setItems(res.items);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const onNew = () => {
    setDraft(EMPTY_DRAFT);
    setEditId('new');
    setInfo(null);
    setError(null);
  };

  const onEdit = (t: Template) => {
    setDraft(templateToDraft(t));
    setEditId(t.id);
    setInfo(null);
    setError(null);
  };

  const onCancel = () => {
    setEditId(null);
    setDraft(EMPTY_DRAFT);
  };

  const onSave = async () => {
    if (!draft.name.trim()) {
      setError('Name is required.');
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const payload = {
        name: draft.name.trim(),
        theme: draft.theme,
        custom_css: draft.custom_css || null,
        pdf_options: draftToPdfOptions(draft),
      };
      if (editId === 'new') {
        await api.createTemplate(payload);
        setInfo('Template created.');
      } else if (typeof editId === 'number') {
        await api.updateTemplate(editId, payload);
        setInfo('Template updated.');
      }
      setEditId(null);
      setDraft(EMPTY_DRAFT);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  const onDelete = async (id: number) => {
    if (!confirm('Delete this template?')) return;
    try {
      await api.deleteTemplate(id);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div className="grid lg:grid-cols-2 gap-6">
      <div>
        <div className="flex items-center justify-between mb-3">
          <h2 className="font-semibold text-slate-800">Saved templates</h2>
          <button
            type="button"
            onClick={onNew}
            className="text-sm bg-brand-600 hover:bg-brand-700 text-white px-3 py-1.5 rounded-md"
          >
            New template
          </button>
        </div>
        {error && (
          <div className="text-sm text-red-700 bg-red-50 border border-red-200 rounded px-3 py-2 mb-3">
            {error}
          </div>
        )}
        {info && !error && (
          <div className="text-sm text-emerald-700 bg-emerald-50 border border-emerald-200 rounded px-3 py-2 mb-3">
            {info}
          </div>
        )}
        {items === null ? (
          <p className="text-sm text-slate-500">Loading…</p>
        ) : items.length === 0 ? (
          <p className="text-sm text-slate-500">
            No templates yet. Create one to apply consistent styling across documents.
          </p>
        ) : (
          <ul className="space-y-2">
            {items.map((t) => (
              <li
                key={t.id}
                className={`border rounded-lg p-3 ${editId === t.id ? 'border-brand-500 bg-brand-50' : 'border-slate-200'}`}
              >
                <div className="flex items-center justify-between">
                  <div>
                    <div className="font-medium text-slate-900">{t.name}</div>
                    <div className="text-xs text-slate-500 mt-0.5">
                      {t.theme ?? 'default'} ·{' '}
                      {new Date(t.updated_at * 1000).toLocaleString()}
                    </div>
                  </div>
                  <div className="flex items-center gap-3 text-xs">
                    <button onClick={() => onEdit(t)} className="text-brand-600 hover:text-brand-700">
                      Edit
                    </button>
                    <button onClick={() => onDelete(t.id)} className="text-red-600 hover:text-red-700">
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
        {editId === null ? (
          <p className="text-sm text-slate-500">Select a template to edit, or create a new one.</p>
        ) : (
          <div className="space-y-3">
            <h3 className="font-semibold text-slate-900">
              {editId === 'new' ? 'New template' : 'Edit template'}
            </h3>
            <div>
              <label className="text-xs font-medium text-slate-700 block mb-1">Name</label>
              <input
                type="text"
                value={draft.name}
                onChange={(e) => setDraft({ ...draft, name: e.target.value })}
                className="w-full border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
              />
            </div>
            <div>
              <label className="text-xs font-medium text-slate-700 block mb-1">Theme</label>
              <select
                value={draft.theme}
                onChange={(e) => setDraft({ ...draft, theme: e.target.value })}
                className="w-full border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
              >
                {THEMES.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="text-xs font-medium text-slate-700 block mb-1">Page size</label>
                <select
                  value={draft.page_size}
                  onChange={(e) => setDraft({ ...draft, page_size: e.target.value })}
                  className="w-full border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
                >
                  {['A4', 'A3', 'A5', 'Letter', 'Legal', 'Tabloid'].map((s) => (
                    <option key={s} value={s}>
                      {s}
                    </option>
                  ))}
                </select>
              </div>
              <div>
                <label className="text-xs font-medium text-slate-700 block mb-1">Orientation</label>
                <select
                  value={draft.orientation}
                  onChange={(e) =>
                    setDraft({ ...draft, orientation: e.target.value as 'portrait' | 'landscape' })
                  }
                  className="w-full border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
                >
                  <option value="portrait">Portrait</option>
                  <option value="landscape">Landscape</option>
                </select>
              </div>
            </div>
            <div>
              <label className="text-xs font-medium text-slate-700 block mb-1">
                Margins (top / right / bottom / left)
              </label>
              <div className="grid grid-cols-4 gap-2">
                {(['margin_top', 'margin_right', 'margin_bottom', 'margin_left'] as const).map(
                  (k) => (
                    <input
                      key={k}
                      type="text"
                      value={draft[k]}
                      onChange={(e) => setDraft({ ...draft, [k]: e.target.value })}
                      className="border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
                    />
                  ),
                )}
              </div>
            </div>
            <label className="flex items-center gap-2 text-sm text-slate-700">
              <input
                type="checkbox"
                checked={draft.page_numbers}
                onChange={(e) => setDraft({ ...draft, page_numbers: e.target.checked })}
              />
              Page numbers
            </label>
            <div>
              <label className="text-xs font-medium text-slate-700 block mb-1">Custom CSS</label>
              <textarea
                value={draft.custom_css}
                onChange={(e) => setDraft({ ...draft, custom_css: e.target.value })}
                className="w-full h-24 font-mono text-xs border border-slate-300 rounded-md p-2 bg-white"
              />
            </div>
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={onSave}
                disabled={saving}
                className="bg-brand-600 hover:bg-brand-700 text-white px-4 py-1.5 rounded-md text-sm disabled:opacity-50"
              >
                {saving ? 'Saving…' : 'Save'}
              </button>
              <button
                type="button"
                onClick={onCancel}
                className="border border-slate-300 text-slate-700 px-4 py-1.5 rounded-md text-sm hover:bg-slate-50"
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
