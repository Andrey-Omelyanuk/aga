import http from '@/services/http';

/** Изменения проекта воркстейшна от начала ветки (unified-дифф git ядра). */
export interface ChangesSummary {
  base: string | null;
  changed: boolean;
  diff: string;
}

export const loadChanges = async (workstationId: number) =>
  (await http.get<ChangesSummary>(`/workstations/${workstationId}/changes`)).data;
