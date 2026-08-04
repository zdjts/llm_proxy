export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        surface: {
          50: '#fafaf7', 100: '#f4f1ec', 200: '#e5e0d9', 300: '#c9c1b7',
          400: '#8e877d', 500: '#716b63', 600: '#59544d', 700: '#403d38',
          800: '#292724', 900: '#191816',
        },
        primary: {
          50: '#fcf1ef', 100: '#f8e5e2', 200: '#f0c8c1', 300: '#df9a8f',
          400: '#c96557', 500: '#b04436', 600: '#a53b2f', 700: '#873027',
          800: '#6f2923', 900: '#4a1e19',
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
        sans: ['Inter', 'ui-sans-serif', 'system-ui', 'sans-serif'],
        mono: ['JetBrains Mono', 'SFMono-Regular', 'Consolas', 'monospace'],
      },
      borderRadius: { DEFAULT: '0.375rem', lg: '0.5rem', xl: '0.75rem' },
    },
  },
  plugins: [],
};
