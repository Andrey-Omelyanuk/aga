import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardMeta } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';
import http from '@/services/http';
import { toaster } from '@/utils/toaster';
import type { Agent, AgentCapability, CatalogItem, Llm } from '@/models/project';
export interface AgentSetEditorProps {
  setId: number;
  name: string;
  agents: Agent[];
  skills: CatalogItem[];
  commands: CatalogItem[];
  /** Подключения к LLM: у агента набора выбирается одно из них. */
  connections: Llm[];
  onSaved: () => void;
}

const emptyAgent = (name: string): Agent => ({
  name,
  description: '',
  tools: [],
  max_iterations: 3,
  llm_id: null,
  parent_id: null,
  parent: null,
  skills: [],
  commands: [],
  territory: { folder: name, excludes: [] },
});

/** Редактор состава набора: агенты, их территория (по дереву), данные скиллы
 * и команды по имени (без версии), инструменты, выбранное подключение к LLM.
 * Сохраняется целиком (PATCH). */
export const AgentSetEditor = observer((props: AgentSetEditorProps) => {
  const { setId, skills, commands, connections, onSaved } = props;
  const [name, setName] = useState(props.name);
  const [agents, setAgents] = useState<Agent[]>(() =>
    props.agents.map((a) => ({
      ...a,
      tools: [...a.tools],
      skills: a.skills.map((s) => ({ ...s })),
      commands: a.commands.map((c) => ({ ...c })),
      territory: { ...a.territory },
      // Родитель хранится по имени (API принимает `parent`); из id выводим имя.
      parent: a.parent_id != null
        ? props.agents.find((p) => p.id === a.parent_id)?.name ?? null
        : (a.parent ?? null),
    })),
  );
  const [saving, setSaving] = useState(false);
  const [toolDraft, setToolDraft] = useState('');

  const patchAgent = (index: number, patch: Partial<Agent>) => {
    setAgents((prev) => prev.map((a, i) => (i === index ? { ...a, ...patch } : a)));
  };

  const addTool = (index: number) => {
    const tool = toolDraft.trim();
    if (!tool) return;
    patchAgent(index, {
      tools: agents[index].tools.includes(tool)
        ? agents[index].tools
        : [...agents[index].tools, tool],
    });
    setToolDraft('');
  };

  const removeTool = (index: number, tool: string) => {
    patchAgent(index, { tools: agents[index].tools.filter((t) => t !== tool) });
  };

  const addAgent = () => {
    setAgents((prev) => [...prev, emptyAgent(`folder-${prev.length + 1}`)]);
  };

  const removeAgent = (index: number) => {
    setAgents((prev) => prev.filter((_, i) => i !== index));
  };

  // Включение/выключение способности каталога на агенте; `kind` — skills/commands.
  const toggleCapability = (
    agentIndex: number,
    kind: 'skills' | 'commands',
    item: CatalogItem,
    enabled: boolean,
  ) => {
    setAgents((prev) =>
      prev.map((a, i) => {
        if (i !== agentIndex) return a;
        const existing = a[kind].filter((c) => c.name !== item.name);
        const next: AgentCapability[] = enabled
          ? [...existing, { name: item.name }]
          : existing;
        return { ...a, [kind]: next };
      }),
    );
  };

  const save = async () => {
    setSaving(true);
    try {
      const payloadAgents = agents.map((a) => ({
        name: a.name,
        description: a.description,
        tools: a.tools,
        max_iterations: a.max_iterations,
        llm_id: a.llm_id ?? null,
        parent: a.parent ?? null,
        skills: a.skills,
        commands: a.commands,
      }));
      await http.patch(`/agent-sets/${setId}`, { name, agents: payloadAgents });
      toaster.show({ message: 'Набор сохранён', intent: 'success' });
      onSaved();
    } catch {
      toaster.show({ message: 'Не удалось сохранить набор', intent: 'danger' });
    } finally {
      setSaving(false);
    }
  };

  const capabilityBlock = (
    kind: 'skills' | 'commands',
    items: CatalogItem[],
    label: string,
  ) => {
    if (items.length === 0) {
      return <div className="text-xs text-slate-400">Каталог «{label}» пуст</div>;
    }
    return (
      <div className="space-y-1">
        <div className="text-xs font-medium text-slate-500">{label}</div>
        {agents.map((agent, ai) => (
          <div key={`${label}-${ai}`} className="space-y-1">
            {items.map((item) => {
              const given = agent[kind].find((c) => c.name === item.name);
              return (
                <label key={item.name} className="flex items-center gap-2 text-xs">
                  <input
                    type="checkbox"
                    checked={Boolean(given)}
                    onChange={(e) => toggleCapability(ai, kind, item, e.target.checked)}
                  />
                  <span className="w-32 truncate">{item.name}</span>
                </label>
              );
            })}
          </div>
        ))}
      </div>
    );
  };

  return (
    <div className="space-y-2.5">
      <div className="flex items-center gap-2">
        <Input value={name} onChange={(e) => setName(e.target.value)} className="max-w-xs" />
        <Button onClick={save} disabled={saving}>
          Сохранить набор
        </Button>
      </div>

      {agents.map((agent, i) => {
        const parentOptions = agents
          .map((a) => a.name)
          .filter((n) => n !== agent.name);
        return (
          <Card key={i}>
            <div className="flex items-center gap-2">
              <Input
                className="max-w-52"
                value={agent.name}
                onChange={(e) => patchAgent(i, { name: e.target.value })}
                placeholder="папка агента (путь в проекте)"
              />
              <Select
                className="max-w-44"
                value={agent.parent ?? ''}
                onChange={(e) => patchAgent(i, { parent: e.target.value || null })}
              >
                <option value="">— нет родителя —</option>
                {parentOptions.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </Select>
              <Select
                className="max-w-52"
                value={agent.llm_id != null ? String(agent.llm_id) : ''}
                onChange={(e) =>
                  patchAgent(i, { llm_id: e.target.value ? Number(e.target.value) : null })
                }
                title="Подключение к LLM"
              >
                <option value="">— дефолтная LLM —</option>
                {connections.map((c) => (
                  <option key={c.id} value={String(c.id)}>
                    {c.name}
                  </option>
                ))}
              </Select>
              <Button variant="ghost" size="sm" onClick={() => removeAgent(i)}>
                Удалить агента
              </Button>
            </div>
            <CardMeta>Территория: {agent.territory.folder || '(корень)'}</CardMeta>
            {agent.territory.excludes.length > 0 && (
              <CardMeta>
                кроме:{' '}
                {agent.territory.excludes.map((e) => (
                  <Badge key={e} variant="warn">
                    {e}
                  </Badge>
                ))}
              </CardMeta>
            )}
            <div className="mt-2">
              <textarea
                className="min-h-[44px] w-full rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none focus:border-blue-500"
                placeholder="Правила агента"
                value={agent.description}
                onChange={(e) => patchAgent(i, { description: e.target.value })}
              />
            </div>
            <div className="mt-2">
              <div className="mb-1 flex items-center gap-2">
                <span className="text-xs font-medium text-slate-500">Инструменты</span>
                <Input
                  className="h-7 max-w-60 text-xs"
                  placeholder="имя инструмента"
                  value={toolDraft}
                  onChange={(e) => setToolDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') addTool(i);
                  }}
                />
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => addTool(i)}
                  disabled={!toolDraft.trim()}
                >
                  Добавить
                </Button>
              </div>
              {agent.tools.length === 0 ? (
                <div className="text-xs text-slate-400">Инструментов нет</div>
              ) : (
                <div className="flex flex-wrap gap-1.5">
                  {agent.tools.map((tool) => (
                    <span
                      key={tool}
                      className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-slate-50 px-2 py-0.5 text-xs text-slate-700"
                    >
                      {tool}
                      <button
                        className="text-slate-400 hover:text-red-600"
                        title="Удалить инструмент"
                        onClick={() => removeTool(i, tool)}
                      >
                        ×
                      </button>
                    </span>
                  ))}
                </div>
              )}
            </div>
            <div className="mt-2 grid grid-cols-2 gap-4">
              {capabilityBlock('skills', skills, 'Скиллы')}
              {capabilityBlock('commands', commands, 'Команды')}
            </div>
          </Card>
        );
      })}

      <Button variant="outline" onClick={addAgent}>
        + Агент
      </Button>
    </div>
  );
});