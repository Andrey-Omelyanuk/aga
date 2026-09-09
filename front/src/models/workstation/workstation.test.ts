import { beforeEach, describe, expect, it } from 'vitest';
import { AND, ARRAY, EQ, IN, NUMBER, STRING, Variable } from 'mobx-model-ui';
import { runInAction } from 'mobx';
import { Workstation } from './Workstation';

function ws(id: number, state: string, project_id: number): Workstation {
  return new Workstation({ id, name: `ws-${id}`, state, project_id });
}

// QueryCacheSync берёт items из кэша mobx-model-ui: чистим кэш между
// тестами, чтобы фильтр не подхватывал модели из предыдущего.
beforeEach(() => {
  Workstation.getModelDescriptor().cache.clear();
});

// Тот же фильтр, что строит страница сессий: только ready и свободные
// (project_id = 0). Станция, занятая сессией, вторую открыть не даст (409),
// поэтому в выборку она не входит.
describe('session workstations filter (QueryCacheSync)', () => {
  it('offers only ready and free workstations', () => {
    const projectIds = new Variable(ARRAY(NUMBER()), { value: [0] });
    const state = new Variable(STRING(), { value: 'ready' });
    const query = Workstation.getQueryCacheSync({
      filter: AND(EQ('state', state), IN('project_id', projectIds)),
    });

    runInAction(() => {
      ws(1, 'ready', 0);
      ws(2, 'ready', 2);
      ws(3, 'ready', 3);
      ws(4, 'down', 0);
      ws(5, 'creating', 0);
    });

    expect(query.items.map((w) => w.id)).toEqual([1]);
    query.destroy();
  });

  it('never offers workstations held by a session or not ready', () => {
    const projectIds = new Variable(ARRAY(NUMBER()), { value: [0] });
    const state = new Variable(STRING(), { value: 'ready' });
    const query = Workstation.getQueryCacheSync({
      filter: AND(EQ('state', state), IN('project_id', projectIds)),
    });

    runInAction(() => {
      ws(10, 'ready', 2);
      ws(11, 'down', 0);
      ws(12, 'creating', 3);
    });

    expect(query.items.map((w) => w.id)).toEqual([]);
    query.destroy();
  });
});
