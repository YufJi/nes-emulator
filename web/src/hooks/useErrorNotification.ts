import { useEffect } from 'react';
import { Notification, type NotificationPosition } from 'animal-island-ui';
import { useI18n } from '../i18n';

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
  const { t } = useI18n();

  useEffect(() => {
    if (!error) return;
    Notification.error({
      key: 'nes-error',
      message: t('notification.failed'),
      description: error,
      position,
      duration: 6,
      onClose: clearError,
    });
  }, [error, clearError, position, t]);
}
