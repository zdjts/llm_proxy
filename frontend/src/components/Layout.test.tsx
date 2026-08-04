import { render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { beforeEach, describe, expect, it } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { useAuthStore } from '@/stores/authStore';
import { Layout } from './Layout';

describe('console shell', () => {
  beforeEach(() => {
    localStorage.setItem('dashboard-locale', 'en');
    useAuthStore.getState().setTokens('access', 'refresh', { id: 'user-1', email: 'operator@example.test', name: 'Operator', roles: ['operator'], permissions: [], teams: [], avatar_url: null });
  });

  it('renders the authenticated console shell and outlet without changing route semantics', () => {
    render(<LocaleProvider><MemoryRouter initialEntries={['/console']}><Routes><Route element={<Layout />}><Route path="/console" element={<h1>Overview content</h1>} /></Route></Routes></MemoryRouter></LocaleProvider>);
    expect(screen.getByRole('navigation', { name: 'Main navigation' })).toBeInTheDocument();
    expect(screen.getByText('Dashboard Overview')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Overview content' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Overview' })).toHaveAttribute('href', '/console');
  });
});
