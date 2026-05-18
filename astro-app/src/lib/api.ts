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

  convert: (payload: { type: string; output: 'html' | 'pdf'; content: string; title?: string }) =>
    request<ConvertResult>('/api/convert', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),

  listDocuments: () =>
    request<{ ok: true; items: DocumentSummary[] }>('/api/documents'),
  getDocument: (id: number) =>
    request<{ ok: true; document: SavedDocument }>(`/api/documents/${id}`),
  saveDocument: (payload: {
    title: string;
    type: string;
    output: 'html' | 'pdf';
    content: string;
    rendered_html?: string | null;
  }) =>
    request<{ ok: true; document: SavedDocument }>('/api/documents', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  updateDocument: (
    id: number,
    payload: {
      title: string;
      type: string;
      output: 'html' | 'pdf';
      content: string;
      rendered_html?: string | null;
    },
  ) =>
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
};
