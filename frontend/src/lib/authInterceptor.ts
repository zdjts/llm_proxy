// Auto-injects Authorization header and handles 401 → refresh → retry.

import axios from 'axios';
import { useAuthStore } from '@/stores/authStore';

export const api = axios.create({ baseURL: '' });

// Request interceptor — inject JWT
api.interceptors.request.use((config) => {
  const token = useAuthStore.getState().getToken();
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

// Response interceptor — refresh on 401
let isRefreshing = false;
let failedQueue: Array<{
  resolve: (token: string) => void;
  reject: (err: unknown) => void;
}> = [];

function processQueue(token: string | null, error: unknown = null) {
  failedQueue.forEach((p) => {
    if (token) p.resolve(token);
    else p.reject(error);
  });
  failedQueue = [];
}

api.interceptors.response.use(
  (res) => res,
  async (error) => {
    const original = error.config;
    if (error.response?.status === 401 && !original._retry) {
      if (isRefreshing) {
        return new Promise((resolve, reject) => {
          failedQueue.push({ resolve: (t: string) => { original.headers.Authorization = `Bearer ${t}`; resolve(api(original)); }, reject });
        });
      }

      original._retry = true;
      isRefreshing = true;

      try {
        const newToken = await useAuthStore.getState().refresh();
        if (newToken) {
          processQueue(newToken);
          original.headers.Authorization = `Bearer ${newToken}`;
          return api(original);
        }
        processQueue(null, new Error('refresh failed'));
        useAuthStore.getState().logout();
        window.location.hash = '#/login';
        return Promise.reject(error);
      } catch (e) {
        processQueue(null, e);
        useAuthStore.getState().logout();
        window.location.hash = '#/login';
        return Promise.reject(e);
      } finally {
        isRefreshing = false;
      }
    }
    return Promise.reject(error);
  },
);
