import { render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { beforeEach, describe, expect, it } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { Layout } from './Layout';

describe('console shell', () => {
  beforeEach(() => {
    localStorage.setItem('dashboard-locale', 'en');
  });

  it('renders the console shell and outlet without changing route semantics', () => {
    render(<LocaleProvider><MemoryRouter initialEntries={['/console']}><Routes><Route element={<Layout />}><Route path="/console" element={<h1>Overview content</h1>} /></Route></Routes></MemoryRouter></LocaleProvider>);
    expect(screen.getByRole('navigation', { name: 'Main navigation' })).toBeInTheDocument();
    expect(screen.getByText('Dashboard Overview')).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Overview content' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Overview' })).toHaveAttribute('href', '/console');
  });
});
