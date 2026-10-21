import { observer } from 'mobx-react-lite';
import { useTranslation } from 'react-i18next';
import { Page } from '@/components/core/Page';
import { McpList } from '@/components/project/McpList';
import { McpServer } from '@/models/project';
import { useQuery } from '@/utils/mobx';

const ConfigMcpPage = observer(() => {
  const { t } = useTranslation();
  const [servers] = useQuery(McpServer, { autoupdate: true });

  return (
    <Page queries={[servers]}>
      <div className="max-w-4xl">
        <h2 className="mb-1 text-lg font-semibold text-slate-800">{t('config.mcpServers')}</h2>
        <p className="mb-4 text-sm text-slate-500">
          {t('config.mcpDescription')}
        </p>
        <McpList servers={servers.items} onChanged={() => servers.shadowLoad()} />
      </div>
    </Page>
  );
});

export default ConfigMcpPage;
