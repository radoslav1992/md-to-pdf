/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/**/*.{astro,html,js,jsx,md,mdx,ts,tsx}'],
  theme: {
    extend: {
      colors: {
        // Primary: teal — productive, calm, modern.
        brand: {
          50: '#f0fdfa',
          100: '#ccfbf1',
          200: '#99f6e4',
          300: '#5eead4',
          400: '#2dd4bf',
          500: '#14b8a6',
          600: '#0d9488',
          700: '#0f766e',
          800: '#115e59',
          900: '#134e4a',
        },
        // Accent: peach/orange — for premium badges, celebratory moments,
        // calls to action that need to pop against the cool primary.
        peach: {
          50: '#fff7ed',
          100: '#ffedd5',
          200: '#fed7aa',
          300: '#fdba74',
          400: '#fb923c',
          500: '#f97316',
          600: '#ea580c',
          700: '#c2410c',
        },
        // Warm neutral page background — replaces pure white everywhere.
        cream: {
          50: '#fffbf3',
          100: '#fff5e6',
          200: '#fce8c8',
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
        sans: ['Inter', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'ui-monospace', 'monospace'],
      },
      boxShadow: {
        // Soft layered shadows. Avoids the harsh single-pixel grey shadow
        // Tailwind ships by default.
        soft: '0 1px 2px rgba(15, 23, 42, 0.04), 0 4px 12px rgba(15, 23, 42, 0.06)',
        glow: '0 8px 32px rgba(13, 148, 136, 0.18)',
        peach: '0 8px 32px rgba(249, 115, 22, 0.22)',
      },
      borderRadius: {
        '4xl': '2rem',
      },
      backgroundImage: {
        // Subtle warm gradient for hero sections.
        'hero-warm':
          'radial-gradient(ellipse at top, rgba(253, 230, 138, 0.45) 0%, rgba(204, 251, 241, 0.45) 30%, rgba(255, 251, 243, 0) 70%)',
        'hero-mint':
          'radial-gradient(ellipse at top right, rgba(94, 234, 212, 0.35) 0%, rgba(255, 251, 243, 0) 60%)',
      },
      transitionTimingFunction: {
        bounce: 'cubic-bezier(0.34, 1.56, 0.64, 1)',
      },
    },
  },
  plugins: [],
};
