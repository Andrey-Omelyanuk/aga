import { Variable } from 'mobx-model-ui';
import { observer } from 'mobx-react-lite';
import { Input } from '@/components/ui/input';

export interface StringInputProps {
  input: Variable<string>;
  label?: string;
  placeholder?: string;
  disabled?: boolean;
  autoFocus?: boolean;
  onPressEnter?: () => void;
  className?: string;
}

export const StringInput = observer((props: StringInputProps) => {
  const { input, label, placeholder, disabled = false, autoFocus, onPressEnter, className } = props;

  const onChange = (e: React.ChangeEvent<HTMLInputElement>) => input.set(e.target.value);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') onPressEnter?.();
  };

  return (
    <label className="flex items-center gap-2 text-sm text-slate-500">
      {label && <span>{label}</span>}
      <Input
        className={className}
        value={input.value ?? ''}
        onChange={onChange}
        onKeyDown={handleKeyDown}
        placeholder={placeholder}
        autoFocus={autoFocus}
        disabled={disabled}
      />
    </label>
  );
});