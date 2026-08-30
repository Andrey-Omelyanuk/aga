import { beforeEach, describe, expect, it } from 'vitest';
import { runInAction } from 'mobx';
import { AppStore } from './store';
import { Workstation } from './workstation';
import { workstationRepo } from './registry';

function ws(id: number, state: string, project_id: number): Workstation {
  return new Workstation({ id, name: `ws-${id}`, state, project_id });
}

// sessionWorkstations идёт через QueryCacheSync (кэш mobx-model-ui): чистим
// кэш между тестами, чтобы фильтр не подхватывал модели из предыдущего.
beforeEach(() => {
  workstationRepo.modelDescriptor.cache.clear();
});

describe('sessionWorkstations', () => {
  it('offers only ready workstations that are free or bound to the current project', () => {
    const store = new AppStore();
    store.setActiveProject(2);
    runInAction(() => {
      store.workstations = [
        ws(1, 'ready', 0),
        ws(2, 'ready', 2),
        ws(3, 'ready', 3),
        ws(4, 'down', 0),
        ws(5, 'creating', 2),
      ];
    });
    expect(store.sessionWorkstations.map((w) => w.id)).toEqual([1, 2]);
  });

  it('without a project offers only free ready workstations', () => {
    const store = new AppStore();
    store.setActiveProject(null);
    runInAction(() => {
      store.workstations = [ws(10, 'ready', 0), ws(11, 'ready', 2)];
    });
    expect(store.sessionWorkstations.map((w) => w.id)).toEqual([10]);
  });
});