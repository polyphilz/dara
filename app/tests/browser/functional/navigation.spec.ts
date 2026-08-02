import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import { expect, test } from '../fixtures/test.ts'

const ACTIVITY_LAYOUT_SAMPLE_COUNT = 4
const ACTIVITY_LAYOUT_SAMPLES_KEY = '__daraActivityCalendarLayoutSamples'

type ActivityLayoutSample = {
  calendarX: number
  containerX: number
  marginLeft: string
}

test('Home activity graph keeps its weekday gutter from the first frame', async ({
  page,
}) => {
  await page.addInitScript(
    ({ sampleCount, samplesKey }) => {
      Reflect.set(window, samplesKey, [])

      const waitForCalendar = () => {
        const calendar = document.querySelector<SVGElement>(
          '.react-activity-calendar__calendar',
        )
        const container = document.querySelector<HTMLElement>(
          '.react-activity-calendar',
        )

        if (!calendar || !container) {
          window.requestAnimationFrame(waitForCalendar)
          return
        }

        let frameCount = 0
        const sampleFrame = () => {
          const samples = Reflect.get(window, samplesKey) as ActivityLayoutSample[]
          samples.push({
            calendarX: calendar.getBoundingClientRect().x,
            containerX: container.getBoundingClientRect().x,
            marginLeft: window.getComputedStyle(calendar).marginLeft,
          })
          frameCount += 1

          if (frameCount < sampleCount) {
            window.requestAnimationFrame(sampleFrame)
          }
        }

        sampleFrame()
      }

      window.requestAnimationFrame(waitForCalendar)
    },
    {
      sampleCount: ACTIVITY_LAYOUT_SAMPLE_COUNT,
      samplesKey: ACTIVITY_LAYOUT_SAMPLES_KEY,
    },
  )

  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.waitForFunction(
    ({ sampleCount, samplesKey }) => {
      const samples = Reflect.get(window, samplesKey) as
        | ActivityLayoutSample[]
        | undefined
      return samples?.length === sampleCount
    },
    {
      sampleCount: ACTIVITY_LAYOUT_SAMPLE_COUNT,
      samplesKey: ACTIVITY_LAYOUT_SAMPLES_KEY,
    },
  )

  const samples = await page.evaluate(samplesKey => {
    return Reflect.get(window, samplesKey) as ActivityLayoutSample[]
  }, ACTIVITY_LAYOUT_SAMPLES_KEY)

  expect([...new Set(samples.map(sample => sample.calendarX))]).toHaveLength(1)
  expect([...new Set(samples.map(sample => sample.containerX))]).toHaveLength(1)
  expect([...new Set(samples.map(sample => sample.marginLeft))]).toEqual([
    '28px',
  ])
})

test('Add protects a dirty draft while explicit Cancel discards it', async ({
  page,
}) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: 'Add' }).click()
  const front = page.getByRole('textbox', { name: 'Front' })
  await front.fill('unfinished add draft')

  await page.getByRole('button', { name: 'Home' }).click()
  const dialog = page.getByRole('alertdialog', {
    name: 'Discard unsaved changes?',
  })
  await expect(dialog).toBeVisible()
  await expect(front).toHaveText('unfinished add draft')

  await dialog.getByRole('button', { name: 'Keep editing' }).click()
  await expect(dialog).toBeHidden()
  await expect(front).toBeFocused()

  await page.getByRole('button', { name: 'Cancel' }).click()
  await expect(page.getByRole('region', { name: 'Review activity' })).toBeVisible()
  await expect(dialog).toBeHidden()
})
