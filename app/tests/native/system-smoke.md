# Dara macOS release smoke

Record the Dara version, macOS version, hardware/display configuration, tester, date, and result
for each item. This checklist covers environmental behavior that the automated native lane does
not claim.

- Invoke both global shortcuts from a normal external app and from Dara.
- Confirm Quick Add receives a caret without Dock/menu identity and restores focus after save and
  cancel.
- Confirm click-away preserves the application selected by the user.
- Open Quick Add over a fullscreen app and on another Space and monitor.
- Repeat show/hide across sleep/wake and a monitor change.
- Exercise real `Meta+C/V/X/A/Z`, image paste, IME composition, and dead keys in both WebViews.
- Confirm a native file picker does not trigger Quick Add click-away dismissal.
- Change system Light/Dark appearance while both persistent windows are hidden and visible.
- On a Retina display, inspect activity cells, icons, focus outlines, inline images, and the
  occlusion image/SVG overlay for sharpness and alignment.
- Run a brief VoiceOver pass over navigation, editors, grading, listboxes, dialogs, and occlusion
  controls.
