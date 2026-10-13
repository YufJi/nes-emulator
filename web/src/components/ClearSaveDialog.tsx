import type { CSSProperties } from 'react';
import { Button, Modal } from 'animal-island-ui';
import { useI18n } from '../i18n';

/**
 * The mask fills the physical screen (`viewport-fit=cover`), so the dialog it
 * centres has to be kept inside the safe area itself — a `Modal` has no prop
 * for its own box, but it does take one for the mask.
 */
const SAFE_AREA_PADDING: CSSProperties = {
  paddingTop: 'env(safe-area-inset-top, 0px)',
  paddingRight: 'env(safe-area-inset-right, 0px)',
  paddingBottom: 'env(safe-area-inset-bottom, 0px)',
  paddingLeft: 'env(safe-area-inset-left, 0px)',
};

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
      maskStyle={SAFE_AREA_PADDING}
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
