import { useCallback, useState } from 'react';
import { api } from '../lib/api';

type Mode = 'auto' | 'force' | 'off';

export default function ExtractTool() {
  const [mode, setMode] = useState<Mode>('auto');
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<{
    markdown: string;
    method: string;
    page_count: number | null;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filename, setFilename] = useState<string>('');

  const onFile = useCallback(
    async (file: File) => {
      setFilename(file.name);
      setLoading(true);
      setError(null);
      setResult(null);
      try {
        const buf = await file.arrayBuffer();
        const bytes = new Uint8Array(buf);
        // Encode to base64 in chunks to handle large PDFs without blowing
        // the call stack on String.fromCharCode.
        let binary = '';
        const chunk = 0x8000;
        for (let i = 0; i < bytes.length; i += chunk) {
          binary += String.fromCharCode.apply(null, Array.from(bytes.subarray(i, i + chunk)));
        }
        const b64 = btoa(binary);
        const res = await api.extract({ pdf_base64: b64, ocr: mode });
        setResult({ markdown: res.markdown, method: res.method, page_count: res.page_count });
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setLoading(false);
      }
    },
    [mode],
  );

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <label className="text-sm font-medium text-stone-700">OCR mode</label>
        <select
          value={mode}
          onChange={(e) => setMode(e.target.value as Mode)}
          className="border border-stone-300 rounded-md px-2 py-1 text-sm bg-white"
        >
          <option value="auto">Auto (OCR only if text PDF is sparse)</option>
          <option value="force">Force OCR (slow, accurate on scans)</option>
          <option value="off">Text only (no OCR)</option>
        </select>
        <label className="ml-auto text-sm bg-brand-600 hover:bg-brand-700 text-white px-4 py-1.5 rounded-md cursor-pointer">
          {loading ? 'Extracting…' : 'Choose PDF'}
          <input
            type="file"
            accept="application/pdf"
            className="hidden"
            onChange={(e) => {
              const f = e.target.files?.[0];
              if (f) void onFile(f);
            }}
            disabled={loading}
          />
        </label>
      </div>

      {filename && (
        <p className="text-xs text-stone-500">
          File: <span className="font-mono">{filename}</span>
        </p>
      )}

      {error && (
        <div className="text-sm text-danger-700 bg-danger-50 border border-danger-100 rounded px-3 py-2">
          {error}
        </div>
      )}

      {result && (
        <div>
          <div className="flex items-center justify-between mb-2">
            <p className="text-xs text-stone-600">
              Method: <span className="font-mono">{result.method}</span>
              {result.page_count !== null && (
                <span className="ml-3">
                  Pages: <span className="font-mono">{result.page_count}</span>
                </span>
              )}
              <span className="ml-3">
                Length: <span className="font-mono">{result.markdown.length}</span> chars
              </span>
            </p>
            <button
              type="button"
              onClick={() => void navigator.clipboard.writeText(result.markdown)}
              className="text-xs text-brand-600 hover:text-brand-700 underline"
            >
              Copy markdown
            </button>
          </div>
          <textarea
            readOnly
            value={result.markdown}
            className="w-full h-[400px] font-mono text-sm border border-stone-300 rounded-lg p-3 bg-stone-50"
          />
        </div>
      )}
    </div>
  );
}
