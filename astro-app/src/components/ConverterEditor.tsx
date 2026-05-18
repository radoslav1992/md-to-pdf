import { useCallback, useMemo, useState } from 'react';
import { api, type ConvertResult } from '../lib/api';
import { useUser } from '../lib/useUser';

type InputType = 'markdown' | 'html' | 'json' | 'xml';
type OutputType = 'html' | 'pdf';

interface Props {
  /** When true, hide premium-only input formats from the picker. Defaults to false. */
  anonymousMode?: boolean;
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

const PREMIUM_INPUTS: InputType[] = ['html', 'json', 'xml'];

export default function ConverterEditor({ anonymousMode = false }: Props) {
  const userState = useUser();
  const [inputType, setInputType] = useState<InputType>('markdown');
  const [outputType, setOutputType] = useState<OutputType>('html');
  const [content, setContent] = useState<string>(SAMPLES.markdown);
  const [title, setTitle] = useState('Untitled');
  const [result, setResult] = useState<ConvertResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [savedId, setSavedId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [info, setInfo] = useState<string | null>(null);

  const isAuthed = userState.status === 'authed';
  const isPremium = isAuthed && (userState.user.role === 'premium' || userState.user.role === 'admin');

  const onTypeChange = useCallback((next: InputType) => {
    setInputType(next);
    setContent(SAMPLES[next]);
    setResult(null);
    setSavedId(null);
    setError(null);
    setInfo(null);
  }, []);

  const convert = useCallback(async () => {
    setLoading(true);
    setError(null);
    setInfo(null);
    setResult(null);
    setSavedId(null);
    try {
      const res = await api.convert({ type: inputType, output: outputType, content, title });
      setResult(res);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [inputType, outputType, content, title]);

  const save = useCallback(async () => {
    if (!isAuthed) {
      window.location.href = `/login?next=${encodeURIComponent(window.location.pathname)}`;
      return;
    }
    setSaving(true);
    setError(null);
    setInfo(null);
    try {
      const res = await api.saveDocument({
        title: title.trim() || 'Untitled',
        type: inputType,
        output: outputType,
        content,
        rendered_html: result?.content ?? null,
      });
      setSavedId(res.document.id);
      setInfo(`Saved to your dashboard (#${res.document.id}).`);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }, [isAuthed, title, inputType, outputType, content, result]);

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

  const showPremiumLock = !isPremium && PREMIUM_INPUTS.includes(inputType);

  return (
    <div className="grid lg:grid-cols-2 gap-4">
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <input
            type="text"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="Document title"
            className="flex-1 min-w-[180px] border border-slate-300 rounded-md px-3 py-1.5 text-sm bg-white focus:outline-none focus:ring-2 focus:ring-brand-500"
          />
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <label className="text-sm font-medium text-slate-700">Input</label>
          <select
            value={inputType}
            onChange={(e) => onTypeChange(e.target.value as InputType)}
            className="border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
          >
            <option value="markdown">Markdown</option>
            {!anonymousMode && (
              <>
                <option value="html">HTML {isPremium ? '' : '· Premium'}</option>
                <option value="json">JSON {isPremium ? '' : '· Premium'}</option>
                <option value="xml">XML {isPremium ? '' : '· Premium'}</option>
              </>
            )}
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

          <div className="ml-auto flex items-center gap-2">
            {!anonymousMode && (
              <button
                type="button"
                onClick={save}
                disabled={saving || !result}
                title={isAuthed ? 'Save to your dashboard' : 'Log in to save'}
                className="border border-slate-300 hover:border-brand-500 disabled:opacity-50 text-slate-700 hover:text-brand-700 font-medium px-4 py-1.5 rounded-md text-sm transition"
              >
                {saving ? 'Saving…' : isAuthed ? 'Save' : 'Log in to save'}
              </button>
            )}
            <button
              type="button"
              onClick={convert}
              disabled={loading || showPremiumLock}
              className="bg-brand-600 hover:bg-brand-700 disabled:bg-slate-300 text-white font-medium px-4 py-1.5 rounded-md text-sm transition"
            >
              {loading ? 'Converting…' : 'Convert'}
            </button>
          </div>
        </div>

        {showPremiumLock && (
          <div className="text-xs text-amber-800 bg-amber-50 border border-amber-200 rounded px-3 py-2">
            HTML, JSON and XML inputs require a premium account. Markdown remains free for everyone.
          </div>
        )}

        <textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          spellCheck={false}
          className="w-full h-[460px] font-mono text-sm border border-slate-300 rounded-lg p-3 focus:outline-none focus:ring-2 focus:ring-brand-500"
        />
      </div>

      <div className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-semibold text-slate-700">Preview</h2>
          {result?.warnings && result.warnings.length > 0 && (
            <span className="text-xs text-amber-600">{result.warnings.length} warning(s)</span>
          )}
        </div>
        <div className="border border-slate-300 rounded-lg h-[460px] bg-white overflow-hidden">
          {error && (
            <div className="p-4 text-sm text-red-700 bg-red-50 border-b border-red-200">{error}</div>
          )}
          {info && !error && (
            <div className="p-4 text-sm text-emerald-700 bg-emerald-50 border-b border-emerald-200">{info}</div>
          )}
          {pdfDataUrl ? (
            <iframe title="PDF preview" src={pdfDataUrl} className="w-full h-full bg-white" />
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
        {result && (
          <div className="flex items-center justify-between text-xs text-slate-500">
            <span>{result.output_type === 'pdf' ? 'PDF ready' : 'HTML ready'}</span>
            {pdfDataUrl && (
              <a
                href={pdfDataUrl}
                download={`${(title || 'document').replace(/[^a-z0-9._-]+/gi, '_')}.pdf`}
                className="text-brand-600 hover:text-brand-700 underline"
              >
                Download PDF
              </a>
            )}
            {savedId !== null && (
              <a href={`/dashboard`} className="text-brand-600 hover:text-brand-700 underline">
                Open dashboard
              </a>
            )}
          </div>
        )}
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
