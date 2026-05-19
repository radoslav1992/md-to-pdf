export type Role = 'free' | 'premium' | 'admin';

export interface PublicUser {
  id: number;
  email: string;
  role: Role;
  created_at: number;
}

export interface DocumentSummary {
  id: number;
  title: string;
  input_type: string;
  output_type: string;
  is_encrypted: boolean;
  created_at: number;
  updated_at: number;
}

export interface SavedDocument extends DocumentSummary {
  user_id: number;
  content: string;
  rendered_html: string | null;
  theme: string | null;
  custom_css: string | null;
  pdf_options: string | null;
  encryption_salt: string | null;
}

export type PageSize = 'A4' | 'A3' | 'A5' | 'Letter' | 'Legal' | 'Tabloid';
export type Orientation = 'portrait' | 'landscape';

export interface PdfMargin {
  top?: string;
  right?: string;
  bottom?: string;
  left?: string;
}

export interface PdfCover {
  title?: string;
  subtitle?: string;
  author?: string;
  date?: string;
}

export interface PdfOptions {
  page_size?: PageSize | string;
  orientation?: Orientation;
  margin?: PdfMargin;
  page_numbers?: boolean;
  header_template?: string;
  footer_template?: string;
  cover?: PdfCover;
}

export interface ConvertFile {
  path: string;
  content: string;
}

export interface EnrichmentOptions {
  toc?: boolean;
  toc_depth?: number;
  syntax_highlight?: boolean;
  math?: boolean;
  mermaid?: boolean;
}

export interface ImageUploadResult {
  id: number;
  url: string;
  filename: string;
  content_type: string;
  size_bytes: number;
  created_at: number;
}

export interface ImageMeta {
  id: number;
  user_id: number;
  filename: string;
  content_type: string;
  size_bytes: number;
  sha256: string;
  created_at: number;
}

export interface ConvertPayload {
  type: string;
  output: 'html' | 'pdf';
  content?: string;
  files?: ConvertFile[];
  title?: string;
  theme?: string;
  custom_css?: string;
  pdf_options?: PdfOptions;
  template_id?: number;
  enrichments?: EnrichmentOptions;
}

export interface Template {
  id: number;
  user_id: number;
  name: string;
  theme: string | null;
  custom_css: string | null;
  pdf_options: string | null;
  created_at: number;
  updated_at: number;
}

export interface SaveTemplatePayload {
  name: string;
  theme?: string | null;
  custom_css?: string | null;
  pdf_options?: PdfOptions | null;
}

export interface ApiKey {
  id: number;
  user_id: number;
  name: string;
  prefix: string;
  created_at: number;
  last_used_at: number | null;
}

export interface UsageSummary {
  used: number;
  limit: number;
  period_start: number;
}

export interface BatchItemResult {
  index: number;
  ok: boolean;
  result?: ConvertResult;
  error?: string;
}

export interface BatchResponse {
  ok: boolean;
  items: BatchItemResult[];
  webhook_delivered: boolean | null;
}

export interface BatchPayload {
  items: ConvertPayload[];
  webhook?: {
    url: string;
    secret?: string;
  };
}

export interface SaveDocumentPayload {
  title: string;
  type: string;
  output: 'html' | 'pdf';
  content: string;
  rendered_html?: string | null;
  theme?: string | null;
  custom_css?: string | null;
  pdf_options?: PdfOptions | null;
  encrypt_password?: string | null;
}

export interface DocumentVersionSummary {
  id: number;
  document_id: number;
  version: number;
  title: string;
  content_bytes: number;
  created_at: number;
}

export interface DocumentVersion extends DocumentVersionSummary {
  input_type: string;
  output_type: string;
  content: string;
  rendered_html: string | null;
  theme: string | null;
  custom_css: string | null;
  pdf_options: string | null;
}

export interface ShareLink {
  id: number;
  document_id: number;
  user_id: number;
  prefix: string;
  format: 'html' | 'pdf';
  expires_at: number | null;
  view_count: number;
  created_at: number;
}

export interface ShareMeta {
  ok: true;
  format: 'html' | 'pdf';
  requires_password: boolean;
  expires_at: number | null;
}

export interface ShareView {
  ok: true;
  title: string;
  format: 'html' | 'pdf';
  rendered_html: string | null;
}

export interface JobView {
  id: number;
  kind: 'convert' | 'batch';
  status: 'queued' | 'running' | 'done' | 'failed' | 'canceled';
  error_message: string | null;
  result: unknown;
  created_at: number;
  started_at: number | null;
  finished_at: number | null;
}

export interface ExtractRequest {
  pdf_base64: string;
  ocr?: 'auto' | 'force' | 'off';
}

export interface ExtractResponse {
  ok: boolean;
  method: 'pdftotext' | 'ocr';
  markdown: string;
  page_count: number | null;
}

export interface ConvertResult {
  ok: boolean;
  output_type: 'html' | 'pdf';
  input_type: string;
  content?: string;
  pdf_base64?: string;
  warnings?: string[];
  error?: string;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    credentials: 'same-origin',
    ...init,
    headers: {
      'content-type': 'application/json',
      ...(init?.headers ?? {}),
    },
  });
  const text = await res.text();
  let body: unknown;
  try {
    body = text ? JSON.parse(text) : {};
  } catch {
    body = { ok: false, error: text || `HTTP ${res.status}` };
  }
  if (!res.ok) {
    const message =
      typeof body === 'object' && body && 'error' in body && typeof (body as { error: unknown }).error === 'string'
        ? (body as { error: string }).error
        : `HTTP ${res.status}`;
    throw new Error(message);
  }
  return body as T;
}

export const api = {
  me: () => request<{ ok: true; user: PublicUser | null }>('/api/auth/me'),
  signup: (email: string, password: string) =>
    request<{ ok: true; user: PublicUser }>('/api/auth/signup', {
      method: 'POST',
      body: JSON.stringify({ email, password }),
    }),
  login: (email: string, password: string) =>
    request<{ ok: true; user: PublicUser }>('/api/auth/login', {
      method: 'POST',
      body: JSON.stringify({ email, password }),
    }),
  logout: () => request<{ ok: true }>('/api/auth/logout', { method: 'POST' }),

  convert: (payload: ConvertPayload) =>
    request<ConvertResult>('/api/convert', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  listDocuments: () =>
    request<{ ok: true; items: DocumentSummary[] }>('/api/documents'),
  getDocument: (id: number) =>
    request<{ ok: true; document: SavedDocument }>(`/api/documents/${id}`),
  saveDocument: (payload: SaveDocumentPayload) =>
    request<{ ok: true; document: SavedDocument }>('/api/documents', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  updateDocument: (id: number, payload: SaveDocumentPayload) =>
    request<{ ok: true; document: SavedDocument }>(`/api/documents/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  deleteDocument: (id: number) =>
    request<{ ok: true }>(`/api/documents/${id}`, { method: 'DELETE' }),

  listUsers: () =>
    request<{ ok: true; items: PublicUser[] }>('/api/admin/users'),
  updateUserRole: (id: number, role: Role) =>
    request<{ ok: true; user: PublicUser }>(`/api/admin/users/${id}/role`, {
      method: 'POST',
      body: JSON.stringify({ role }),
    }),

  listTemplates: () => request<{ ok: true; items: Template[] }>('/api/templates'),
  getTemplate: (id: number) =>
    request<{ ok: true; template: Template }>(`/api/templates/${id}`),
  createTemplate: (payload: SaveTemplatePayload) =>
    request<{ ok: true; template: Template }>('/api/templates', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  updateTemplate: (id: number, payload: SaveTemplatePayload) =>
    request<{ ok: true; template: Template }>(`/api/templates/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  deleteTemplate: (id: number) =>
    request<{ ok: true }>(`/api/templates/${id}`, { method: 'DELETE' }),

  listKeys: () => request<{ ok: true; items: ApiKey[] }>('/api/keys'),
  createKey: (name: string) =>
    request<{ ok: true; key: ApiKey; plaintext: string }>('/api/keys', {
      method: 'POST',
      body: JSON.stringify({ name }),
    }),
  revokeKey: (id: number) =>
    request<{ ok: true }>(`/api/keys/${id}`, { method: 'DELETE' }),

  usage: () => request<{ ok: true; usage: UsageSummary }>('/api/usage'),

  batchConvert: (payload: BatchPayload) =>
    request<BatchResponse>('/api/convert/batch', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  listVersions: (documentId: number) =>
    request<{ ok: true; items: DocumentVersionSummary[] }>(
      `/api/documents/${documentId}/versions`,
    ),
  getVersion: (documentId: number, versionId: number) =>
    request<{ ok: true; version: DocumentVersion }>(
      `/api/documents/${documentId}/versions/${versionId}`,
    ),
  restoreVersion: (documentId: number, versionId: number) =>
    request<{ ok: true; document: SavedDocument }>(
      `/api/documents/${documentId}/versions/${versionId}/restore`,
      { method: 'POST' },
    ),

  decryptDocument: (documentId: number, password: string) =>
    request<{ ok: true; content: string }>(
      `/api/documents/${documentId}/decrypt`,
      { method: 'POST', body: JSON.stringify({ password }) },
    ),

  listShares: (documentId: number) =>
    request<{ ok: true; items: ShareLink[] }>(`/api/documents/${documentId}/shares`),
  createShare: (
    documentId: number,
    payload: {
      format: 'html' | 'pdf';
      expires_in_seconds?: number | null;
      password?: string | null;
    },
  ) =>
    request<{ ok: true; share: ShareLink; url: string; token: string }>(
      `/api/documents/${documentId}/shares`,
      { method: 'POST', body: JSON.stringify(payload) },
    ),
  revokeShare: (id: number) =>
    request<{ ok: true }>(`/api/shares/${id}`, { method: 'DELETE' }),

  shareMeta: (token: string) => request<ShareMeta>(`/api/share/${token}`),
  shareView: (token: string, password?: string) =>
    request<ShareView>(`/api/share/${token}`, {
      method: 'POST',
      body: JSON.stringify({ password: password ?? null }),
    }),

  listJobs: () => request<{ ok: true; items: JobView[] }>('/api/jobs'),
  getJob: (id: number) => request<{ ok: true; job: JobView }>(`/api/jobs/${id}`),
  cancelJob: (id: number) =>
    request<{ ok: true }>(`/api/jobs/${id}/cancel`, { method: 'POST' }),
  enqueueConvertJob: (payload: ConvertPayload) =>
    request<{ ok: true; job_id: number; status: string }>('/api/jobs/convert', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  enqueueBatchJob: (payload: BatchPayload) =>
    request<{ ok: true; job_id: number; status: string }>('/api/jobs/batch', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  extract: (payload: ExtractRequest) =>
    request<ExtractResponse>('/api/extract', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  listImages: () =>
    request<{ ok: true; items: ImageMeta[] }>('/api/images'),
  uploadImage: (payload: { filename: string; content_type: string; data_base64: string }) =>
    request<{ ok: true; image: ImageUploadResult }>('/api/images', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  deleteImage: (id: number) =>
    request<{ ok: true }>(`/api/images/${id}`, { method: 'DELETE' }),
};

/**
 * Read a File/Blob into a base64-encoded string with no `data:` prefix.
 * Used by the editor's drag-and-drop handler before calling `uploadImage`.
 */
export function readFileAsBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      // FileReader yields `data:<type>;base64,<payload>` — strip the prefix.
      const comma = result.indexOf(',');
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.onerror = () => reject(reader.error ?? new Error('failed to read file'));
    reader.readAsDataURL(file);
  });
}
