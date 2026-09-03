import { AlertCircle } from "lucide-react";

type ErrorAlertProps = {
  message: string | null;
  onDismiss: () => void;
};

export function ErrorAlert({ message, onDismiss }: ErrorAlertProps) {
  if (!message) return null;

  return (
    <div className="alert error-alert">
      <AlertCircle size={18} />
      <div><strong>Ada masalah</strong><span>{message}</span></div>
      <button onClick={onDismiss}>×</button>
    </div>
  );
}
