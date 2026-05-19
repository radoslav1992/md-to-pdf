import { useCallback } from 'react';
import { api } from '../lib/api';
import { useUser } from '../lib/useUser';

export default function NavBar() {
  const state = useUser();

  const logout = useCallback(async () => {
    try {
      await api.logout();
    } finally {
      window.location.href = '/';
    }
  }, []);

  return (
    <div className="flex items-center gap-3 text-sm text-slate-700">
      {state.status === 'loading' ? (
        <span className="text-slate-400">…</span>
      ) : state.status === 'anon' ? (
        <>
          <a href="/login" className="hover:text-brand-600">Log in</a>
          <a
            href="/signup"
            className="bg-brand-600 hover:bg-brand-700 text-white px-3 py-1.5 rounded-md text-sm transition"
          >
            Sign up
          </a>
        </>
      ) : (
        <>
          <a href="/dashboard" className="hover:text-brand-600">Dashboard</a>
          {(state.user.role === 'premium' || state.user.role === 'admin') && (
            <>
              <a href="/templates" className="hover:text-brand-600">Templates</a>
              <a href="/api-keys" className="hover:text-brand-600">API keys</a>
            </>
          )}
          {state.user.role === 'admin' && (
            <a href="/admin" className="hover:text-brand-600">Admin</a>
          )}
          <RoleBadge role={state.user.role} />
          <span className="text-slate-500 hidden sm:inline">{state.user.email}</span>
          <button
            type="button"
            onClick={logout}
            className="text-slate-600 hover:text-brand-600 underline-offset-2 hover:underline"
          >
            Log out
          </button>
        </>
      )}
    </div>
  );
}

function RoleBadge({ role }: { role: 'free' | 'premium' | 'admin' }) {
  const classes =
    role === 'admin'
      ? 'bg-purple-100 text-purple-800'
      : role === 'premium'
      ? 'bg-amber-100 text-amber-800'
      : 'bg-slate-100 text-slate-700';
  return (
    <span className={`text-[10px] uppercase tracking-wide font-semibold px-2 py-0.5 rounded ${classes}`}>
      {role}
    </span>
  );
}
