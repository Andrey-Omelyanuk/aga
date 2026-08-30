import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { EmptyState } from '@/components/ui/tabs';
import { useStore } from '@/store-hooks';

export const Projects = observer(function Projects() {
  const store = useStore();
  const [url, setUrl] = useState('');

  const create = async () => {
    if (!url.trim()) return;
    await store.createProject(url.trim());
    setUrl('');
  };

  return (
    <div>
      <div className="mb-4 flex items-center gap-2">
        <Input
          placeholder="git-URL проекта"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && create()}
        />
        <Button variant="secondary" onClick={create}>
          Добавить проект
        </Button>
      </div>
      {store.projects.length === 0 && <EmptyState>Проектов пока нет</EmptyState>}
      {store.projects.map((p) => (
        <Card key={p.id}>
          <CardTitle>{p.git_url}</CardTitle>
          <CardMeta>Проект #{p.id}</CardMeta>
          {p.agents.length > 0 && (
            <div className="mt-1.5">
              {p.agents.map((a) => (
                <Badge key={a.name} variant="info">
                  {a.name}
                </Badge>
              ))}
            </div>
          )}
          <div className="mt-2">
            <Button variant="outline" size="sm" onClick={() => store.deleteProject(p.id)}>
              Удалить
            </Button>
          </div>
        </Card>
      ))}
    </div>
  );
});