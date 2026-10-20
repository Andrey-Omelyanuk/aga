import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SaveObjectForm, STRING, Variable } from 'mobx-model-ui';
import { Button } from '@/components/ui/button';
import { StringInput } from '@/components/core/inputs';
import { Project } from '@/models/project';
import { useForm } from '@/utils/mobx';
import { toaster } from '@/utils/toaster';

export interface ProjectCreateProps {
  onCreated?: (project: Project) => void;
}

const CreateForm = observer(({ onCreated }: ProjectCreateProps) => {
  const { t } = useTranslation();
  const obj = useState(() => new Project())[0];
  const form = useForm(
    () =>
      new SaveObjectForm<Project>(
        obj,
        {
          git_url: new Variable(STRING({ required: true }), { value: '' }),
        },
        (project) => {
          toaster.show({ message: t('projects.added'), intent: 'success' });
          onCreated?.(project as Project);
        },
        () => toaster.show({ message: t('projects.addError'), intent: 'danger' }),
      ),
  );

  const submit = async () => {
    await form.submit();
  };

  return (
    <div className="mb-4 flex items-center gap-2">
      <StringInput
        input={form.inputs.git_url}
        placeholder={t('projects.gitUrlPlaceholder')}
        onPressEnter={submit}
      />
      <Button variant="secondary" onClick={submit} disabled={form.isLoading}>
        {t('projects.addProject')}
      </Button>
    </div>
  );
});

export const ProjectCreate = observer((props: ProjectCreateProps) => {
  // nonce пересоздаёт форму после успешного создания — чтобы в форме был
  // новый пустой объект, а не уже сохранённый (иначе следующий submit
  // обновил бы предыдущий проект вместо создания нового).
  const [nonce, setNonce] = useState(0);
  return (
    <CreateForm
      key={nonce}
      onCreated={(project) => {
        setNonce((n) => n + 1);
        props.onCreated?.(project);
      }}
    />
  );
});