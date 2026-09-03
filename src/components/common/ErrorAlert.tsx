import { AlertCircle, X } from "lucide-react";
import { useI18n } from "../../i18n";

type ErrorAlertProps = {
  message: string | null;
  onDismiss: () => void;
};

export function ErrorAlert({ message, onDismiss }: ErrorAlertProps) {
  const { t } = useI18n();
  if (!message) return null;

  return (
    <div className="error-alert-banner" role="alert">
      <div className="alert-icon-wrap">
        <AlertCircle size={18} />
      </div>
      <div className="alert-body">
        <strong className="alert-title">{t("error.title")}</strong>
        <p className="alert-message-text">{message}</p>
      </div>
      <button
        type="button"
        className="alert-dismiss-btn"
        onClick={onDismiss}
        title={t("error.dismiss")}
        aria-label={t("error.dismiss")}
      >
        <X size={16} />
      </button>
    </div>
  );
}
