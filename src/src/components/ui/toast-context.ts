import { createContext, useContext } from 'react';

export type ToastItem = {
  id: string;
  title?: string;
  description?: string;
  variant?: 'default' | 'destructive' | 'warning';
};

export type ToastContextValue = {
  toasts: ToastItem[];
  push: (toast: Omit<ToastItem, 'id'>) => void;
  dismiss: (id: string) => void;
};

export const ToastContext = createContext<ToastContextValue | null>(null);

export function useToast() {
  const context = useContext(ToastContext);
  if (!context) throw new Error('useToast must be used inside <ToasterProvider>');
  return context;
}
