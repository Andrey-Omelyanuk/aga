import { createContext, useContext } from 'react';
import { ObjectInput } from 'mobx-model-ui';
import { Project } from '@/models/project';

export interface AppContextValue {
  activeProject: ObjectInput<Project>;
}

export const AppContext = createContext<AppContextValue | null>(null);

export function useApp(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error('AppContext.Provider is missing');
  return ctx;
}