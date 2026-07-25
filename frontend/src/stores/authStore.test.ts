import { describe, it, expect, beforeEach } from 'vitest';
import { useAuthStore } from './authStore';

describe('authStore', () => {
  beforeEach(() => {
    useAuthStore.getState().logout();
  });

  it('initializes unauthenticated', () => {
    const state = useAuthStore.getState();
    expect(state.isAuthenticated).toBe(false);
    expect(state.user).toBeNull();
  });

  it('sets tokens and user', () => {
    const user = { id: 'u1', email: 'a@b.com', name: 'A', roles: ['owner'], permissions: ['team.manage'], teams: [], avatar_url: null };
    useAuthStore.getState().setTokens('tok', 'ref', user);
    const state = useAuthStore.getState();
    expect(state.isAuthenticated).toBe(true);
    expect(state.accessToken).toBe('tok');
    expect(state.can('team.manage')).toBe(true);
    expect(state.can('providers.manage')).toBe(false);
  });

  it('logout clears state', () => {
    const user = { id: 'u1', email: 'a@b.com', name: 'A', roles: ['owner'], permissions: [], teams: [], avatar_url: null };
    useAuthStore.getState().setTokens('tok', 'ref', user);
    useAuthStore.getState().logout();
    const state = useAuthStore.getState();
    expect(state.isAuthenticated).toBe(false);
    expect(state.accessToken).toBeNull();
  });

  it('compat mode: can() returns true when no user', () => {
    // after logout, no user
    expect(useAuthStore.getState().can('anything')).toBe(true);
  });
});