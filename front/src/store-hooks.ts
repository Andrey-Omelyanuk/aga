import { createContext, useContext } from 'react';
import type { AppStore } from './models/store';

export const StoreContext = createContext<AppStore | null>(null);

export function useStore(): AppStore {
  const store = useContext(StoreContext);
  if (!store) throw new Error('StoreContext.Provider is missing');
  return store;
}