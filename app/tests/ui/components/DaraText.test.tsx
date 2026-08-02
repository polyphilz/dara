import { fireEvent, render } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import { DaraText } from '../../../src/components/DaraText.tsx'
import {
  DaraTextTone,
  DaraTextVariant,
} from '../../../src/components/dara-text-types.ts'

const variantClasses: Record<DaraTextVariant, string> = {
  [DaraTextVariant.Display]: 'dara-text-display',
  [DaraTextVariant.Title]: 'dara-text-title',
  [DaraTextVariant.Heading]: 'dara-text-heading',
  [DaraTextVariant.Subheading]: 'dara-text-subheading',
  [DaraTextVariant.Body]: 'dara-text-body',
  [DaraTextVariant.Supporting]: 'dara-text-supporting',
  [DaraTextVariant.Label]: 'dara-text-label',
  [DaraTextVariant.Caption]: 'dara-text-caption',
  [DaraTextVariant.Eyebrow]: 'dara-text-eyebrow',
  [DaraTextVariant.Metric]: 'dara-text-metric',
}

const toneClasses: Record<DaraTextTone, string> = {
  [DaraTextTone.Default]: 'dara-text-tone-default',
  [DaraTextTone.Muted]: 'dara-text-tone-muted',
  [DaraTextTone.Accent]: 'dara-text-tone-accent',
  [DaraTextTone.Success]: 'dara-text-tone-success',
  [DaraTextTone.Warning]: 'dara-text-tone-warning',
  [DaraTextTone.Danger]: 'dara-text-tone-danger',
  [DaraTextTone.Inherit]: 'dara-text-tone-inherit',
}

test('renders the requested semantic element rather than one implied by the variant', () => {
  const { getByText } = render(
    <DaraText as="h2" variant={DaraTextVariant.Body}>
      Off-site backup
    </DaraText>,
  )

  expect(getByText('Off-site backup').tagName).toBe('H2')
})

test.each(Object.values(DaraTextVariant))(
  'maps the %s variant to its role class',
  (variant) => {
    const { getByText } = render(
      <DaraText as="span" variant={variant}>
        Specimen
      </DaraText>,
    )

    const element = getByText('Specimen')
    expect(element.className).toContain('dara-text')
    expect(element.className).toContain(variantClasses[variant])
  },
)

test.each(Object.values(DaraTextTone))(
  'maps the %s tone to its tone class',
  (tone) => {
    const { getByText } = render(
      <DaraText as="span" tone={tone} variant={DaraTextVariant.Supporting}>
        Specimen
      </DaraText>,
    )

    expect(getByText('Specimen').className).toContain(toneClasses[tone])
  },
)

test('applies the default tone and composes the caller layout class', () => {
  const { getByText } = render(
    <DaraText
      as="p"
      className="settings-section-description"
      variant={DaraTextVariant.Supporting}
    >
      This device does not have any Dara data yet.
    </DaraText>,
  )

  const paragraph = getByText('This device does not have any Dara data yet.')
  expect(paragraph.className).toContain('dara-text-tone-default')
  expect(paragraph.className).toContain('settings-section-description')
})

test('forwards identity, ARIA, data, and event attributes to the element', () => {
  const onClick = vi.fn()
  const { getByTestId } = render(
    <DaraText
      aria-live="polite"
      as="output"
      data-testid="restore-status"
      id="restore-status"
      onClick={onClick}
      variant={DaraTextVariant.Caption}
      tone={DaraTextTone.Success}
    >
      Ready to restore
    </DaraText>,
  )

  const status = getByTestId('restore-status')
  expect(status.tagName).toBe('OUTPUT')
  expect(status.getAttribute('id')).toBe('restore-status')
  expect(status.getAttribute('aria-live')).toBe('polite')

  fireEvent.click(status)
  expect(onClick).toHaveBeenCalledTimes(1)
})

test('keeps heading and label markup queryable by their semantic roles', () => {
  const { getByLabelText, getByRole } = render(
    <div>
      <DaraText as="h1" variant={DaraTextVariant.Title}>
        Settings
      </DaraText>
      <DaraText
        as="label"
        htmlFor="desired-retention"
        variant={DaraTextVariant.Label}
      >
        Desired retention
      </DaraText>
      <input id="desired-retention" />
    </div>,
  )

  expect(getByRole('heading', { level: 1, name: 'Settings' })).toBeTruthy()
  expect(getByLabelText('Desired retention')).toBeTruthy()
})

test('adds no focus stop of its own', () => {
  const { getByText } = render(
    <DaraText as="p" variant={DaraTextVariant.Body}>
      Ordinary copy
    </DaraText>,
  )

  expect(getByText('Ordinary copy').getAttribute('tabindex')).toBeNull()
})
