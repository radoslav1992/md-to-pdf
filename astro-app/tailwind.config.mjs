/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/**/*.{astro,html,js,jsx,md,mdx,ts,tsx}'],
  // Class-based dark mode so the toggle in the navbar can flip it without
  // depending on the OS preference at first paint.
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Primary: Claude's warm clay / terracotta. Calm, editorial, human —
        // the signature of the claude.ai look. Most components reference these
        // `brand-*` tokens, so this palette drives the whole product's accent.
        brand: {
          50: '#fdf6f2',
          100: '#fae8df',
          200: '#f3ccb9',
          300: '#e9aa8c',
          400: '#df8c66',
          500: '#d97757', // Claude clay
          600: '#c2603f',
          700: '#a14a30',
          800: '#833d2b',
          900: '#6b3326',
        },
        // Accent: a softer amber, harmonised with the clay primary. Used for
        // premium badges and celebratory moments that need to lift off the page.
        peach: {
          50: '#fdf6ed',
          100: '#fbe9d3',
          200: '#f6d2a8',
          300: '#efb574',
          400: '#e89849',
          500: '#dd7f2b',
          600: '#c5681f',
          700: '#9e5119',
        },
        // Warm paper page background — the claude.ai ivory. Replaces pure white
        // as the default surface across marketing and app shells.
        cream: {
          50: '#faf9f5',
          100: '#f3f1ea',
          200: '#e9e6dc',
        },
        // Warm charcoal scale for dark mode. Claude's dark surfaces keep a hint
        // of brown rather than going cold blue/black, so the product still feels
        // like the same warm paper at night.
        ink: {
          950: '#1a1915',
          900: '#1f1e1d',
          800: '#262624', // elevated cards in dark mode
          700: '#30302e',
          600: '#3d3d3a',
        },
        // Semantic feedback. Pulled into one place so call sites use
        // `text-success-700` instead of one-off arbitrary hex.
        success: {
          50: '#ecfdf5',
          100: '#d1fae5',
          500: '#10b981',
          600: '#059669',
          700: '#047857',
          800: '#065f46',
        },
        warn: {
          50: '#fffbeb',
          100: '#fef3c7',
          500: '#f59e0b',
          600: '#d97706',
          700: '#b45309',
          800: '#92400e',
        },
        danger: {
          50: '#fef2f2',
          100: '#fee2e2',
          500: '#ef4444',
          600: '#dc2626',
          700: '#b91c1c',
        },
      },
      fontFamily: {
        // Body / UI — clean grotesque, the closest open alternative to
        // claude.ai's Styrene.
        sans: ['Inter', 'system-ui', '-apple-system', 'sans-serif'],
        // Display serif for hero headlines — a transitional serif in the spirit
        // of Claude's Copernicus/Tiempos.
        serif: ['Newsreader', 'Georgia', 'Cambria', 'Times New Roman', 'serif'],
        mono: ['JetBrains Mono', 'ui-monospace', 'SFMono-Regular', 'monospace'],
      },
      boxShadow: {
        // Soft, low-contrast layered shadows in a warm tone — avoids the harsh
        // single-pixel grey shadow Tailwind ships by default.
        soft: '0 1px 2px rgba(40, 31, 24, 0.04), 0 4px 14px rgba(40, 31, 24, 0.06)',
        glow: '0 10px 34px rgba(217, 119, 87, 0.20)',
        peach: '0 10px 34px rgba(221, 127, 43, 0.22)',
      },
      borderRadius: {
        '4xl': '2rem',
      },
      backgroundImage: {
        // Subtle warm clay wash for hero sections — barely-there, calm.
        'hero-warm':
          'radial-gradient(ellipse 80% 50% at 50% -10%, rgba(217, 119, 87, 0.14) 0%, rgba(233, 170, 140, 0.08) 35%, rgba(250, 249, 245, 0) 70%)',
        'hero-mint':
          'radial-gradient(ellipse at top right, rgba(223, 140, 102, 0.12) 0%, rgba(250, 249, 245, 0) 60%)',
      },
      transitionTimingFunction: {
        bounce: 'cubic-bezier(0.34, 1.56, 0.64, 1)',
      },
    },
  },
  plugins: [],
};
