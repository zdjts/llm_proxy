import { render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { LocaleProvider } from '@/i18n/context';
import { ErrorState, Pagination, Skeleton, Table } from './index';

afterEach(() => localStorage.removeItem('dashboard-locale'));

describe('shared UI localization', () => {
  it('uses Chinese defaults from the locale provider', () => {
    localStorage.setItem('dashboard-locale', 'zh-CN');
    render(<LocaleProvider><Skeleton /><ErrorState /><Table headers={['项目']} rows={[]} /><Pagination page={2} pageCount={3} onPageChange={() => undefined} /></LocaleProvider>);

    expect(screen.getByLabelText('加载中')).toBeInTheDocument();
    expect(screen.getByText('数据加载失败')).toBeInTheDocument();
    expect(screen.getByText('暂无数据')).toBeInTheDocument();
    expect(screen.getByRole('navigation', { name: '分页' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '上一页' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '下一页' })).toBeInTheDocument();
  });
});
