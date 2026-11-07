import { useState, type ComponentProps, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';

export interface ConfirmButtonProps extends Omit<ComponentProps<typeof Button>, 'onClick'> {
  /** Что удаляется — подставляется в текст подтверждения. */
  name: string;
  /** Действие после подтверждения. */
  onConfirm: () => void | Promise<void>;
  children: ReactNode;
}

/** Кнопка разрушающего действия: сначала попап подтверждения (`ConfirmDialog`),
 *  действие — только по «Удалить». Клик не всплывает к карточке-родителю. */
export function ConfirmButton({ name, onConfirm, children, ...button }: ConfirmButtonProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <span onClick={(e) => e.stopPropagation()}>
      <Button {...button} onClick={() => setOpen(true)}>
        {children}
      </Button>
      {open && (
        <ConfirmDialog
          title={t('common.confirmDeleteTitle')}
          message={t('common.confirmDeleteMessage', { name })}
          onConfirm={() => {
            setOpen(false);
            void onConfirm();
          }}
          onCancel={() => setOpen(false)}
        />
      )}
    </span>
  );
}
