import { useCallback, useMemo, useState } from 'react';
import { api, readFileAsBase64 } from '../lib/api';
import { useUser } from '../lib/useUser';

type Tool = 'merge' | 'split' | 'compress' | 'watermark' | 'encrypt';

const TOOLS: { value: Tool; label: string; hint: string }[] = [
  { value: 'merge', label: 'Merge', hint: 'Concatenate multiple PDFs in order' },
  { value: 'split', label: 'Split', hint: 'Extract a page range (e.g. 1-3,7)' },
  { value: 'compress', label: 'Compress', hint: 'Shrink file size via ghostscript' },
  { value: 'watermark', label: 'Watermark', hint: 'Stamp every page with text' },
  { value: 'encrypt', label: 'Encrypt', hint: 'Add a password (AES-256)' },
];

const COMPRESS_LEVELS: { value: 'screen' | 'ebook' | 'printer' | 'prepress'; label: string }[] = [
  { value: 'screen', label: 'Screen (72 dpi · smallest)' },
  { value: 'ebook', label: 'Ebook (150 dpi · balanced)' },
  { value: 'printer', label: 'Printer (300 dpi)' },
  { value: 'prepress', label: 'Prepress (300 dpi · embeds fonts)' },
];

interface PickedFile {
  name: string;
  size: number;
  base64: string;
}

async function fileToPicked(f: File): Promise<PickedFile> {
  return {
    name: f.name,
    size: f.size,
    base64: await readFileAsBase64(f),
  };
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MiB`;
}

export default function PdfTools() {
  const user = useUser();
  const [tool, setTool] = useState<Tool>('merge');
  const [files, setFiles] = useState<PickedFile[]>([]);
  const [pages, setPages] = useState('1-z');
  const [compressLevel, setCompressLevel] =
    useState<'screen' | 'ebook' | 'printer' | 'prepress'>('ebook');
  const [watermarkText, setWatermarkText] = useState('CONFIDENTIAL');
  const [userPassword, setUserPassword] = useState('');
  const [ownerPassword, setOwnerPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<{
    base64: string;
    size: number;
    sourceTotal: number;
  } | null>(null);

  const isPremium =
    user.status === 'authed' && (user.user.role === 'premium' || user.user.role === 'admin');

  const onDrop = useCallback(async (e: React.DragEvent) => {
    e.preventDefault();
    const picked = Array.from(e.dataTransfer?.files ?? []).filter(
      (f) => f.type === 'application/pdf' || f.name.toLowerCase().endsWith('.pdf'),
    );
    if (picked.length === 0) return;
    const all = await Promise.all(picked.map(fileToPicked));
    setFiles((cur) => [...cur, ...all]);
  }, []);

  const onPick = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    const picked = Array.from(e.target.files ?? []);
    if (picked.length === 0) return;
    const all = await Promise.all(picked.map(fileToPicked));
    if (tool === 'merge') {
      setFiles((cur) => [...cur, ...all]);
    } else {
      // Other tools accept exactly one input.
      setFiles(all.slice(0, 1));
    }
    e.target.value = '';
  }, [tool]);

  const needsMultiple = tool === 'merge';
  const single = files[0];

  const run = useCallback(async () => {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      let res;
      const sourceTotal = files.reduce((sum, f) => sum + f.size, 0);
      switch (tool) {
        case 'merge': {
          if (files.length < 2) {
            throw new Error('Pick at least two PDFs to merge.');
          }
          res = await api.pdfMerge(files.map((f) => f.base64));
          break;
        }
        case 'split': {
          if (!single) throw new Error('Pick a PDF to split.');
          if (!pages.trim()) throw new Error('Enter a page range, e.g. 1-3,5');
          res = await api.pdfSplit(single.base64, pages.trim());
          break;
        }
        case 'compress': {
          if (!single) throw new Error('Pick a PDF to compress.');
          res = await api.pdfCompress(single.base64, compressLevel);
          break;
        }
        case 'watermark': {
          if (!single) throw new Error('Pick a PDF to watermark.');
          if (!watermarkText.trim()) throw new Error('Enter watermark text.');
          res = await api.pdfWatermark(single.base64, watermarkText.trim());
          break;
        }
        case 'encrypt': {
          if (!single) throw new Error('Pick a PDF to encrypt.');
          if (userPassword.length === 0) throw new Error('User password is required.');
          res = await api.pdfEncrypt(
            single.base64,
            userPassword,
            ownerPassword.trim() || undefined,
          );
          break;
        }
      }
      if (res) {
        setResult({
          base64: res.pdf_base64,
          size: res.size_bytes,
          sourceTotal,
        });
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [tool, files, pages, compressLevel, watermarkText, userPassword, ownerPassword]);

  const downloadUrl = useMemo(() => {
    if (!result) return null;
    return `data:application/pdf;base64,${result.base64}`;
  }, [result]);

  if (user.status === 'loading') return <p className="text-stone-500 text-sm">Loading…</p>;
  if (!isPremium) {
    return (
      <div className="card p-6 text-center">
        <p className="text-stone-700">
          PDF tools (merge, split, compress, watermark, encrypt) are a premium feature.
        </p>
        <a href="/pricing" className="btn-primary mt-4 inline-flex">
          Upgrade
        </a>
      </div>
    );
  }

  return (
    <div className="grid lg:grid-cols-3 gap-4">
      {/* Tool picker */}
      <aside className="card p-3 space-y-1">
        {TOOLS.map((t) => (
          <button
            key={t.value}
            type="button"
            onClick={() => {
              setTool(t.value);
              setResult(null);
              if (t.value !== 'merge') setFiles((cur) => cur.slice(0, 1));
            }}
            className={
              'w-full text-left px-3 py-2 rounded-lg transition ' +
              (tool === t.value
                ? 'bg-brand-100 text-brand-900'
                : 'hover:bg-stone-100 text-stone-700')
            }
          >
            <div className="text-sm font-medium">{t.label}</div>
            <div className="text-xs text-stone-500">{t.hint}</div>
          </button>
        ))}
      </aside>

      {/* Tool config + output */}
      <section className="lg:col-span-2 card p-4 space-y-4">
        <div
          onDragOver={(e) => e.preventDefault()}
          onDrop={onDrop}
          className="border-2 border-dashed border-stone-300 rounded-xl p-4 text-center text-sm text-stone-600 hover:border-brand-400 transition"
        >
          <p>
            Drag {needsMultiple ? 'PDFs' : 'a PDF'} here, or
          </p>
          <label className="mt-2 inline-flex items-center gap-2 cursor-pointer text-brand-700 hover:text-brand-800 underline">
            <input
              type="file"
              accept="application/pdf,.pdf"
              multiple={needsMultiple}
              onChange={onPick}
              className="hidden"
            />
            choose file{needsMultiple ? 's' : ''}
          </label>
        </div>

        {files.length > 0 && (
          <ul className="text-sm divide-y divide-stone-200 border border-stone-200 rounded-xl overflow-hidden">
            {files.map((f, i) => (
              <li key={i} className="flex items-center gap-2 px-3 py-1.5">
                <span className="flex-1 truncate" title={f.name}>{f.name}</span>
                <span className="text-xs text-stone-500">{formatBytes(f.size)}</span>
                <button
                  type="button"
                  onClick={() => setFiles((cur) => cur.filter((_, j) => j !== i))}
                  className="text-xs text-danger-600 hover:text-danger-700"
                >
                  remove
                </button>
              </li>
            ))}
          </ul>
        )}

        {/* Tool-specific config */}
        {tool === 'split' && (
          <div>
            <label className="text-xs font-medium text-stone-700 block mb-1">
              Page range (1-indexed; <code>z</code> = last)
            </label>
            <input
              type="text"
              value={pages}
              onChange={(e) => setPages(e.target.value)}
              placeholder="1-3,5,7-z"
              className="input"
            />
          </div>
        )}
        {tool === 'compress' && (
          <div>
            <label className="text-xs font-medium text-stone-700 block mb-1">
              Quality preset
            </label>
            <select
              value={compressLevel}
              onChange={(e) =>
                setCompressLevel(
                  e.target.value as 'screen' | 'ebook' | 'printer' | 'prepress',
                )
              }
              className="input"
            >
              {COMPRESS_LEVELS.map((l) => (
                <option key={l.value} value={l.value}>
                  {l.label}
                </option>
              ))}
            </select>
          </div>
        )}
        {tool === 'watermark' && (
          <div>
            <label className="text-xs font-medium text-stone-700 block mb-1">
              Watermark text
            </label>
            <input
              type="text"
              value={watermarkText}
              onChange={(e) => setWatermarkText(e.target.value)}
              placeholder="CONFIDENTIAL"
              maxLength={120}
              className="input"
            />
          </div>
        )}
        {tool === 'encrypt' && (
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div>
              <label className="text-xs font-medium text-stone-700 block mb-1">
                User password (required to open)
              </label>
              <input
                type="password"
                value={userPassword}
                onChange={(e) => setUserPassword(e.target.value)}
                className="input"
              />
            </div>
            <div>
              <label className="text-xs font-medium text-stone-700 block mb-1">
                Owner password (optional)
              </label>
              <input
                type="password"
                value={ownerPassword}
                onChange={(e) => setOwnerPassword(e.target.value)}
                placeholder="defaults to user password"
                className="input"
              />
            </div>
          </div>
        )}

        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={run}
            disabled={busy || files.length === 0}
            className="btn-primary"
          >
            {busy ? 'Working…' : `Run ${tool}`}
          </button>
          {result && downloadUrl && (
            <a
              href={downloadUrl}
              download={`${tool}.pdf`}
              className="text-sm text-brand-700 hover:text-brand-800 underline"
            >
              Download result ({formatBytes(result.size)}
              {tool === 'compress' && result.sourceTotal > 0 && (
                <>
                  {' '}— {Math.round((1 - result.size / result.sourceTotal) * 100)}% smaller
                </>
              )}
              )
            </a>
          )}
        </div>

        {error && (
          <div className="text-sm text-danger-700 bg-danger-50 border border-danger-100 rounded-xl px-3 py-2">
            {error}
          </div>
        )}
      </section>
    </div>
  );
}
