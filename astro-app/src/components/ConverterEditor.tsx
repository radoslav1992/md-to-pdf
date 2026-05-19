import { useCallback, useMemo, useState } from 'react';
import { api, type ConvertResult, type PdfOptions } from '../lib/api';
import { useUser } from '../lib/useUser';

type InputType =
  | 'markdown'
  | 'html'
  | 'json'
  | 'xml'
  | 'csv'
  | 'org'
  | 'asciidoc'
  | 'rst'
  | 'latex';
type OutputType = 'html' | 'pdf';

interface Props {
  /** When true, hide premium-only input formats from the picker. Defaults to false. */
  anonymousMode?: boolean;
  initialData?: {
    id: number;
    title: string;
    type: InputType;
    output: OutputType;
    content: string;
    theme?: string | null;
    custom_css?: string | null;
    pdf_options?: PdfOptions | null;
  };
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
  csv: `name,role,joined
Alice,Engineer,2021-03-04
Bob,Designer,2022-07-19
Carol,PM,2020-11-30`,
  org: `* Hello

This is /Org-mode/. Try editing this content.

- Bullet one
- Bullet two
`,
  asciidoc: `= Hello

This is AsciiDoc.

* Bullet one
* Bullet two

[source,js]
----
console.log("converted via pandoc");
----
`,
  rst: `Hello
=====

This is **reStructuredText**.

* Bullet one
* Bullet two
`,
  latex: `\\section{Hello}

This is \\LaTeX{}.

\\begin{itemize}
  \\item Bullet one
  \\item Bullet two
\\end{itemize}
`,
};

const PREMIUM_INPUTS: InputType[] = ['html', 'json', 'xml', 'csv', 'org', 'asciidoc', 'rst', 'latex'];

const THEMES: { value: string; label: string; premium: boolean }[] = [
  { value: 'default', label: 'Default', premium: false },
  { value: 'clean', label: 'Clean', premium: true },
  { value: 'academic', label: 'Academic', premium: true },
  { value: 'resume', label: 'Resume', premium: true },
  { value: 'letter', label: 'Letter', premium: true },
  { value: 'github', label: 'GitHub', premium: true },
];

const DEFAULT_PDF_OPTIONS: PdfOptions = {
  page_size: 'A4',
  orientation: 'portrait',
  margin: { top: '1in', right: '1in', bottom: '1in', left: '1in' },
  page_numbers: false,
};

export default function ConverterEditor({ anonymousMode = false, initialData }: Props) {
  const userState = useUser();
  const [inputType, setInputType] = useState<InputType>(initialData?.type ?? 'markdown');
  const [outputType, setOutputType] = useState<OutputType>(initialData?.output ?? 'html');
  const [content, setContent] = useState<string>(initialData?.content ?? SAMPLES.markdown);
  const [title, setTitle] = useState(initialData?.title ?? 'Untitled');
  const [theme, setTheme] = useState<string>(initialData?.theme ?? 'default');
  const [customCss, setCustomCss] = useState<string>(initialData?.custom_css ?? '');
  const [pdfOptions, setPdfOptions] = useState<PdfOptions>(
    initialData?.pdf_options ?? DEFAULT_PDF_OPTIONS,
  );
  const [stylePanelOpen, setStylePanelOpen] = useState(false);
  const [result, setResult] = useState<ConvertResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [savedId, setSavedId] = useState<number | null>(initialData?.id ?? null);
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

  const themeOrDefault = isPremium ? theme : 'default';
  const customCssToSend = isPremium && customCss.trim() ? customCss : undefined;
  const pdfOptionsToSend = isPremium && outputType === 'pdf' ? pdfOptions : undefined;

  const convert = useCallback(async () => {
    setLoading(true);
    setError(null);
    setInfo(null);
    setResult(null);
    setSavedId(null);
    try {
      const res = await api.convert({
        type: inputType,
        output: outputType,
        content,
        title,
        theme: themeOrDefault,
        custom_css: customCssToSend,
        pdf_options: pdfOptionsToSend,
      });
      setResult(res);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [inputType, outputType, content, title, themeOrDefault, customCssToSend, pdfOptionsToSend]);

  const save = useCallback(async () => {
    if (!isAuthed) {
      window.location.href = `/login?next=${encodeURIComponent(window.location.pathname)}`;
      return;
    }
    setSaving(true);
    setError(null);
    setInfo(null);
    try {
      const payload = {
        title: title.trim() || 'Untitled',
        type: inputType,
        output: outputType,
        content,
        rendered_html: result?.content ?? null,
        theme: isPremium ? theme : null,
        custom_css: isPremium && customCss.trim() ? customCss : null,
        pdf_options: isPremium && outputType === 'pdf' ? pdfOptions : null,
      };
      if (savedId) {
        const res = await api.updateDocument(savedId, payload);
        setInfo(`Updated document #${res.document.id}.`);
      } else {
        const res = await api.saveDocument(payload);
        setSavedId(res.document.id);
        setInfo(`Saved to your dashboard (#${res.document.id}).`);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }, [
    isAuthed,
    isPremium,
    savedId,
    title,
    inputType,
    outputType,
    content,
    result,
    theme,
    customCss,
    pdfOptions,
  ]);

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
                <option value="csv">CSV {isPremium ? '' : '· Premium'}</option>
                <option value="org">Org-mode {isPremium ? '' : '· Premium'}</option>
                <option value="asciidoc">AsciiDoc {isPremium ? '' : '· Premium'}</option>
                <option value="rst">reStructuredText {isPremium ? '' : '· Premium'}</option>
                <option value="latex">LaTeX {isPremium ? '' : '· Premium'}</option>
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
            HTML, JSON, XML, CSV, Org, AsciiDoc, RST and LaTeX inputs require a premium account.
            Markdown remains free for everyone.
          </div>
        )}

        <details
          open={stylePanelOpen}
          onToggle={(e) => setStylePanelOpen((e.target as HTMLDetailsElement).open)}
          className="border border-slate-200 rounded-lg bg-slate-50/50"
        >
          <summary className="cursor-pointer px-3 py-2 text-sm font-medium text-slate-700 select-none">
            Styling & page setup {isPremium ? '' : '· Premium'}
          </summary>
          <div className={`p-3 border-t border-slate-200 ${isPremium ? '' : 'opacity-60 pointer-events-none'}`}>
            <div className="flex flex-wrap items-center gap-3">
              <label className="text-xs font-medium text-slate-700">Theme</label>
              <select
                value={theme}
                onChange={(e) => setTheme(e.target.value)}
                className="border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
              >
                {THEMES.map((t) => (
                  <option key={t.value} value={t.value}>
                    {t.label}
                  </option>
                ))}
              </select>
            </div>

            {outputType === 'pdf' && (
              <div className="mt-3 grid grid-cols-2 gap-3">
                <div>
                  <label className="text-xs font-medium text-slate-700 block mb-1">Page size</label>
                  <select
                    value={pdfOptions.page_size ?? 'A4'}
                    onChange={(e) =>
                      setPdfOptions({ ...pdfOptions, page_size: e.target.value })
                    }
                    className="w-full border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
                  >
                    <option value="A4">A4</option>
                    <option value="A3">A3</option>
                    <option value="A5">A5</option>
                    <option value="Letter">Letter</option>
                    <option value="Legal">Legal</option>
                    <option value="Tabloid">Tabloid</option>
                  </select>
                </div>
                <div>
                  <label className="text-xs font-medium text-slate-700 block mb-1">Orientation</label>
                  <select
                    value={pdfOptions.orientation ?? 'portrait'}
                    onChange={(e) =>
                      setPdfOptions({
                        ...pdfOptions,
                        orientation: e.target.value as 'portrait' | 'landscape',
                      })
                    }
                    className="w-full border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
                  >
                    <option value="portrait">Portrait</option>
                    <option value="landscape">Landscape</option>
                  </select>
                </div>
                <div className="col-span-2">
                  <label className="text-xs font-medium text-slate-700 block mb-1">
                    Margins (top / right / bottom / left)
                  </label>
                  <div className="grid grid-cols-4 gap-2">
                    {(['top', 'right', 'bottom', 'left'] as const).map((side) => (
                      <input
                        key={side}
                        type="text"
                        placeholder="1in"
                        value={pdfOptions.margin?.[side] ?? ''}
                        onChange={(e) =>
                          setPdfOptions({
                            ...pdfOptions,
                            margin: { ...pdfOptions.margin, [side]: e.target.value || undefined },
                          })
                        }
                        className="border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
                      />
                    ))}
                  </div>
                </div>
                <label className="col-span-2 flex items-center gap-2 text-sm text-slate-700">
                  <input
                    type="checkbox"
                    checked={pdfOptions.page_numbers ?? false}
                    onChange={(e) => setPdfOptions({ ...pdfOptions, page_numbers: e.target.checked })}
                  />
                  Page numbers (Chromium footer)
                </label>
                <div className="col-span-2">
                  <label className="text-xs font-medium text-slate-700 block mb-1">
                    Header HTML (appears on every page)
                  </label>
                  <input
                    type="text"
                    placeholder='e.g. <span>Acme Corp — Confidential</span>'
                    value={pdfOptions.header_template ?? ''}
                    onChange={(e) =>
                      setPdfOptions({ ...pdfOptions, header_template: e.target.value })
                    }
                    className="w-full border border-slate-300 rounded-md px-2 py-1 text-sm bg-white font-mono"
                  />
                </div>
                <div className="col-span-2">
                  <label className="text-xs font-medium text-slate-700 block mb-1">
                    Footer HTML (appears on every page)
                  </label>
                  <input
                    type="text"
                    placeholder='e.g. <span>© 2026 Acme</span>'
                    value={pdfOptions.footer_template ?? ''}
                    onChange={(e) =>
                      setPdfOptions({ ...pdfOptions, footer_template: e.target.value })
                    }
                    className="w-full border border-slate-300 rounded-md px-2 py-1 text-sm bg-white font-mono"
                  />
                </div>
                <fieldset className="col-span-2 border border-slate-200 rounded-md p-2 mt-1">
                  <legend className="text-xs font-medium text-slate-700 px-1">Cover page</legend>
                  <div className="grid grid-cols-2 gap-2">
                    <input
                      type="text"
                      placeholder="Title"
                      value={pdfOptions.cover?.title ?? ''}
                      onChange={(e) =>
                        setPdfOptions({
                          ...pdfOptions,
                          cover: { ...pdfOptions.cover, title: e.target.value },
                        })
                      }
                      className="border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
                    />
                    <input
                      type="text"
                      placeholder="Subtitle"
                      value={pdfOptions.cover?.subtitle ?? ''}
                      onChange={(e) =>
                        setPdfOptions({
                          ...pdfOptions,
                          cover: { ...pdfOptions.cover, subtitle: e.target.value },
                        })
                      }
                      className="border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
                    />
                    <input
                      type="text"
                      placeholder="Author"
                      value={pdfOptions.cover?.author ?? ''}
                      onChange={(e) =>
                        setPdfOptions({
                          ...pdfOptions,
                          cover: { ...pdfOptions.cover, author: e.target.value },
                        })
                      }
                      className="border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
                    />
                    <input
                      type="text"
                      placeholder="Date"
                      value={pdfOptions.cover?.date ?? ''}
                      onChange={(e) =>
                        setPdfOptions({
                          ...pdfOptions,
                          cover: { ...pdfOptions.cover, date: e.target.value },
                        })
                      }
                      className="border border-slate-300 rounded-md px-2 py-1 text-sm bg-white"
                    />
                  </div>
                </fieldset>
              </div>
            )}

            <div className="mt-3">
              <label className="text-xs font-medium text-slate-700 block mb-1">Custom CSS</label>
              <textarea
                value={customCss}
                onChange={(e) => setCustomCss(e.target.value)}
                placeholder="body { font-family: 'My Font'; }"
                spellCheck={false}
                className="w-full h-24 font-mono text-xs border border-slate-300 rounded-md p-2 bg-white"
              />
            </div>
          </div>
        </details>

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
