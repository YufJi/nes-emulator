import { Button, Modal } from 'animal-island-ui';

interface ClearSaveDialogProps {
  open: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

/** Second confirmation for deleting a cartridge's battery save. */
export function ClearSaveDialog({ open, onCancel, onConfirm }: ClearSaveDialogProps) {
  return (
    <Modal
      open={open}
      title="清空卡带存档？"
      width={420}
      typeSpeed={25}
      onClose={onCancel}
      footer={
        <>
          <Button onClick={onCancel}>取消</Button>
          <Button
            type="primary"
            danger
            onClick={() => {
              onCancel();
              onConfirm();
            }}
          >
            清空并重新开始
          </Button>
        </>
      }
    >
      删除该卡带的电池存档并重新开始，此操作无法撤销。
    </Modal>
  );
}
