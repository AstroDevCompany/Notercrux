import { actions, useApp } from '../state/app';
import { Dialog } from './Dialog';

export function ConfirmDialog() {
  const req = useApp((s) => s.confirm);
  if (!req) return null;
  const close = () => actions.ask(null);
  return (
    <Dialog title={req.title} onClose={close}>
      <p className="dialog-body">{req.message}</p>
      <div className="dialog-actions">
        <button className="btn" onClick={close}>
          Cancel
        </button>
        <button
          className={`btn ${req.destructive ? 'btn-danger' : 'btn-primary'}`}
          data-autofocus={req.destructive ? undefined : ''}
          onClick={() => {
            close();
            req.onConfirm();
          }}
        >
          {req.confirmLabel}
        </button>
      </div>
    </Dialog>
  );
}
