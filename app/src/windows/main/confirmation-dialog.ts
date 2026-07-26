export const ConfirmationDialogInitialFocus = {
  Cancel: 'CANCEL',
  Confirm: 'CONFIRM',
} as const

export type ConfirmationDialogInitialFocus =
  (typeof ConfirmationDialogInitialFocus)[keyof typeof ConfirmationDialogInitialFocus]
