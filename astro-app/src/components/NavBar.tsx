import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../lib/api';
import { useUser } from '../lib/useUser';

interface Props {
  /** Pathname of the current page; used to highlight the active link. */
  currentPath?: string;
}

const TOOL_LINKS = [
  { href: '/templates', label: 'Templates', hint: 'Reusable styling bundles' },
  { href: '/jobs', label: 'Jobs', hint: 'Background conversions' },
  { href: '/extract', label: 'PDF → Markdown', hint: 'Reverse-direction OCR' },
  { href: '/pdf-tools', label: 'PDF tools', hint: 'Merge, split, compress, watermark' },
  { href: '/api-keys', label: 'API keys', hint: 'Programmatic access' },
];

const MARKETING_LINKS = [
  { href: '/editor', label: 'Editor' },
  { href: '/pricing', label: 'Pricing' },
  { href: '/blog', label: 'Blog' },
];

function isActive(currentPath: string | undefined, href: string): boolean {
  if (!currentPath) return false;
  const p = currentPath.replace(/\/$/, '') || '/';
  if (href === '/') return p === '/';
  return p === href || p.startsWith(href + '/');
}

export default function NavBar({ currentPath }: Props) {
  const state = useUser();
  const [mobileOpen, setMobileOpen] = useState(false);
  const [theme, setTheme] = useState<'light' | 'dark'>(() => {
    if (typeof document === 'undefined') return 'light';
    return document.documentElement.classList.contains('dark') ? 'dark' : 'light';
  });

  const toggleTheme = useCallback(() => {
    setTheme((cur) => {
      const next = cur === 'light' ? 'dark' : 'light';
      try {
        localStorage.setItem('udc-theme', next);
      } catch {
        /* private mode etc — ignore */
      }
      document.documentElement.classList.toggle('dark', next === 'dark');
      return next;
    });
  }, []);

  const logout = useCallback(async () => {
    try {
      await api.logout();
    } finally {
      window.location.href = '/';
    }
  }, []);

  const isPremium =
    state.status === 'authed' &&
    (state.user.role === 'premium' || state.user.role === 'admin');
  const isAdmin = state.status === 'authed' && state.user.role === 'admin';

  return (
    <div className="flex items-center gap-2">
      <ThemeToggle theme={theme} onToggle={toggleTheme} />
      {state.status === 'loading' ? (
        <span className="text-stone-400 text-sm px-2">…</span>
      ) : state.status === 'anon' ? (
        <div className="flex items-center gap-1">
          <a
            href="/login"
            className="text-sm text-stone-700 hover:text-brand-700 px-3 py-1.5 rounded-lg hover:bg-stone-100 transition"
          >
            Log in
          </a>
          <a
            href="/signup"
            className="bg-brand-600 hover:bg-brand-700 text-white px-4 py-1.5 rounded-xl text-sm font-medium shadow-soft transition"
          >
            Sign up
          </a>
        </div>
      ) : (
        <>
          {/* Dashboard link — always visible on desktop. */}
          <a
            href="/dashboard"
            className={
              'hidden md:inline-flex text-sm px-3 py-1.5 rounded-lg transition ' +
              (isActive(currentPath, '/dashboard')
                ? 'text-brand-700 bg-brand-100/70 font-medium'
                : 'text-stone-700 hover:text-brand-700 hover:bg-stone-100')
            }
          >
            Dashboard
          </a>

          {/* Tools dropdown — premium-only utilities. */}
          {isPremium && (
            <ToolsMenu
              currentPath={currentPath}
              isActive={TOOL_LINKS.some((l) => isActive(currentPath, l.href))}
            />
          )}

          {isAdmin && (
            <a
              href="/admin"
              className={
                'hidden md:inline-flex text-sm px-3 py-1.5 rounded-lg transition ' +
                (isActive(currentPath, '/admin')
                  ? 'text-peach-700 bg-peach-100 font-medium'
                  : 'text-stone-700 hover:text-peach-700 hover:bg-peach-50')
              }
            >
              Admin
            </a>
          )}

          {/* Avatar / user menu. */}
          <UserMenu email={state.user.email} role={state.user.role} onLogout={logout} />

          {/* Mobile hamburger — also handles authed app links. */}
          <button
            type="button"
            onClick={() => setMobileOpen((v) => !v)}
            className="md:hidden inline-flex items-center justify-center w-9 h-9 rounded-lg hover:bg-stone-100 transition"
            aria-label="Toggle menu"
          >
            <Hamburger open={mobileOpen} />
          </button>
        </>
      )}

      {/* Mobile menu always available, anonymous too. */}
      {(state.status === 'anon' || state.status === 'authed') && (
        <button
          type="button"
          onClick={() => setMobileOpen((v) => !v)}
          className={
            (state.status === 'anon' ? '' : 'hidden ') +
            'md:hidden inline-flex items-center justify-center w-9 h-9 rounded-lg hover:bg-stone-100 transition'
          }
          aria-label="Toggle menu"
        >
          <Hamburger open={mobileOpen} />
        </button>
      )}

      {mobileOpen && (
        <MobileSheet
          currentPath={currentPath}
          authed={state.status === 'authed'}
          isPremium={isPremium}
          isAdmin={isAdmin}
          onClose={() => setMobileOpen(false)}
          onLogout={logout}
        />
      )}
    </div>
  );
}

function ToolsMenu({
  currentPath,
  isActive: hasActive,
}: {
  currentPath?: string;
  isActive: boolean;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useClickOutside(ref, () => setOpen(false));
  useEscape(() => setOpen(false));

  return (
    <div ref={ref} className="relative hidden md:block">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className={
          'inline-flex items-center gap-1 text-sm px-3 py-1.5 rounded-lg transition ' +
          (hasActive
            ? 'text-brand-700 bg-brand-100/70 font-medium'
            : 'text-stone-700 hover:text-brand-700 hover:bg-stone-100')
        }
      >
        Tools
        <Chevron open={open} />
      </button>
      {open && (
        <div className="absolute right-0 mt-2 w-64 card overflow-hidden">
          <ul className="py-1.5">
            {TOOL_LINKS.map((l) => (
              <li key={l.href}>
                <a
                  href={l.href}
                  className={
                    'block px-3 py-2 hover:bg-stone-50 transition ' +
                    (currentPath && currentPath.startsWith(l.href) ? 'bg-brand-50' : '')
                  }
                >
                  <div className="text-sm font-medium text-stone-900">{l.label}</div>
                  <div className="text-xs text-stone-500">{l.hint}</div>
                </a>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function UserMenu({
  email,
  role,
  onLogout,
}: {
  email: string;
  role: 'free' | 'premium' | 'admin';
  onLogout: () => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useClickOutside(ref, () => setOpen(false));
  useEscape(() => setOpen(false));
  const initial = email.charAt(0).toUpperCase();
  const tone =
    role === 'admin'
      ? 'from-peach-500 to-peach-700'
      : role === 'premium'
        ? 'from-peach-400 to-brand-600'
        : 'from-brand-500 to-brand-700';

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-2 px-1.5 py-1 rounded-xl hover:bg-stone-100 transition"
        aria-label="Account menu"
      >
        <span
          className={`inline-flex w-8 h-8 rounded-xl bg-gradient-to-br ${tone} text-white items-center justify-center text-sm font-bold shadow-soft`}
        >
          {initial}
        </span>
      </button>
      {open && (
        <div className="absolute right-0 mt-2 w-64 card overflow-hidden">
          <div className="px-4 py-3 border-b border-stone-200/70">
            <div className="text-xs text-stone-500">Signed in as</div>
            <div className="text-sm font-medium text-stone-900 truncate">{email}</div>
            <RoleBadge role={role} />
          </div>
          <ul className="py-1.5 text-sm">
            <li>
              <a href="/dashboard" className="block px-4 py-2 hover:bg-stone-50 text-stone-700">
                Dashboard
              </a>
            </li>
            {role === 'free' && (
              <li>
                <a
                  href="/pricing"
                  className="block px-4 py-2 hover:bg-peach-50 text-peach-700 font-medium"
                >
                  Upgrade to premium ✨
                </a>
              </li>
            )}
            <li>
              <button
                type="button"
                onClick={onLogout}
                className="block w-full text-left px-4 py-2 hover:bg-stone-50 text-stone-700"
              >
                Log out
              </button>
            </li>
          </ul>
        </div>
      )}
    </div>
  );
}

function MobileSheet({
  currentPath,
  authed,
  isPremium,
  isAdmin,
  onClose,
  onLogout,
}: {
  currentPath?: string;
  authed: boolean;
  isPremium: boolean;
  isAdmin: boolean;
  onClose: () => void;
  onLogout: () => void;
}) {
  return (
    <div className="fixed inset-0 z-50 md:hidden" role="dialog">
      <div className="absolute inset-0 bg-stone-900/30 backdrop-blur-sm" onClick={onClose}></div>
      <div className="absolute top-0 right-0 bottom-0 w-72 max-w-[85vw] bg-cream-50 shadow-glow flex flex-col overflow-y-auto">
        <div className="flex items-center justify-between px-4 py-3 border-b border-stone-200">
          <span className="font-semibold text-stone-900">Menu</span>
          <button
            type="button"
            onClick={onClose}
            className="w-8 h-8 inline-flex items-center justify-center rounded-lg hover:bg-stone-100"
            aria-label="Close menu"
          >
            ✕
          </button>
        </div>
        <nav className="flex-1 px-2 py-3 text-sm space-y-1">
          <Section title="Site">
            {MARKETING_LINKS.map((l) => (
              <MobileLink key={l.href} href={l.href} currentPath={currentPath} onClose={onClose}>
                {l.label}
              </MobileLink>
            ))}
          </Section>
          {authed && (
            <Section title="Workspace">
              <MobileLink href="/dashboard" currentPath={currentPath} onClose={onClose}>
                Dashboard
              </MobileLink>
              {isPremium &&
                TOOL_LINKS.map((l) => (
                  <MobileLink key={l.href} href={l.href} currentPath={currentPath} onClose={onClose}>
                    {l.label}
                  </MobileLink>
                ))}
              {isAdmin && (
                <MobileLink href="/admin" currentPath={currentPath} onClose={onClose}>
                  Admin
                </MobileLink>
              )}
            </Section>
          )}
        </nav>
        <div className="border-t border-stone-200 p-3">
          {authed ? (
            <button type="button" onClick={onLogout} className="btn-secondary w-full">
              Log out
            </button>
          ) : (
            <div className="flex gap-2">
              <a href="/login" className="btn-secondary flex-1">
                Log in
              </a>
              <a href="/signup" className="btn-primary flex-1">
                Sign up
              </a>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="mb-3">
      <div className="px-3 mt-2 mb-1 text-[11px] uppercase tracking-wider font-semibold text-stone-500">
        {title}
      </div>
      <div className="flex flex-col">{children}</div>
    </div>
  );
}

function MobileLink({
  href,
  children,
  currentPath,
  onClose,
}: {
  href: string;
  children: React.ReactNode;
  currentPath?: string;
  onClose: () => void;
}) {
  const active = isActive(currentPath, href);
  return (
    <a
      href={href}
      onClick={onClose}
      className={
        'px-3 py-2 rounded-lg transition ' +
        (active ? 'bg-brand-100/70 text-brand-800 font-medium' : 'text-stone-700 hover:bg-stone-100')
      }
    >
      {children}
    </a>
  );
}

function RoleBadge({ role }: { role: 'free' | 'premium' | 'admin' }) {
  const cls =
    role === 'admin'
      ? 'pill-peach mt-2'
      : role === 'premium'
        ? 'pill-peach mt-2'
        : 'pill-stone mt-2';
  return <span className={cls}>{role}</span>;
}

function ThemeToggle({
  theme,
  onToggle,
}: {
  theme: 'light' | 'dark';
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onToggle}
      title={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
      aria-label="Toggle color theme"
      className="inline-flex items-center justify-center w-9 h-9 rounded-lg hover:bg-stone-100 dark:hover:bg-stone-800 transition text-stone-600 dark:text-stone-300"
    >
      {theme === 'dark' ? (
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
          {/* Sun */}
          <circle cx="8" cy="8" r="3" stroke="currentColor" strokeWidth="1.6" />
          {[0, 45, 90, 135, 180, 225, 270, 315].map((d) => (
            <line
              key={d}
              x1="8"
              y1="1.5"
              x2="8"
              y2="3"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
              transform={`rotate(${d} 8 8)`}
            />
          ))}
        </svg>
      ) : (
        <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden>
          {/* Moon */}
          <path
            d="M6.5 1.5a6.5 6.5 0 1 0 8 8 5 5 0 0 1-8-8Z"
            fill="currentColor"
          />
        </svg>
      )}
    </button>
  );
}

function Chevron({ open }: { open: boolean }) {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 12 12"
      className={'transition-transform ' + (open ? 'rotate-180' : '')}
      aria-hidden
    >
      <path d="M2 4l4 4 4-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
    </svg>
  );
}

function Hamburger({ open }: { open: boolean }) {
  return (
    <svg width="20" height="20" viewBox="0 0 20 20" aria-hidden>
      {open ? (
        <>
          <path d="M4 4l12 12" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
          <path d="M16 4L4 16" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
        </>
      ) : (
        <>
          <path d="M3 6h14" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
          <path d="M3 10h14" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
          <path d="M3 14h14" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" />
        </>
      )}
    </svg>
  );
}

function useClickOutside(ref: React.RefObject<HTMLElement | null>, handler: () => void) {
  useEffect(() => {
    function onClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) handler();
    }
    document.addEventListener('mousedown', onClick);
    return () => document.removeEventListener('mousedown', onClick);
  }, [ref, handler]);
}

function useEscape(handler: () => void) {
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') handler();
    }
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [handler]);
}
