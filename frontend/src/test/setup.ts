import '@testing-library/jest-dom';

if (!globalThis.localStorage) {
  const values = new Map<string, string>();
  Object.defineProperty(globalThis, 'localStorage', { value: {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
    clear: () => values.clear(),
  }, configurable: true });
}