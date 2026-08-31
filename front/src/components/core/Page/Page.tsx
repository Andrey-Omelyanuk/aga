import { reaction } from 'mobx';
import { Query } from 'mobx-model-ui';
import { observer } from 'mobx-react-lite';
import { useEffect, useState } from 'react';

/**
 * Page: ждёт готовности переданных Query (единожды на страницу).
 * Дальше каждый компонент сам управляет своим состоянием загрузки.
 */
export interface PageProps {
  queries?: Query<any>[];
  children: React.ReactNode;
}

export const Page = observer((props: PageProps) => {
  const { queries = [], children } = props;
  const [isReady, setIsReady] = useState(!queries);

  useEffect(() => {
    if (!queries.length) {
      setIsReady(true);
      return;
    }
    return reaction(
      () => queries.every((query) => !query.isLoading && query.isReady),
      (ready) => ready && setIsReady(ready),
    );
  }, [queries]);

  return <div>{!isReady ? <div className="p-10 text-slate-400">Загрузка…</div> : children}</div>;
});