import { AlertCircle, X } from "lucide-react";

type ErrorAlertProps = {
  message: string | null;
  onDismiss: () => void;
};

export function ErrorAlert({ message, onDismiss }: ErrorAlertProps) {
  if (!message) return null;

  return (
    <div className="error-alert-banner" role="alert">
      <div className="alert-icon-wrap">
        <AlertCircle size={18} />
      </div>
      <div className="alert-body">
        <strong className="alert-title">Terjadi Kendala Teknis</strong>
        <p className="alert-message-text">{message}</p>
      </div>
      <button
        type="button"
        className="alert-dismiss-btn"
        onClick={onDismiss}
        title="Tutup Pesan Error"
      >
        <X size={16} />
      </button>
    </div>
  );
}
