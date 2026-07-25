import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { api } from '@/lib/api';

export interface AuthUser {
  id: string;
  email: string;
  name: string;
  roles: string[];
  permissions: string[];
  teams: string[];
  avatar_url: string | null;
}

interface AuthState {
  user: AuthUser | null;
  accessToken: string | null;
  refreshToken: string | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  login: (email: string, password: string) => Promise<void>;
  logout: () => void;
  refresh: () => Promise<string | null>;
  getToken: () => string | null;
  can: (permission: string) => boolean;
  setTokens: (access: string, refresh: string, user: AuthUser) => void;
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set, get) => ({
      user: null,
      accessToken: null,
      refreshToken: null,
      isAuthenticated: false,
      isLoading: false,

      login: async (email: string, password: string) => {
        set({ isLoading: true });
        try {
          const { data } = await api.post('/api/auth/login', { email, password });
          set({
            user: data.user,
            accessToken: data.access_token,
            refreshToken: data.refresh_token,
            isAuthenticated: true,
            isLoading: false,
          });
        } catch (e) {
          set({ isLoading: false });
          throw e;
        }
      },

      logout: () => {
        set({
          user: null,
          accessToken: null,
          refreshToken: null,
          isAuthenticated: false,
        });
      },

      refresh: async () => {
        const rt = get().refreshToken;
        if (!rt) return null;
        try {
          const { data } = await api.post('/api/auth/refresh', { refresh_token: rt });
          set({ accessToken: data.access_token });
          return data.access_token;
        } catch {
          get().logout();
          return null;
        }
      },

      getToken: () => get().accessToken,

      can: (permission: string) => {
        // compat_mode: if no user (IP whitelist), allow everything
        const user = get().user;
        if (!user) return true;
        return user.permissions.includes(permission);
      },

      setTokens: (access: string, refresh: string, user: AuthUser) => {
        set({
          accessToken: access,
          refreshToken: refresh,
          user,
          isAuthenticated: true,
        });
      },
    }),
    {
      name: 'llm-proxy-auth',
      partialize: (state) => ({
        refreshToken: state.refreshToken,
        accessToken: state.accessToken,
        user: state.user,
        isAuthenticated: state.isAuthenticated,
      }),
    },
  ),
);
