import * as React from 'react';
import { Check } from 'lucide-react';
import { cn } from '@/lib/utils';

export interface CheckboxProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'type'> {
  checked?: boolean;
  onCheckedChange?: (checked: boolean) => void;
}

const Checkbox = React.forwardRef<HTMLInputElement, CheckboxProps>(
  ({ className, checked, onCheckedChange, onChange, ...props }, ref) => (
    <label className="inline-flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center rounded-sm border border-input bg-background shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-50 data-[state=checked]:border-primary data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground">
      <input
        ref={ref}
        type="checkbox"
        checked={checked}
        onChange={(e) => {
          onChange?.(e);
          onCheckedChange?.(e.target.checked);
        }}
        className="sr-only"
        {...props}
      />
      <Check className={cn('h-3.5 w-3.5 text-current opacity-0 transition-opacity', checked && 'opacity-100')} />
    </label>
  ),
);
Checkbox.displayName = 'Checkbox';

export { Checkbox };
