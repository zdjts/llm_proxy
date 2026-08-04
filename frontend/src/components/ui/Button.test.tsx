import { render, screen } from '@testing-library/react';
import { Button } from './index';

describe('Button', () => {
  it('renders children', () => {
    render(<Button>Click</Button>);
    expect(screen.getByText('Click')).toBeInTheDocument();
  });

  it('shows loading spinner when loading', () => {
    render(<Button loading>Save</Button>);
    expect(screen.getByText('Save')).toBeInTheDocument();
    // spinner element exists (the inline-block div)
    expect(document.querySelector('.animate-spin')).toBeTruthy();
  });

  it('applies variant classes', () => {
    const { container } = render(<Button variant="danger">Del</Button>);
    expect(container.firstChild).toHaveClass('bg-danger');
  });
});