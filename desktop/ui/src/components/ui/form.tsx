import { forwardRef, type InputHTMLAttributes } from 'react'
import { cn } from '@/lib/utils'

/**
 * Form primitives (shadcn copy-in pattern). State lives in react-hook-form
 * (+ zod resolvers at the call site); these components only handle layout,
 * label/error wiring and theme styling — no second form state machine.
 *
 * ```tsx
 * const form = useForm({ resolver: zodResolver(schema) })
 * <Form onSubmit={form.handleSubmit(onValid)}>
 *   <FormField name="title" label="Title" error={form.formState.errors.title?.message}>
 *     <FormInput {...form.register('title')} invalid={!!form.formState.errors.title} />
 *   </FormField>
 * </Form>
 * ```
 */

export function Form({ className, ...props }: React.ComponentPropsWithoutRef<'form'>) {
  return <form className={cn('space-y-md', className)} noValidate {...props} />
}

type FormFieldProps = {
  /** Field name — used as the label's htmlFor and the error's aria target. */
  name: string
  label: string
  error?: string
  hint?: string
  required?: boolean
  children: React.ReactNode
}

export function FormField({ name, label, error, hint, required, children }: FormFieldProps) {
  return (
    <div className="space-y-xs">
      <label
        htmlFor={name}
        className={cn('block font-label-md text-on-surface', error && 'text-error')}
      >
        {label}
        {required && <span aria-hidden="true" className="text-error ml-0.5">*</span>}
      </label>
      {children}
      {error ? (
        <p id={`${name}-error`} role="alert" className="font-body-sm text-error">
          {error}
        </p>
      ) : hint ? (
        <p id={`${name}-hint`} className="font-body-sm text-on-surface-variant">{hint}</p>
      ) : null}
    </div>
  )
}

/** Standard text input with error styling wired for FormField. */
export const FormInput = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement> & { invalid?: boolean }>(
  function FormInput({ className, invalid, ...props }, ref) {
    return (
      <input
        ref={ref}
        aria-invalid={invalid || undefined}
        className={cn(
          'w-full rounded-lg border border-outline-variant/50 bg-surface-container-lowest px-sm py-xs font-body-md text-on-surface',
          'placeholder:text-on-surface-variant/60 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
          invalid && 'border-error focus-visible:outline-error',
          className,
        )}
        {...props}
      />
    )
  },
)
