import { DeleteObjectForm, Model } from 'mobx-model-ui';
import { observer } from 'mobx-react-lite';
import { Button } from '@/components/ui/button';
import { useForm } from '@/utils/mobx';
import { toaster } from '@/utils/toaster';

export interface DeleteObjectButtonProps {
  obj: Model;
  onDeleted?: () => void;
}

export const DeleteObjectButton = observer((props: DeleteObjectButtonProps) => {
  const { obj, onDeleted } = props;
  const form = useForm(() =>
    new DeleteObjectForm(
      obj,
      {},
      () => {
        toaster.show({ message: 'Объект удалён', intent: 'success' });
        onDeleted?.();
      },
      () => toaster.show({ message: 'Не удалось удалить объект', intent: 'danger' }),
    ),
  );

  const handleDelete = async (e: React.MouseEvent) => {
    e.stopPropagation();
    await form.submit();
  };

  return (
    <Button variant="ghost" size="sm" onClick={handleDelete} disabled={form.isLoading}>
      Удалить
    </Button>
  );
});