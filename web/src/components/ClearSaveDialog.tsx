import { Button, Modal } from 'animal-island-ui';
import { useI18n } from '../i18n';

interface ClearSaveDialogProps {
  open: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

/** Second confirmation for deleting a cartridge's battery save. */
export function ClearSaveDialog({ open, onCancel, onConfirm }: ClearSaveDialogProps) {
  const { t } = useI18n();

  return (
    <Modal
      open={open}
      title={t('clearSave.title')}
      width={420}
      typeSpeed={25}
      onClose={onCancel}
      footer={
        <>
          <Button onClick={onCancel}>{t('clearSave.cancel')}</Button>
          <Button
            type="primary"
            danger
            onClick={() => {
              onCancel();
              onConfirm();
            }}
          >
            {t('clearSave.confirm')}
          </Button>
        </>
      }
    >
      {t('clearSave.body')}
    </Modal>
  );
}
