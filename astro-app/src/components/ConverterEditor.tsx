import { useCallback, useMemo, useState } from 'react';

type InputType = 'markdown' | 'html' | 'json' | 'xml';
type OutputType = 'html' | 'pdf';

interface ConvertResponse {
  ok: boolean;
  output_type: OutputType;
  content?: string;
  pdf_base64?: string;
  warnings?: string[];
  error?: string;
}

const SAMPLES: Record<InputType, string> = {
  markdown: `# Hello

This is **Markdown**. Try editing this content.

- Bullet one
- Bullet two

\`\`\`js
console.log("converted at the edge");
\`\`\`
`,
  html: `<h1>Hello</h1>
<p>This is <strong>HTML</strong>.</p>
<ul><li>One</li><li>Two</li></ul>`,
  json: `{
  "title": "Hello",
  "body": "This is JSON. The converter will pretty-print and render it."
}`,
  xml: `<?xml version="1.0"?>
<document>
  <title>Hello</title>
  <body>This is XML.</body>
</document>`,
};

export default function ConverterEditor() {
  const [inputType, setInputType] = useState<InputType>('markdown');
  const [outputType, setOutputType] = useState<OutputType>('html');
  const [content, setContent] = useState<string>(SAMPLES.markdown);
  const [result, setResult] = useState<ConvertResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const onTypeChange = useCallback((next: InputType) => {
    setInputType(next);
    setContent(SAMPLES[next]);
    setResult(null);
    setError(null);
  }, []);

  const convert = useCallback(async () => {
    setLoading(true);
    setError(null);
    setResult(null);
    try {
      const res = await fetch('/api/convert', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          type: inputType,
          output: outputType,
          content,
        }),
      });
      const text = await res.text();
      let parsed: ConvertResponse;
      try {
        parsed = JSON.parse(text) as ConvertResponse;
      } catch {
        parsed = {
          ok: res.ok,
          output_type: outputType,
          content: text,
          error: res.ok ? undefined : `HTTP ${res.status}`,
        };
      }
      if (!res.ok || parsed.ok === false) {
        setError(parsed.error ?? `HTTP ${res.status}`);
      }
      setResult(parsed);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [inputType, outputType, content]);

  const previewSrcDoc = useMemo(() => {
    if (!result?.content) return '';
    if (result.output_type === 'html') return result.content;
    return `<pre style="font-family: ui-monospace, monospace; padding: 16px; white-space: pre-wrap;">${escapeHtml(result.content)}</pre>`;
  }, [result]);

  const pdfDataUrl = useMemo(() => {
    if (result?.output_type === 'pdf' && result.pdf_base64) {
      return `data:application/pdf;base64,${result.pdf_base64}`;
    }
    return null;
  }, [result]);

  return (
    <div className="grid lg:grid-cols-2 gap-4">
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <label className="text-sm font-medium text-slate-700">Input</label>
          <select
            value={inputType}
            onChange={(e) => onTypeChange(e.target.value as InputType)}
            className="border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
          >
            <option value="markdown">Markdown</option>
            <option value="html">HTML</option>
            <option value="json">JSON</option>
            <option value="xml">XML</option>
          </select>
          <label className="text-sm font-medium text-slate-700 ml-4">Output</label>
          <select
            value={outputType}
            onChange={(e) => setOutputType(e.target.value as OutputType)}
            className="border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
          >
            <option value="html">HTML</option>
            <option value="pdf">PDF</option>
          </select>
          <button
            type="button"
            onClick={convert}
            disabled={loading}
            className="ml-auto bg-brand-600 hover:bg-brand-700 disabled:bg-slate-300 text-white font-medium px-4 py-1.5 rounded-md text-sm transition"
          >
            {loading ? 'Converting…' : 'Convert'}
          </button>
        </div>
        <textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          spellCheck={false}
          className="w-full h-[480px] font-mono text-sm border border-slate-300 rounded-lg p-3 focus:outline-none focus:ring-2 focus:ring-brand-500"
        />
      </div>

      <div className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-semibold text-slate-700">Preview</h2>
          {result?.warnings && result.warnings.length > 0 && (
            <span className="text-xs text-amber-600">{result.warnings.length} warning(s)</span>
          )}
        </div>
        <div className="border border-slate-300 rounded-lg h-[480px] bg-white overflow-hidden">
          {error && (
            <div className="p-4 text-sm text-red-700 bg-red-50 border-b border-red-200">
              {error}
            </div>
          )}
          {pdfDataUrl ? (
            <iframe
              title="PDF preview"
              src={pdfDataUrl}
              className="w-full h-full bg-white"
            />
          ) : result?.content ? (
            <iframe
              title="HTML preview"
              srcDoc={previewSrcDoc}
              className="w-full h-full bg-white"
              sandbox="allow-same-origin"
            />
          ) : (
            <div className="p-4 text-sm text-slate-500">
              The converted output will appear here. Press <strong>Convert</strong> to run the request.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}
