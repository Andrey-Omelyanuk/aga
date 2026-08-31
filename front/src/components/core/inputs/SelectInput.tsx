import { Model, ObjectInput } from 'mobx-model-ui';
import { observer } from 'mobx-react-lite';
import { Select } from '@/components/ui/select';

export interface SelectInputProps<M extends Model> {
  input: ObjectInput<M>;
  label?: string;
  optionKey?: (item: M) => string;
  optionLabel?: (item: M) => string;
  emptyLabel?: string;
  className?: string;
}

export const SelectInput = observer(<M extends Model>(props: SelectInputProps<M>) => {
  const { input, label, optionKey = (item) => String(item.id), optionLabel = (item) => String(item.id), emptyLabel = 'Выберите…', className } = props;

  const items = input.options?.items ?? [];

  const onChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const value = e.target.value;
    if (value) input.setFromString(value);
    else input.set(undefined);
  };

  return (
    <label className="flex items-center gap-2 text-sm text-slate-500">
      {label && <span>{label}</span>}
      <Select
        className={className}
        value={input.value !== undefined && input.value !== null ? String(input.value) : ''}
        onChange={onChange}
      >
        <option value="">{emptyLabel}</option>
        {items.map((item) => {
          const value = optionKey(item);
          return (
            <option key={value} value={value}>
              {optionLabel(item)}
            </option>
          );
        })}
      </Select>
    </label>
  );
});