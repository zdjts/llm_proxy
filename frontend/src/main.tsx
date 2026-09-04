import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import App from './App';
import './index.css';
import './styles/suixiang-public.css';
// v4.1: Import auth interceptor to auto-inject JWT on all API calls
import '@/lib/api';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
