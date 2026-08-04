import { fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { useAuthStore } from '@/stores/authStore';
import { Sidebar } from './Sidebar';

function renderSidebar(mobileOpen = true, onMobileClose = () => undefined) {
  return render(<LocaleProvider><MemoryRouter><Sidebar mobileOpen={mobileOpen} onMobileClose={onMobileClose} /></MemoryRouter></LocaleProvider>);
}

describe('Sidebar', () => {
  beforeEach(() => {
    localStorage.setItem('dashboard-locale', 'en');
    useAuthStore.getState().setTokens('access', 'refresh', { id: 'user-1', email: 'operator@example.test', name: 'Operator', roles: ['operator'], permissions: [], teams: [], avatar_url: null });
  });

  it('keeps public routes visible while filtering restricted routes', () => {
    renderSidebar();
    expect(screen.getAllByRole('link', { name: 'Overview' }).length).toBeGreaterThan(0);
    expect(screen.queryByRole('link', { name: 'Providers' })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Users' })).not.toBeInTheDocument();
  });

  it('closes the mobile drawer after navigation is selected', () => {
    const onMobileClose = vi.fn();
    renderSidebar(true, onMobileClose);
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('link', { name: 'Cost' }));
    expect(onMobileClose).toHaveBeenCalledOnce();
  });

  it('focuses the close button and traps tab navigation in the open drawer', () => {
    renderSidebar();
    const drawer = screen.getByRole('dialog');
    const closeButton = within(drawer).getByRole('button', { name: 'Close navigation' });
    const languageButton = within(drawer).getByTitle('Switch Language');

    expect(closeButton).toHaveFocus();
    languageButton.focus();
    fireEvent.keyDown(drawer, { key: 'Tab' });
    expect(closeButton).toHaveFocus();
    fireEvent.keyDown(drawer, { key: 'Tab', shiftKey: true });
    expect(languageButton).toHaveFocus();
  });

  it('does not render mobile drawer controls while closed', () => {
    renderSidebar(false);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
});
