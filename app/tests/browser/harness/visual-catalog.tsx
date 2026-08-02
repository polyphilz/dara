import { useState } from 'react'
import { DaraButton } from '../../../src/components/DaraButton.tsx'
import { DaraInput } from '../../../src/components/DaraInput.tsx'
import { DaraPercentageControl } from '../../../src/components/DaraPercentageControl.tsx'
import { DaraSelect } from '../../../src/components/DaraSelect.tsx'
import { DaraShortcutRecorder } from '../../../src/components/DaraShortcutRecorder.tsx'
import { DaraText } from '../../../src/components/DaraText.tsx'
import { DaraToggle } from '../../../src/components/DaraToggle.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../../../src/components/dara-button-types.ts'
import {
  DaraTextTone,
  DaraTextVariant,
} from '../../../src/components/dara-text-types.ts'
import './visual-catalog.css'

const CatalogOption = {
  First: 'FIRST',
  Second: 'SECOND',
  Third: 'THIRD',
} as const

type CatalogOption =
  (typeof CatalogOption)[keyof typeof CatalogOption]

const catalogOptions = [
  { label: 'First option', value: CatalogOption.First },
  { label: 'Second option', value: CatalogOption.Second },
  { label: 'Third option', value: CatalogOption.Third },
] as const

const typeSpecimens = [
  { variant: DaraTextVariant.Display, sample: 'Display hero copy' },
  { variant: DaraTextVariant.Title, sample: 'Page and dialog titles' },
  { variant: DaraTextVariant.Heading, sample: 'How would you like to begin?' },
  { variant: DaraTextVariant.Subheading, sample: 'Section and group heading' },
  { variant: DaraTextVariant.Body, sample: 'Ordinary readable interface copy.' },
  {
    variant: DaraTextVariant.Supporting,
    sample: 'This device does not have any Dara data yet.',
  },
  { variant: DaraTextVariant.Label, sample: 'Desired retention' },
  { variant: DaraTextVariant.Caption, sample: 'Checkpoint 4 of 12 · 1.2 MB' },
  { variant: DaraTextVariant.Eyebrow, sample: 'Confirm change' },
  { variant: DaraTextVariant.Metric, sample: '128' },
] as const

const textTones = [
  DaraTextTone.Default,
  DaraTextTone.Muted,
  DaraTextTone.Accent,
  DaraTextTone.Success,
  DaraTextTone.Warning,
  DaraTextTone.Danger,
  DaraTextTone.Inherit,
] as const

const buttonVariants = [
  DaraButtonVariant.Surface,
  DaraButtonVariant.Ghost,
  DaraButtonVariant.Primary,
  DaraButtonVariant.Accent,
  DaraButtonVariant.Danger,
] as const

export function VisualCatalog() {
  const [selected, setSelected] = useState<CatalogOption>(CatalogOption.Second)
  const [toggle, setToggle] = useState(true)
  const [percentage, setPercentage] = useState(90)
  const [accelerator, setAccelerator] = useState('control+alt+super+KeyD')
  return (
    <main className="visual-catalog">
      {/*
       * The catalog is captured as two focused snapshots so each one fits the
       * established 1000x720 visual viewport without shrinking a specimen.
       */}
      <div className="catalog-group" id="catalog-type-group">
        <h1>Dara design system</h1>
        <section aria-labelledby="catalog-typography">
          <h2 id="catalog-typography">Typography</h2>
          <div className="catalog-type-grid">
            {typeSpecimens.map(({ sample, variant }) => (
              <div className="catalog-type-row" key={variant}>
                <DaraText
                  as="span"
                  className="catalog-type-role"
                  tone={DaraTextTone.Muted}
                  variant={DaraTextVariant.Eyebrow}
                >
                  {variant}
                </DaraText>
                <DaraText as="span" variant={variant}>
                  {sample}
                </DaraText>
              </div>
            ))}
          </div>
        </section>
        <section aria-labelledby="catalog-tones">
          <h2 id="catalog-tones">Text tones</h2>
          <div className="catalog-tone-row">
            {textTones.map((tone) => (
              <DaraText
                as="span"
                key={tone}
                tone={tone}
                variant={DaraTextVariant.Supporting}
              >
                {tone}
              </DaraText>
            ))}
          </div>
        </section>
        <section aria-labelledby="catalog-semantics">
          <h2 id="catalog-semantics">Semantic combinations</h2>
          <div className="catalog-semantic-grid">
            <DaraText as="h1" variant={DaraTextVariant.Title}>
              Restore from backup
            </DaraText>
            <DaraText as="h2" variant={DaraTextVariant.Heading}>
              Off-site backup
            </DaraText>
            <DaraText as="p" variant={DaraTextVariant.Body}>
              Dara keeps one interleaved pool of cards.
            </DaraText>
            <DaraText
              as="p"
              tone={DaraTextTone.Muted}
              variant={DaraTextVariant.Supporting}
            >
              Reviews stay available offline while a restore runs.
            </DaraText>
            <DaraText
              as="label"
              htmlFor="catalog-semantic-field"
              variant={DaraTextVariant.Label}
            >
              Bucket name
            </DaraText>
            <input id="catalog-semantic-field" readOnly value="dara/primary" />
            <DaraText
              as="span"
              tone={DaraTextTone.Muted}
              variant={DaraTextVariant.Caption}
            >
              Verified 12 minutes ago
            </DaraText>
            <DaraText
              as="span"
              tone={DaraTextTone.Accent}
              variant={DaraTextVariant.Eyebrow}
            >
              Confirm change
            </DaraText>
            <DaraText as="output" variant={DaraTextVariant.Metric}>
              42
            </DaraText>
          </div>
        </section>
      </div>
      <div className="catalog-group" id="catalog-control-group">
        <section aria-labelledby="catalog-buttons">
          <h2 id="catalog-buttons">Buttons</h2>
          <div className="catalog-grid catalog-button-grid">
            {buttonVariants.map((variant) => (
              <div className="catalog-state" key={variant}>
                <span>{variant}</span>
                <DaraButton variant={variant}>{variant}</DaraButton>
                <DaraButton disabled variant={variant}>Disabled</DaraButton>
                <DaraButton size={DaraButtonSize.Compact} variant={variant}>Compact</DaraButton>
              </div>
            ))}
          </div>
        </section>
        <section aria-labelledby="catalog-fields">
          <h2 id="catalog-fields">Fields and choices</h2>
          <div className="catalog-grid">
            <label className="catalog-state">
              <span>Input</span>
              <DaraInput aria-label="Catalog input" defaultValue="Editable value" />
              <DaraInput aria-label="Disabled catalog input" disabled value="Disabled value" readOnly />
            </label>
            <div className="catalog-state">
              <span>Listbox</span>
              <DaraSelect
                ariaLabel="Catalog choice"
                onSelect={setSelected}
                options={catalogOptions}
                value={selected}
              />
              <DaraSelect
                ariaLabel="Disabled catalog choice"
                disabled
                onSelect={() => undefined}
                options={catalogOptions}
                value={CatalogOption.First}
              />
            </div>
            <div className="catalog-state">
              <span>Toggle</span>
              <DaraToggle checked={toggle} label="Catalog toggle" onChange={setToggle} />
              <DaraToggle checked={false} disabled label="Disabled catalog toggle" onChange={() => undefined} />
            </div>
            <div className="catalog-state">
              <span>Percentage</span>
              <DaraPercentageControl
                label="Catalog percentage"
                max={99}
                min={70}
                onChange={setPercentage}
                value={percentage}
              />
            </div>
            <div className="catalog-state">
              <span>Shortcut</span>
              <DaraShortcutRecorder
                accelerator={accelerator}
                label="Catalog shortcut"
                onCapture={setAccelerator}
              />
            </div>
          </div>
        </section>
        <section aria-labelledby="catalog-feedback">
          <h2 id="catalog-feedback">Feedback</h2>
          <div className="catalog-feedback-grid">
            <p role="status">Loading deterministic content…</p>
            <p role="alert">A specific operation could not be completed.</p>
            <p className="catalog-warning">Review this change before continuing.</p>
            <p className="catalog-success">The operation completed successfully.</p>
          </div>
        </section>
      </div>
    </main>
  )
}
