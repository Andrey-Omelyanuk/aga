import { observer } from 'mobx-react-lite';
import { Page } from '@/components/core/Page';
import { LlmList } from '@/components/project/LlmList';
import { Llm } from '@/models/project';
import { useQuery } from '@/utils/mobx';

const ConfigLlmPage = observer(() => {
  const [connections] = useQuery(Llm, { autoupdate: true });

  return (
    <Page queries={[connections]}>
      <div className="max-w-3xl">
        <h2 className="mb-1 text-lg font-semibold text-slate-800">Подключения к LLM</h2>
        <p className="mb-4 text-sm text-slate-500">
          Название, url API, ключ доступа и модель. Агент набора выбирает
          подключение; одно из подключений — дефолтная LLM, к ней ходят агенты
          без своего подключения.
        </p>
        <LlmList connections={connections.items} onChanged={() => connections.shadowLoad()} />
      </div>
    </Page>
  );
});

export default ConfigLlmPage;