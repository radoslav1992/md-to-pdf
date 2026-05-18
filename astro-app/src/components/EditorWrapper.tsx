import { useEffect, useState } from 'react';
import { api, type SavedDocument } from '../lib/api';
import ConverterEditor from './ConverterEditor';

export default function EditorWrapper() {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [initialData, setInitialData] = useState<{
    id: number;
    title: string;
    type: any;
    output: any;
    content: string;
  } | null>(null);

  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const idStr = params.get('id');
    if (!idStr) return;

    const id = parseInt(idStr, 10);
    if (isNaN(id)) return;

    setLoading(true);
    api.getDocument(id)
      .then(res => {
        setInitialData({
          id: res.document.id,
          title: res.document.title,
          type: res.document.input_type,
          output: res.document.output_type,
          content: res.document.content,
        });
      })
      .catch(err => {
        setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        setLoading(false);
      });
  }, []);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-20">
        <p className="text-slate-500">Loading document…</p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="max-w-2xl mx-auto py-20 text-center">
        <div className="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-lg mb-4">
          {error}
        </div>
        <a href="/editor" className="text-brand-600 hover:underline">Start with a new document</a>
      </div>
    );
  }

  return <ConverterEditor initialData={initialData ?? undefined} />;
}
