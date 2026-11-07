import { useEffect, useMemo, useState } from 'react';
import {
  Destroyable,
  Model,
  ObjectInput,
  Query,
  QueryProps,
  Variable,
} from 'mobx-model-ui';

/**
 * Хуки для работы с mobx-model-ui: queries, inputs, формы.
 * Аналог эталона (infobiz/front/web/src/utils/mobx.ts).
 */

enum QueryType {
  QUERY = 'getQuery',
  QUERY_PAGE = 'getQueryPage',
  QUERY_CACHE_SYNC = 'getQueryCacheSync',
}

// Query создаётся один раз на жизнь компонента (как useRef): смена аргументов
// между рендерами намеренно не пересоздаёт его — отсюда пустые зависимости.
const useModelQuery = <M extends typeof Model>(
  model: M,
  queryType: QueryType,
  options?: QueryProps<InstanceType<M>>,
): [Query<InstanceType<M>>, Promise<boolean>] => {
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const query = useMemo(() => (model as any)[queryType](options) as Query<InstanceType<M>>, []);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const ready = useMemo<Promise<boolean>>(() => query.ready() as Promise<boolean>, []);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => () => query.destroy(), []);
  return [query, ready];
};

export const useQuery = <M extends typeof Model>(model: M, options?: QueryProps<InstanceType<M>>) =>
  useModelQuery(model, QueryType.QUERY, options);

export const useQueryPage = <M extends typeof Model>(model: M, options?: QueryProps<InstanceType<M>>) =>
  useModelQuery(model, QueryType.QUERY_PAGE, options);

export const useQueryCacheSync = <M extends typeof Model>(model: M, options?: QueryProps<InstanceType<M>>) =>
  useModelQuery(model, QueryType.QUERY_CACHE_SYNC, options);

export const useInput = <T>(createInput: () => Variable<T>, reset?: boolean) => {
  const input = useMemo(() => createInput(), []);
  useEffect(
    () => () => {
      if (reset) input.set(undefined);
      input.destroy();
    },
    [],
  );
  return input;
};

export const useObjectInput = <M extends Model>(
  createObjectInput: () => ObjectInput<M>,
  reset?: boolean,
) => {
  const input = useMemo(() => createObjectInput(), []);
  useEffect(
    () => () => {
      if (reset) input.set(undefined);
      input.destroy();
    },
    [],
  );
  return input;
};

export const useForm = <F extends Destroyable>(builder: () => F) => {
  const form = useMemo(builder, []);
  useEffect(() => () => form.destroy(), []);
  return form;
};

export const useObject = <T>(asyncFunc: () => Promise<T>): [T | undefined, boolean] => {
  const [object, setObject] = useState<T | undefined>(undefined);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    void (async () => {
      try {
        setObject(await asyncFunc());
      } finally {
        setLoading(false);
      }
    })();
  }, []);
  return [object, loading];
};