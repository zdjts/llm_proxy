export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        surface: {
          50: '#fafaf7', 100: '#f7f6f2', 200: '#d4d4d0', 300: '#c4c4bf',
          400: '#767673', 500: '#61615e', 600: '#525252', 700: '#333333',
          800: '#202020', 900: '#0a0a0a',
        },
        primary: {
          50: '#fafaf7', 100: '#f2f2ef', 200: '#e4e4df', 300: '#d4d4d0',
          400: '#767673', 500: '#525252', 600: '#333333', 700: '#0a0a0a',
          800: '#0a0a0a', 900: '#0a0a0a',
        },
        accent: {
          50: '#f9f4e9', 100: '#f7eedb', 200: '#ebdbb7', 300: '#dec78c',
          400: '#c9a85b', 500: '#a98537', 600: '#80611f', 700: '#69501c',
          800: '#514016', 900: '#352b11',
        },
        success: { DEFAULT: '#3d7a4e', light: '#e4f1e7', dark: '#27633b' },
        danger: { DEFAULT: '#b04436', light: '#f8e5e2', dark: '#873027' },
        warning: { DEFAULT: '#a98537', light: '#f7eedb', dark: '#80611f' },
        info: { DEFAULT: '#4f7887', light: '#e5eff2', dark: '#3d6475' },
      },
      fontFamily: {
        sans: ['Noto Sans SC', 'PingFang SC', 'Microsoft YaHei', 'Inter', 'ui-sans-serif', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'SFMono-Regular', 'Consolas', 'monospace'],
        serif: ['Noto Serif SC', 'Songti SC', 'Georgia', 'serif'],
      },
      borderRadius: { DEFAULT: '0.375rem', lg: '0.5rem', xl: '0.75rem' },
    },
  },
  plugins: [],
};
