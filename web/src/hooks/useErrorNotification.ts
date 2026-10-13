import { useEffect } from 'react';
import { Notification, type NotificationPosition } from 'animal-island-ui';

/**
 * Surface a load failure as an island notification.
 *
 * Every page that can load a ROM reports the same way, and clearing the error
 * once it is dismissed is what keeps the same failure reportable a second time.
 *
 * The corner is a parameter because the phone layout keeps its controls at the
 * bottom: a notification there would sit on top of them until it times out.
 */
export function useErrorNotification(
  error: string | null,
  clearError: () => void,
  position: NotificationPosition = 'bottomRight',
): void {
  useEffect(() => {
    if (!error) return;
    Notification.error({
      key: 'nes-error',
      message: '出错了',
      description: error,
      position,
      duration: 6,
      onClose: clearError,
    });
  }, [error, clearError, position]);
}
