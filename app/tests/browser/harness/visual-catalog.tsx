import { useState } from 'react'
import { DaraButton } from '../../../src/components/DaraButton.tsx'
import { DaraInput } from '../../../src/components/DaraInput.tsx'
import { DaraPercentageControl } from '../../../src/components/DaraPercentageControl.tsx'
import { DaraSelect } from '../../../src/components/DaraSelect.tsx'
import { DaraShortcutRecorder } from '../../../src/components/DaraShortcutRecorder.tsx'
import { DaraToggle } from '../../../src/components/DaraToggle.tsx'
import {
  DaraButtonSize,
  DaraButtonVariant,
} from '../../../src/components/dara-button-types.ts'
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
      <h1>Dara control states</h1>
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
    </main>
  )
}
