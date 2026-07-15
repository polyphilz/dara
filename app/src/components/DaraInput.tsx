import {
  forwardRef,
  type ComponentPropsWithoutRef,
} from 'react'
import { DARA_WRITING_ASSISTANCE_ATTRIBUTES } from './writing-assistance.ts'

type ManagedWritingAssistanceProp =
  | 'autoCapitalize'
  | 'autoComplete'
  | 'autoCorrect'
  | 'spellCheck'

export type DaraInputProps = Omit<
  ComponentPropsWithoutRef<'input'>,
  ManagedWritingAssistanceProp
>

/** A native input with browser and macOS writing assistance disabled. */
export const DaraInput = forwardRef<HTMLInputElement, DaraInputProps>(
  function DaraInput(props, ref) {
    return (
      <input
        {...props}
        autoCapitalize={DARA_WRITING_ASSISTANCE_ATTRIBUTES.autocapitalize}
        autoComplete={DARA_WRITING_ASSISTANCE_ATTRIBUTES.autocomplete}
        autoCorrect={DARA_WRITING_ASSISTANCE_ATTRIBUTES.autocorrect}
        ref={ref}
        spellCheck={false}
        {...{
          writingsuggestions:
            DARA_WRITING_ASSISTANCE_ATTRIBUTES.writingsuggestions,
        }}
      />
    )
  },
)
