import { DeleteObjectForm, Model } from 'mobx-model-ui';
import { observer } from 'mobx-react-lite';
import { useTranslation } from 'react-i18next';
import { ConfirmButton } from '@/components/ui/ConfirmButton';
import { useForm } from '@/utils/mobx';
import { toaster } from '@/utils/toaster';

export interface DeleteObjectButtonProps {
  obj: Model;
  /** Название объекта в попапе подтверждения. */
  name: string;
  onDeleted?: () => void;
}

export const DeleteObjectButton = observer((props: DeleteObjectButtonProps) => {
  const { t } = useTranslation();
  const { obj, name, onDeleted } = props;
  const form = useForm(() =>
    new DeleteObjectForm(
      obj,
      {},
      () => {
        toaster.show({ message: t('common.objectDeleted'), intent: 'success' });
        onDeleted?.();
      },
      () => toaster.show({ message: t('common.failedToDelete'), intent: 'danger' }),
    ),
  );

  // Удаление необратимо — только после подтверждения в попапе.
  return (
    <ConfirmButton
      variant="ghost"
      size="sm"
      name={name}
      disabled={form.isLoading}
      onConfirm={() => form.submit()}
    >
      {t('common.delete')}
    </ConfirmButton>
  );
});