import { useEffect, useState } from 'react'
import { native, type SpikeStatus } from '../../lib/native.ts'

const topology = [
  ['main', 'Ordinary NSWindow', 'Activates dara and becomes main/key'],
  ['quick-add', 'Non-activating NSPanel', 'Takes keyboard input over the current app'],
] as const

export function MainWindow() {
  const [status, setStatus] = useState<SpikeStatus | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [probe, setProbe] = useState('Place the caret here before opening quick add.')

  useEffect(() => {
    void native
      .getSpikeStatus()
      .then(setStatus)
      .catch((cause: unknown) => setError(String(cause)))
  }, [])

  const openQuickAdd = () => {
    setError(null)
    void native.showQuickAdd().catch((cause: unknown) => setError(String(cause)))
  }

  return (
    <main className="main-window">
      <header className="main-header">
        <div>
          <p className="eyebrow">Activation spike</p>
          <h1>dara</h1>
          <p className="lede">
            This window should behave like an ordinary Mac application window. The
            capture panel should not.
          </p>
        </div>
        <span className={status?.panel_ready ? 'status ready' : 'status'}>
          {status?.panel_ready ? 'Panel ready' : 'Checking panel'}
        </span>
      </header>

      <section className="topology" aria-labelledby="topology-heading">
        <div className="section-heading">
          <h2 id="topology-heading">Two native surfaces</h2>
          <button className="primary-action" type="button" onClick={openQuickAdd}>
            Open quick add
          </button>
        </div>
        <div className="topology-list">
          {topology.map(([label, kind, behavior]) => (
            <article className="topology-row" key={label}>
              <code>{label}</code>
              <strong>{kind}</strong>
              <span>{behavior}</span>
            </article>
          ))}
        </div>
      </section>

      <section className="focus-probe" aria-labelledby="focus-probe-heading">
        <div>
          <p className="eyebrow">Keyboard probe</p>
          <h2 id="focus-probe-heading">Exact in-app restoration</h2>
          <p>
            Put the caret here, open quick add, then dismiss it. The next keystroke should
            return to this exact field without a click.
          </p>
        </div>
        <textarea
          aria-label="Main-window focus restoration probe"
          rows={3}
          value={probe}
          onChange={(event) => setProbe(event.target.value)}
        />
      </section>

      <section className="shortcut-card" aria-labelledby="shortcuts-heading">
        <div>
          <p className="eyebrow">Global shortcuts</p>
          <h2 id="shortcuts-heading">Test from another application</h2>
        </div>
        <dl>
          <div>
            <dt>Quick add</dt>
            <dd>{status?.quick_add_shortcut ?? '⌃⌥⌘D'}</dd>
          </div>
          <div>
            <dt>Review window</dt>
            <dd>{status?.review_shortcut ?? '⌃⌥⌘R'}</dd>
          </div>
        </dl>
      </section>

      {status?.shortcut_errors.map((message) => (
        <p className="error-banner" key={message} role="alert">
          {message}
        </p>
      ))}
      {error && (
        <p className="error-banner" role="alert">
          {error}
        </p>
      )}
    </main>
  )
}
