import {
  BrowserHarnessSurface,
  BrowserScenarioId,
} from '../harness/scenarios.ts'
import type { DaraBrowserTestApi } from '../harness/ipc-driver.ts'
import { expect, test } from '../fixtures/test.ts'
import { CardContentType } from '../../../src/review/contracts.ts'
import { MainWindowRoutePath } from '../../../src/windows/main/main-window-routes.ts'

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

test('Add saves consecutive cards without leaving the fresh form', async ({
  page,
}) => {
  await page.goto(
    `/tests/browser/harness/?scenario=${BrowserScenarioId.MainReviewBasic}&surface=${BrowserHarnessSurface.Main}`,
  )
  await page.getByRole('button', { name: 'Add' }).click()
  const form = page.getByRole('region', { name: 'Add a card' })
  const front = form.getByRole('textbox', { name: 'Front' })
  const back = form.getByRole('textbox', { name: 'Back' })
  const source = form.getByRole('textbox', { name: /Source/ })

  await front.fill('First question')
  await back.fill('First answer')
  await source.fill('First source')
  await page.keyboard.press('Meta+Enter')

  await expect(page).toHaveURL(new RegExp(`#${MainWindowRoutePath.Add}$`))
  await expect(form).toBeVisible()
  await expect(front).toHaveText('')
  await expect(back).toHaveText('')
  await expect(source).toHaveValue('')
  await expect(front).toBeFocused()

  await front.fill('Second question')
  await back.fill('Second answer')
  await page.keyboard.press('Meta+Enter')
  await expect(front).toHaveText('')
  await expect(front).toBeFocused()

  const snapshot = await page.evaluate(() =>
    (window as Window & { __DARA_BROWSER_TEST__: DaraBrowserTestApi })
      .__DARA_BROWSER_TEST__.snapshot(),
  )
  expect(snapshot.cards.map(({ content }) => content)).toEqual([
    {
      backMd: 'First answer',
      frontMd: 'First question',
      source: 'First source',
      type: CardContentType.Basic,
    },
    {
      backMd: 'Second answer',
      frontMd: 'Second question',
      source: null,
      type: CardContentType.Basic,
    },
  ])
  expect(new Set(snapshot.cards.map(({ mediaLeaseId }) => mediaLeaseId)).size).toBe(
    2,
  )
})
