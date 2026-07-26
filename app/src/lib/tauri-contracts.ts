export const DaraIpcCommand = {
  AdoptLegacyZoom: 'adopt_legacy_zoom',
  CreateCardContent: 'create_card_content',
  DeleteCardContent: 'delete_card_content',
  DismissQuickAdd: 'dismiss_quick_add',
  GetSpikeStatus: 'get_spike_status',
  IngestClipboardImage: 'ingest_clipboard_image',
  IngestImageBytes: 'ingest_image_bytes',
  InstallSchedulerReplay: 'install_scheduler_replay',
  LoadCardContent: 'load_card_content',
  LoadDiagnostics: 'load_diagnostics',
  LoadHomeStats: 'load_home_stats',
  LoadReviewContext: 'load_review_context',
  LoadSchedulerReplaySnapshot: 'load_scheduler_replay_snapshot',
  LoadSettings: 'load_settings',
  MaintainMedia: 'maintain_media',
  MaintainSearch: 'maintain_search',
  OpenExternalUrl: 'open_external_url',
  PrepareDesiredRetentionReplay: 'prepare_desired_retention_replay',
  RecordGrade: 'record_grade',
  RenewMediaLease: 'renew_media_lease',
  SearchCardContent: 'search_card_content',
  SearchStatus: 'search_status',
  SelectNextReviewCard: 'select_next_review_card',
  SetAppearance: 'set_appearance',
  SetCardContentSuspended: 'set_card_content_suspended',
  SetKeyboardBindings: 'set_keyboard_bindings',
  SetLaunchAtLogin: 'set_launch_at_login',
  SetQuickAddFileDialogOpen: 'set_quick_add_file_dialog_open',
  SetZoomPercent: 'set_zoom_percent',
  ShowMain: 'show_main',
  ShowQuickAdd: 'show_quick_add',
  UndoLastGrade: 'undo_last_grade',
  UpdateCardContent: 'update_card_content',
} as const

export type DaraIpcCommand =
  (typeof DaraIpcCommand)[keyof typeof DaraIpcCommand]

export const DaraEvent = {
  BrowseCommand: 'browse-command',
  CardCreated: 'card-created',
  OpenHome: 'open-home',
  OpenSettings: 'open-settings',
  QuickAddShown: 'quick-add-shown',
  ReviewClockRefresh: 'review-clock-refresh',
  SettingsChanged: 'settings-changed',
  ZoomCommand: 'app-zoom-command',
} as const

export type DaraEvent = (typeof DaraEvent)[keyof typeof DaraEvent]
