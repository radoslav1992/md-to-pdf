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
};
