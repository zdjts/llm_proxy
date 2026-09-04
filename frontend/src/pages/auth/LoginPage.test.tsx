import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LoginPage } from './LoginPage';

vi.mock('@/lib/api', () => ({
  apiError: (error: unknown) => error instanceof Error ? error.message : 'Login failed',
}));

const login = vi.fn();
let isAuthenticated = false;

vi.mock('@/stores/authStore', () => ({
  useAuthStore: () => ({ login, isAuthenticated }),
}));

function Location() {
  return <output data-testid="location">{useLocation().pathname}</output>;
}

describe('LoginPage', () => {
  beforeEach(() => {
    login.mockReset();
    login.mockResolvedValue(undefined);
    isAuthenticated = false;
  });

  function submitLogin() {
    fireEvent.change(screen.getByPlaceholderText('admin@example.com'), { target: { value: 'operator@example.test' } });
    fireEvent.change(screen.getByPlaceholderText('••••••••'), { target: { value: 'test-password' } });
    fireEvent.click(screen.getByRole('button', { name: 'Sign In' }));
  }

  it('enters the protected console after successful authentication', async () => {
    render(<MemoryRouter initialEntries={['/login']}><Routes><Route path="/login" element={<LoginPage />} /><Route path="/console" element={<Location />} /></Routes></MemoryRouter>);

    submitLogin();

    await waitFor(() => expect(login).toHaveBeenCalledWith('operator@example.test', 'test-password'));
    expect(await screen.findByTestId('location')).toHaveTextContent('/console');
  });

  it('shows the authentication failure instead of silently retrying', async () => {
    login.mockRejectedValueOnce(new Error('Invalid email or password'));
    render(<MemoryRouter initialEntries={['/login']}><Routes><Route path="/login" element={<LoginPage />} /></Routes></MemoryRouter>);

    submitLogin();

    expect(await screen.findByText('Invalid email or password')).toBeInTheDocument();
  });
});
