import { useState, type FormEvent } from 'react';
import { api } from '../lib/api';

interface Props {
  mode: 'login' | 'signup';
}

export default function AuthForm({ mode }: Props) {
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      if (mode === 'signup') {
        await api.signup(email, password);
      } else {
        await api.login(email, password);
      }
      const next = new URLSearchParams(window.location.search).get('next') ?? '/dashboard';
      window.location.href = next;
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  const submitLabel = mode === 'signup' ? 'Create account' : 'Log in';
  const switchHref = mode === 'signup' ? '/login' : '/signup';
  const switchLabel =
    mode === 'signup' ? 'Already have an account? Log in' : "Don't have an account? Sign up";

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div>
        <label htmlFor="email" className="block text-sm font-medium text-stone-700 mb-1">
          Email
        </label>
        <input
          id="email"
          type="email"
          required
          autoComplete="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className="input"
        />
      </div>
      <div>
        <label htmlFor="password" className="block text-sm font-medium text-stone-700 mb-1">
          Password
        </label>
        <input
          id="password"
          type="password"
          required
          minLength={8}
          autoComplete={mode === 'signup' ? 'new-password' : 'current-password'}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className="input"
        />
        {mode === 'signup' && (
          <p className="mt-1.5 text-xs text-stone-500">At least 8 characters.</p>
        )}
      </div>
      {error && (
        <div className="text-sm text-danger-700 bg-danger-50 border border-danger-100 rounded-xl px-3 py-2">
          {error}
        </div>
      )}
      <button type="submit" disabled={loading} className="btn-primary w-full">
        {loading ? 'Working…' : submitLabel}
      </button>
      <p className="text-sm text-center text-stone-600">
        <a href={switchHref} className="link">{switchLabel}</a>
      </p>
    </form>
  );
}
