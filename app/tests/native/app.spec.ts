import '@wdio/tauri-service'
import { browser, expect } from '@wdio/globals'
import { DaraIpcCommand } from '../../src/lib/tauri-contracts.ts'

const NativeWindowLabel = {
  Main: 'main',
  QuickAdd: 'quick-add',
} as const

const NativeTestPluginCommand = {
  GetWindowStates: 'plugin:wdio|get_window_states',
} as const

interface NativeWindowState {
  label: string
  title: string
  is_visible: boolean
  is_focused: boolean
}

describe('native Tauri boundary', () => {
  it('discovers both persistent windows and drives real IPC', async () => {
    await browser.waitUntil(async () => {
      const windows = await browser.tauri.listWindows()
      return windows.includes(NativeWindowLabel.Main) &&
        windows.includes(NativeWindowLabel.QuickAdd)
    })
    expect(await visibleWindowLabels()).toEqual([])

    const settings = await browser.tauri.execute(
      (tauri, command) => tauri.core.invoke(command),
      DaraIpcCommand.LoadSettings,
    )
    expect(settings).toMatchObject({ zoomPercent: 100 })

    await browser.tauri.execute(
      (tauri, command) => tauri.core.invoke(command),
      DaraIpcCommand.ShowMain,
    )
    await browser.tauri.switchWindow(NativeWindowLabel.Main)
    await expect(browser.$('main')).toBeDisplayed()
    await expect(browser.$('h1')).toHaveText('Dara')
    expect(await visibleWindowLabels()).toContain(NativeWindowLabel.Main)
  })

  it('shows and dismisses the persistent Quick Add window without recreation', async () => {
    await browser.tauri.execute(
      (tauri, command) => tauri.core.invoke(command),
      DaraIpcCommand.ShowMain,
    )
    const before = (await browser.tauri.listWindows()).toSorted()
    await browser.tauri.execute(
      (tauri, command) => tauri.core.invoke(command),
      DaraIpcCommand.ShowQuickAdd,
    )
    await browser.tauri.switchWindow(NativeWindowLabel.QuickAdd)
    await expect(browser.$('main')).toBeDisplayed()
    await expect(browser.$('h1')).toHaveText('Quick add')
    expect(await visibleWindowLabels()).toEqual([
      NativeWindowLabel.Main,
      NativeWindowLabel.QuickAdd,
    ])

    await browser.tauri.execute(
      (tauri, command) => tauri.core.invoke(command),
      DaraIpcCommand.DismissQuickAdd,
    )
    expect((await browser.tauri.listWindows()).toSorted()).toEqual(before)
    expect(await visibleWindowLabels()).toEqual([NativeWindowLabel.Main])
  })

  it('creates, browses, suspends, resumes, and deletes through real UI and IPC', async () => {
    await browser.tauri.execute(
      (tauri, command) => tauri.core.invoke(command),
      DaraIpcCommand.ShowQuickAdd,
    )
    await browser.tauri.switchWindow(NativeWindowLabel.QuickAdd)
    const front = browser.$('[role="textbox"][aria-label="Front"]')
    const back = browser.$('[role="textbox"][aria-label="Back"]')
    await front.setValue('Native boundary question')
    await back.setValue('Native boundary answer')
    await expect(front).toHaveText('Native boundary question')
    await expect(back).toHaveText('Native boundary answer')
    await browser.$('button=Add').click()

    await browser.waitUntil(async () => {
      const result = await browser.tauri.execute(
        (tauri, [command, input]) => tauri.core.invoke(command, { input }),
        [
          DaraIpcCommand.SearchCardContent,
          { query: 'Native boundary question', limit: 2, offset: 0 },
        ] as const,
      )
      return (result as { items: unknown[] }).items.length === 1
    })
    const searchResult = await browser.tauri.execute(
      (tauri, [command, input]) => tauri.core.invoke(command, { input }),
      [
        DaraIpcCommand.SearchCardContent,
        { query: 'Native boundary question', limit: 2, offset: 0 },
      ] as const,
    ) as {
      items: Array<{ cardContent: { id: string } }>
    }
    const cardContentId = searchResult.items[0]!.cardContent.id
    const loaded = await browser.tauri.execute(
      (tauri, [command, id]) =>
        tauri.core.invoke(command, { cardContentId: id }),
      [DaraIpcCommand.LoadCardContent, cardContentId] as const,
    )
    expect(loaded).toMatchObject({
      cardContent: {
        frontMd: 'Native boundary question',
        id: cardContentId,
      },
    })

    await browser.tauri.execute(
      (tauri, command) => tauri.core.invoke(command),
      DaraIpcCommand.ShowMain,
    )
    await browser.tauri.switchWindow(NativeWindowLabel.Main)
    await browser.$('button=Browse').click()
    const result = browser.$('.card-result')
    await expect(result).toHaveText(
      expect.stringContaining('Native boundary question'),
    )
    await expect(browser.$('article*=Native boundary answer')).toBeDisplayed()

    await browser.$('button*=Pause').click()
    await expect(browser.$('button*=Resume')).toBeDisplayed()
    await browser.$('button*=Resume').click()
    await expect(browser.$('button*=Pause')).toBeDisplayed()

    await browser.$('button=Delete').click()
    await expect(browser.$('[role="alert"]')).toHaveText(
      expect.stringContaining('Delete this card?'),
    )
    await browser.$('button=Delete card').click()
    await expect(browser.$('p=No cards found.')).toBeDisplayed()
  })
})

async function visibleWindowLabels(): Promise<string[]> {
  const states = await browser.tauri.execute(
    (tauri, command) => tauri.core.invoke(command),
    NativeTestPluginCommand.GetWindowStates,
  ) as NativeWindowState[]
  return states
    .filter((state) => state.is_visible)
    .map((state) => state.label)
    .toSorted()
}
