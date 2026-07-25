import { Card, EmptyState } from '@/components/ui';

export function ModelCatalogPage() {
  return (
    <div className="space-y-6">
      <div><h1 className="text-2xl font-bold text-surface-800">Model Catalog</h1><p className="text-sm text-surface-400 mt-1">Browse registered models and their capabilities</p></div>
      <EmptyState title="Loading model catalog" description="Model registry data is being loaded from the database." />
    </div>
  );
}
