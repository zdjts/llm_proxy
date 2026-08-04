import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/admin': 'http://127.0.0.1:4000',
      '/api': 'http://127.0.0.1:4000',
      '/v1': 'http://127.0.0.1:4000',
      '/metrics': 'http://127.0.0.1:4000',
      '/health': 'http://127.0.0.1:4000',
    },
  },
  build: {
    outDir: 'dist',
    assetsDir: 'assets',
    sourcemap: false,
  },
});
