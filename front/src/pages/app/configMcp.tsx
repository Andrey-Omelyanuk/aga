import { observer } from 'mobx-react-lite';
import { Page } from '@/components/core/Page';
import { McpList } from '@/components/project/McpList';
import { McpServer } from '@/models/project';
import { useQuery } from '@/utils/mobx';

const ConfigMcpPage = observer(() => {
  const [servers] = useQuery(McpServer, { autoupdate: true });

  return (
    <Page queries={[servers]}>
      <div className="max-w-4xl">
        <h2 className="mb-1 text-lg font-semibold text-slate-800">MCP-серверы</h2>
        <p className="mb-4 text-sm text-slate-500">
          Каталог MCP-серверов: http — сервер по url, stdio — команда, которая
          запускается в воркстейшне агента (в его папке). Агент набора получает
          инструменты отмеченных у него серверов; работают только с LLM в
          режиме tool calling.
        </p>
        <McpList servers={servers.items} onChanged={() => servers.shadowLoad()} />
      </div>
    </Page>
  );
});

export default ConfigMcpPage;
