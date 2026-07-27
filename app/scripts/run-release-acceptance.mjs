import { createHash } from 'node:crypto'
import {
  createReadStream,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { basename, dirname, join, resolve, sep } from 'node:path'
import { spawn, spawnSync } from 'node:child_process'

const AcceptanceCommand = Object.freeze({
  PrepareClean: 'prepare-clean',
  Launch: 'launch',
  CheckInterruptedDownload: 'check-interrupted-download',
  CheckClean: 'check-clean',
  CheckpointRestart: 'checkpoint-restart',
  CheckRestart: 'check-restart',
  PrepareUpgrade: 'prepare-upgrade',
  CheckUpgrade: 'check-upgrade',
  ProveUpgradeRestore: 'prove-upgrade-restore',
  Help: 'help',
})

const CardContentType = Object.freeze({
  Basic: 'BASIC',
  Cloze: 'CLOZE',
  Occlusion: 'OCCLUSION',
})

const ReviewEventType = Object.freeze({
  Revoke: 'REVOKE',
})

const ReviewCardStatus = Object.freeze({
  Suspended: 'SUSPENDED',
})

const KeyboardCommand = Object.freeze({
  QuickAdd: 'QUICK_ADD',
  Review: 'REVIEW',
  Home: 'HOME',
})

const appRoot = resolve(import.meta.dirname, '..')
const dataRoot = resolve(appRoot, '.data')
const defaultAppPath = resolve(
  appRoot,
  'src-tauri/target/release/bundle/macos/Dara.app',
)
const fixtureDescriptionFile = 'release-acceptance-fixture.json'
const restartCheckpointFile = '.release-acceptance-restart.json'
const launchRecordFile = '.release-acceptance-launch.json'
const receiptFile = 'semantic-search-verification.json'
const sidecarPidFile = 'llama-server.pid'
const sidecarLogFile = join('logs', 'llama-server.log')
const modelManifestRelativePath = join(
  'Contents',
  'Resources',
  'embedding-indexes',
  'jina-v1.json',
)
const goldenFixturesRelativePath = join(
  'Contents',
  'Resources',
  'embedding-indexes',
  'jina-v1-golden.json',
)
const releaseManifestRelativePath = join(
  'Contents',
  'Resources',
  'release',
  'llama-server.json',
)
const sidecarRelativePath = join(
  'Contents',
  'Resources',
  'bin',
  'llama-server',
)
const appExecutableRelativePath = join('Contents', 'MacOS', 'dara')

await main()

async function main() {
  mkdirSync(dataRoot, { recursive: true })
  const command = process.argv[2] ?? AcceptanceCommand.Help
  const arguments_ = process.argv.slice(3)

  switch (command) {
    case AcceptanceCommand.PrepareClean:
      prepareClean(arguments_)
      break
    case AcceptanceCommand.Launch:
      launch(arguments_)
      break
    case AcceptanceCommand.CheckInterruptedDownload:
      checkInterruptedDownload(arguments_)
      break
    case AcceptanceCommand.CheckClean:
      await checkClean(arguments_)
      break
    case AcceptanceCommand.CheckpointRestart:
      await checkpointRestart(arguments_)
      break
    case AcceptanceCommand.CheckRestart:
      await checkRestart(arguments_)
      break
    case AcceptanceCommand.PrepareUpgrade:
      prepareUpgrade(arguments_)
      break
    case AcceptanceCommand.CheckUpgrade:
      checkUpgrade(arguments_)
      break
    case AcceptanceCommand.ProveUpgradeRestore:
      proveUpgradeRestore(arguments_)
      break
    case AcceptanceCommand.Help:
      printUsage()
      break
    default:
      throw new Error(`unknown release-acceptance command: ${command}`)
  }
}

function prepareClean(arguments_) {
  const dataDirectory = resolveNewDataDirectory(requiredArgument(arguments_, 0))
  assert(arguments_.length === 1, 'prepare-clean accepts exactly one data directory')
  mkdirSync(dataDirectory)
  assertEqual(readdirSync(dataDirectory), [], 'new clean-run directory contents')
  console.info(`Prepared empty clean-run directory: ${dataDirectory}`)
}

function launch(arguments_) {
  const dataDirectory = resolveExistingDataDirectory(requiredArgument(arguments_, 0))
  const app = resolvePackagedApp(arguments_[1])
  assert(arguments_.length <= 2, 'launch accepts a data directory and optional Dara.app path')
  assertDirectoryEmptyOrInitialized(dataDirectory)
  assertNoPackagedAppProcess(app.executable)

  const environment = { ...process.env, DARA_DATA_DIR: dataDirectory }
  for (const name of [
    'DARA_LLAMA_SERVER_PATH',
    'DARA_EMBEDDING_MODEL_PATH',
    'DARA_LLAMA_DEVICE',
    'DARA_LLAMA_GPU_LAYERS',
  ]) {
    delete environment[name]
  }

  const child = spawn(app.executable, [], {
    detached: true,
    env: environment,
    stdio: 'ignore',
  })
  child.unref()
  writeJsonReplacing(join(dataDirectory, launchRecordFile), {
    formatVersion: 1,
    launchedAt: new Date().toISOString(),
    pid: child.pid,
    appPath: app.path,
    executable: app.executable,
    dataDirectory,
    developmentOverridesRemoved: true,
  })
  console.info(`Launched packaged Dara (pid ${child.pid}) against ${dataDirectory}`)
  console.info('Quit with Cmd+Q before running an acceptance check.')
}

function checkInterruptedDownload(arguments_) {
  const dataDirectory = resolveExistingDataDirectory(requiredArgument(arguments_, 0))
  const app = resolvePackagedApp(arguments_[1])
  assert(
    arguments_.length <= 2,
    'check-interrupted-download accepts a data directory and optional Dara.app path',
  )
  const model = packagedModel(app)
  const paths = modelPaths(dataDirectory, model.manifest)

  assertStopped(dataDirectory, app)
  assert(!existsSync(paths.complete), 'completed model must not exist at interruption checkpoint')
  assert(existsSync(paths.partial), 'resumable .part model is missing')
  const partialBytes = statSync(paths.partial).size
  assert(partialBytes > 0, 'resumable .part model is empty')
  assert(
    partialBytes < model.manifest.config.modelFileSize,
    'interrupted model already has the complete expected length',
  )
  assert(
    !existsSync(join(dataDirectory, receiptFile)),
    'verification receipt must not exist before the model completes',
  )
  assert(
    !existsSync(join(dataDirectory, sidecarLogFile)),
    'llama-server started before the managed model completed',
  )
  const state = readLiveDatabaseState(dataDirectory)
  assert(
    state.activeCardContents >= 1,
    'create at least one card while the model downloads before checking interruption',
  )
  assert(
    state.searchDocuments >= 1,
    'the interrupted run did not preserve a lexical search projection',
  )
  console.info(
    `Interrupted download checkpoint passed: ${partialBytes}/${model.manifest.config.modelFileSize} bytes retained.`,
  )
}

async function checkClean(arguments_) {
  const dataDirectory = resolveExistingDataDirectory(requiredArgument(arguments_, 0))
  const app = resolvePackagedApp(arguments_[1])
  assert(arguments_.length <= 2, 'check-clean accepts a data directory and optional Dara.app path')
  const evidence = await inspectCleanRun(dataDirectory, app)
  console.info(
    `Clean first-run durable checks passed: ${evidence.searchDocuments} documents indexed, model and receipt verified, sidecar stopped.`,
  )
}

async function checkpointRestart(arguments_) {
  const dataDirectory = resolveExistingDataDirectory(requiredArgument(arguments_, 0))
  const app = resolvePackagedApp(arguments_[1])
  assert(
    arguments_.length <= 2,
    'checkpoint-restart accepts a data directory and optional Dara.app path',
  )
  await inspectCleanRun(dataDirectory, app)
  const model = packagedModel(app)
  const paths = modelPaths(dataDirectory, model.manifest)
  const checkpoint = {
    formatVersion: 1,
    recordedAt: new Date().toISOString(),
    appPath: app.path,
    receipt: await fileEvidence(join(dataDirectory, receiptFile)),
    model: await fileEvidence(paths.complete),
    sidecarLog: await fileEvidence(join(dataDirectory, sidecarLogFile)),
  }
  writeJson(join(dataDirectory, restartCheckpointFile), checkpoint)
  console.info('Recorded receipt/model/sidecar evidence. Relaunch without performing a search, quit, then run check-restart.')
}

async function checkRestart(arguments_) {
  const dataDirectory = resolveExistingDataDirectory(requiredArgument(arguments_, 0))
  const app = resolvePackagedApp(arguments_[1])
  assert(
    arguments_.length <= 2,
    'check-restart accepts a data directory and optional Dara.app path',
  )
  await inspectCleanRun(dataDirectory, app)
  const checkpoint = readJson(join(dataDirectory, restartCheckpointFile))
  assertEqual(checkpoint.appPath, app.path, 'restart checkpoint app path')
  const model = packagedModel(app)
  const paths = modelPaths(dataDirectory, model.manifest)
  assertEqual(
    await fileEvidence(join(dataDirectory, receiptFile)),
    checkpoint.receipt,
    'verification receipt reuse',
  )
  assertEqual(
    await fileEvidence(paths.complete),
    checkpoint.model,
    'verified model reuse',
  )
  assertEqual(
    await fileEvidence(join(dataDirectory, sidecarLogFile)),
    checkpoint.sidecarLog,
    'sidecar remained stopped during receipt-only restart',
  )
  console.info('Clean restart passed: the receipt and model were reused without starting llama-server.')
}

function prepareUpgrade(arguments_) {
  const dataDirectory = resolveNewDataDirectory(requiredArgument(arguments_, 0))
  assert(
    arguments_.length === 1,
    'prepare-upgrade accepts exactly one data directory',
  )
  const result = spawnSync(
    'cargo',
    [
      'test',
      '--locked',
      '--lib',
      'database::release_acceptance::write_previous_release_fixture',
      '--',
      '--ignored',
      '--exact',
      '--nocapture',
    ],
    {
      cwd: resolve(appRoot, 'src-tauri'),
      encoding: 'utf8',
      env: {
        ...process.env,
        DARA_RELEASE_ACCEPTANCE_FIXTURE_DIR: dataDirectory,
      },
    },
  )
  requireSuccess(result, 'previous-release fixture builder')
  assert(existsSync(join(dataDirectory, 'dara.sqlite3')), 'fixture main database is missing')
  assert(existsSync(join(dataDirectory, 'media.sqlite3')), 'fixture media database is missing')
  assert(
    existsSync(join(dataDirectory, fixtureDescriptionFile)),
    'fixture description is missing',
  )
  console.info(`Prepared previous-head upgrade fixture: ${dataDirectory}`)
}

function checkUpgrade(arguments_) {
  const dataDirectory = resolveExistingDataDirectory(requiredArgument(arguments_, 0))
  const app = resolvePackagedApp(arguments_[1])
  assert(
    arguments_.length <= 2,
    'check-upgrade accepts a data directory and optional Dara.app path',
  )
  const result = inspectUpgrade(dataDirectory, app)
  console.info(
    `Upgrade passed: main/media reached V${result.currentHeads.main}/V${result.currentHeads.media}; pre-migration V${result.previousHeads.main}/V${result.previousHeads.media} snapshot remains valid.`,
  )
}

function proveUpgradeRestore(arguments_) {
  const dataDirectory = resolveExistingDataDirectory(requiredArgument(arguments_, 0))
  const restoreDirectory = resolveNewDataDirectory(requiredArgument(arguments_, 1))
  const app = resolvePackagedApp(arguments_[2])
  assert(
    arguments_.length <= 3,
    'prove-upgrade-restore accepts source data, restore target, and optional Dara.app path',
  )
  const upgrade = inspectUpgrade(dataDirectory, app)
  mkdirSync(restoreDirectory)
  const output = run(app.executable, [
    'recovery',
    'restore',
    upgrade.preMigrationSnapshot.manifest,
    restoreDirectory,
  ])
  const report = JSON.parse(output)
  assertEqual(
    report.snapshotCreatedAt,
    upgrade.preMigrationSnapshot.createdAt,
    'restored snapshot timestamp',
  )
  assertFixtureState(
    restoreDirectory,
    upgrade.description,
    upgrade.previousHeads,
    false,
  )
  console.info(
    `Pre-migration snapshot restored successfully into ${restoreDirectory}; the source fixture was not modified.`,
  )
}

async function inspectCleanRun(dataDirectory, app) {
  assertStopped(dataDirectory, app)
  const model = packagedModel(app)
  const paths = modelPaths(dataDirectory, model.manifest)
  assert(existsSync(paths.complete), 'managed model is missing')
  assert(!existsSync(paths.partial), 'completed run left a partial model behind')
  assertEqual(
    statSync(paths.complete).size,
    model.manifest.config.modelFileSize,
    'managed model byte length',
  )
  assertEqual(
    await sha256File(paths.complete),
    model.manifest.modelFileSha256,
    'managed model SHA-256',
  )

  const receiptPath = join(dataDirectory, receiptFile)
  const receipt = readJson(receiptPath)
  assertEqual(receipt.receiptVersion, 1, 'verification receipt version')
  assertEqual(
    receipt.manifestSha256,
    await sha256File(join(app.path, modelManifestRelativePath)),
    'receipt embedding manifest',
  )
  assertEqual(
    receipt.goldenFixturesSha256,
    await sha256File(join(app.path, goldenFixturesRelativePath)),
    'receipt golden fixtures',
  )
  assertEqual(
    realpathSync(receipt.model.canonicalPath),
    realpathSync(paths.complete),
    'receipt model path',
  )
  assertEqual(
    receipt.model.byteLength,
    model.manifest.config.modelFileSize,
    'receipt model length',
  )
  assertEqual(
    realpathSync(receipt.sidecar.canonicalPath),
    app.sidecar,
    'receipt bundled sidecar path',
  )
  assertEqual(receipt.runtime.device, null, 'release Metal device override')
  assertEqual(receipt.runtime.gpuLayers, 'all', 'release GPU layer policy')

  const state = readLiveDatabaseState(dataDirectory)
  const currentHeads = expectedMigrationHeads()
  assertEqual(state.mainHead, currentHeads.main, 'clean-run main migration head')
  assertEqual(state.mediaHead, currentHeads.media, 'clean-run media migration head')
  assert(
    state.activeCardContents >= 2,
    'create at least two cards during the clean first-run workflow',
  )
  assert(
    state.reviewEvents >= 1,
    'grade at least one card during the clean first-run workflow',
  )
  assert(state.searchDocuments >= 2, 'clean run is missing search projections')
  assertEqual(
    state.indexedDocuments,
    state.searchDocuments,
    'semantic embedding completeness',
  )
  assertEqual(
    state.activeEmbeddingIndex,
    model.manifest.id,
    'active semantic index',
  )
  assert(
    existsSync(join(dataDirectory, sidecarLogFile)),
    'semantic verification/indexing did not produce a sidecar log',
  )
  const snapshots = recoveryList(app, dataDirectory)
  assert(snapshots.length >= 1, 'clean run did not finalize a launch snapshot')

  return state
}

function inspectUpgrade(dataDirectory, app) {
  assertStopped(dataDirectory, app)
  const description = readJson(join(dataDirectory, fixtureDescriptionFile))
  assertEqual(description.formatVersion, 1, 'fixture description version')
  const currentHeads = expectedMigrationHeads()
  const previousHeads = description.migrationHeads
  assertFixtureState(dataDirectory, description, currentHeads, true)

  const snapshots = recoveryList(app, dataDirectory)
  const preMigrationSnapshot = snapshots.find(
    (snapshot) =>
      snapshot.migrationHeads.main === previousHeads.main &&
      snapshot.migrationHeads.media === previousHeads.media,
  )
  assert(preMigrationSnapshot, 'valid previous-head pre-migration snapshot is missing')
  const launchSnapshot = snapshots.find(
    (snapshot) =>
      snapshot.migrationHeads.main === currentHeads.main &&
      snapshot.migrationHeads.media === currentHeads.media,
  )
  assert(launchSnapshot, 'valid post-migration launch snapshot is missing')
  assert(
    preMigrationSnapshot.createdAt <= launchSnapshot.createdAt,
    'pre-migration snapshot timestamp follows the post-migration launch snapshot',
  )
  const verification = JSON.parse(
    run(app.executable, [
      'recovery',
      'verify',
      preMigrationSnapshot.manifest,
    ]),
  )
  assertEqual(verification.valid, true, 'pre-migration snapshot validity')
  assertEqual(
    verification.snapshot.migrationHeads,
    previousHeads,
    'verified pre-migration snapshot heads',
  )
  return {
    currentHeads,
    description,
    preMigrationSnapshot,
    previousHeads,
  }
}

function assertFixtureState(
  dataDirectory,
  description,
  expectedHeads,
  upgraded,
) {
  const state = readLiveDatabaseState(dataDirectory)
  assertEqual(state.mainHead, expectedHeads.main, 'fixture main migration head')
  assertEqual(state.mediaHead, expectedHeads.media, 'fixture media migration head')
  for (const field of [
    'activeCardContents',
    'deletedCardContents',
    'reviewCards',
    'suspendedReviewCards',
    'reviewEvents',
    'revokedReviewEvents',
    'searchDocuments',
    'images',
    'mediaBlobs',
    'occlusionMasks',
  ]) {
    assertEqual(state[field], description.expected[field], `fixture ${field}`)
  }
  assertEqual(state.appearance, description.expected.appearance, 'fixture appearance')
  assertEqual(state.zoomPercent, description.expected.zoomPercent, 'fixture zoom')
  assertEqual(
    state.imageSha256,
    description.expected.imageSha256,
    'fixture media relationship',
  )

  const main = join(dataDirectory, 'dara.sqlite3')
  const content = sqliteRows(
    main,
    `SELECT id, type, front_md AS frontMd
     FROM card_content
     WHERE id IN (
       '${description.ids.basicContent}',
       '${description.ids.clozeContent}',
       '${description.ids.occlusionContent}'
     )
     ORDER BY id`,
  )
  assertEqual(
    content.map((row) => row.type),
    [CardContentType.Basic, CardContentType.Cloze, CardContentType.Occlusion],
    'fixture card types',
  )
  assert(
    content[0].frontMd.includes('mitochondria'),
    'fixture Basic authored text changed',
  )
  assert(
    content[1].frontMd.includes('{{c1::release}}'),
    'fixture Cloze authored text changed',
  )

  const bindings = sqliteRows(
    main,
    'SELECT command, accelerator FROM keyboard_binding ORDER BY command',
  )
  const expectedBindings = upgraded
    ? [
        {
          command: KeyboardCommand.Home,
          accelerator: description.expected.homeAccelerator,
        },
        {
          command: KeyboardCommand.QuickAdd,
          accelerator: description.expected.quickAddAccelerator,
        },
      ]
    : [
        {
          command: KeyboardCommand.QuickAdd,
          accelerator: description.expected.quickAddAccelerator,
        },
        {
          command: KeyboardCommand.Review,
          accelerator: description.expected.homeAccelerator,
        },
      ]
  expectedBindings.sort((left, right) =>
    left.command.localeCompare(right.command),
  )
  assertEqual(bindings, expectedBindings, 'fixture keyboard bindings')
}

function readLiveDatabaseState(dataDirectory) {
  const main = join(dataDirectory, 'dara.sqlite3')
  const media = join(dataDirectory, 'media.sqlite3')
  assert(existsSync(main), 'main database is missing')
  assert(existsSync(media), 'media database is missing')
  const mainState = sqliteOne(
    main,
    `SELECT
       coalesce((SELECT max(version) FROM refinery_schema_history), 0) AS mainHead,
       (SELECT count(*) FROM card_content WHERE deleted_at IS NULL) AS activeCardContents,
       (SELECT count(*) FROM card_content WHERE deleted_at IS NOT NULL) AS deletedCardContents,
       (SELECT count(*) FROM review_card) AS reviewCards,
       (SELECT count(*) FROM review_card WHERE status = '${ReviewCardStatus.Suspended}') AS suspendedReviewCards,
       (SELECT count(*) FROM review_event) AS reviewEvents,
       (SELECT count(*) FROM review_event WHERE event_type = '${ReviewEventType.Revoke}') AS revokedReviewEvents,
       (SELECT count(*) FROM search_document) AS searchDocuments,
       (SELECT count(*) FROM text_embedding) AS indexedDocuments,
       (SELECT active_text_embedding_index_id FROM app_settings WHERE singleton_id = 1) AS activeEmbeddingIndex,
       (SELECT count(*) FROM image) AS images,
       (SELECT count(*) FROM card_occlusion_mask) AS occlusionMasks,
       (SELECT appearance FROM user_preferences WHERE singleton_id = 1) AS appearance,
       (SELECT zoom_percent FROM user_preferences WHERE singleton_id = 1) AS zoomPercent,
       (SELECT lower(hex(sha256)) FROM image WHERE deleted_at IS NULL LIMIT 1) AS imageSha256`,
  )
  const mediaState = sqliteOne(
    media,
    `SELECT
       coalesce((SELECT max(version) FROM refinery_schema_history), 0) AS mediaHead,
       (SELECT count(*) FROM media_blob) AS mediaBlobs`,
  )
  return { ...mainState, ...mediaState }
}

function recoveryList(app, dataDirectory) {
  const output = run(app.executable, ['recovery', 'list', dataDirectory])
  const snapshots = JSON.parse(output)
  assert(Array.isArray(snapshots), 'recovery list did not return an array')
  return snapshots
}

function assertStopped(dataDirectory, app) {
  assertNoPackagedAppProcess(app.executable)
  assert(
    !existsSync(join(dataDirectory, sidecarPidFile)),
    'llama-server pidfile remains; quit Dara cleanly before checking',
  )
  const receiptPath = join(dataDirectory, receiptFile)
  if (existsSync(receiptPath)) {
    const receipt = readJson(receiptPath)
    const processes = processLines()
    const liveSidecar = processes.find(
      (line) =>
        line.includes(receipt.sidecar.canonicalPath) &&
        line.includes(receipt.model.canonicalPath),
    )
    assert(!liveSidecar, `Dara llama-server is still running: ${liveSidecar}`)
  }
  recoveryList(app, dataDirectory)
}

function assertNoPackagedAppProcess(executable) {
  const live = processLines().find((line) => line.includes(executable))
  assert(!live, `packaged Dara is still running: ${live}`)
}

function processLines() {
  return run('ps', ['-axo', 'pid=,command='])
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
}

function packagedModel(app) {
  const manifestPath = join(app.path, modelManifestRelativePath)
  const manifest = readJson(manifestPath)
  assertEqual(manifest.manifestVersion, 1, 'embedding manifest version')
  return { manifest, manifestPath }
}

function modelPaths(dataDirectory, manifest) {
  const complete = join(dataDirectory, 'models', manifest.config.modelFile)
  return {
    complete,
    partial: `${complete}.part`,
  }
}

function expectedMigrationHeads() {
  return {
    main: latestMigrationVersion(
      resolve(appRoot, 'src-tauri/src/database/migrations/main'),
    ),
    media: latestMigrationVersion(
      resolve(appRoot, 'src-tauri/src/database/migrations/media'),
    ),
  }
}

function latestMigrationVersion(directory) {
  const versions = readdirSync(directory)
    .map((name) => /^V(\d+)__/.exec(name)?.[1])
    .filter(Boolean)
    .map(Number)
  assert(versions.length > 0, `no migrations found in ${directory}`)
  return Math.max(...versions)
}

function resolvePackagedApp(value) {
  const path = realpathSync(resolve(value ?? defaultAppPath))
  const executable = realpathSync(join(path, appExecutableRelativePath))
  const sidecar = realpathSync(join(path, sidecarRelativePath))
  assert(statSync(path).isDirectory(), 'Dara.app is not a directory')
  assert(statSync(executable).isFile(), 'packaged Dara executable is missing')
  assert(statSync(sidecar).isFile(), 'packaged llama-server is missing')
  readJson(join(path, releaseManifestRelativePath))
  return { executable, path, sidecar }
}

function resolveNewDataDirectory(value) {
  const target = resolveDataDirectory(value)
  assert(!existsSync(target), `data directory already exists: ${target}`)
  assertEqual(
    realpathSync(dirname(target)),
    realpathSync(dataRoot),
    'acceptance data-directory parent',
  )
  return target
}

function resolveExistingDataDirectory(value) {
  const target = resolveDataDirectory(value)
  const canonical = realpathSync(target)
  assertEqual(
    realpathSync(dirname(canonical)),
    realpathSync(dataRoot),
    'acceptance data-directory parent',
  )
  assert(statSync(canonical).isDirectory(), 'acceptance data path is not a directory')
  return canonical
}

function resolveDataDirectory(value) {
  const target = value.includes(sep) ? resolve(value) : resolve(dataRoot, value)
  assert(target !== dataRoot, 'acceptance commands may not target app/.data itself')
  assertEqual(dirname(target), dataRoot, 'acceptance data directories must be direct children of app/.data')
  const name = basename(target)
  assert(
    /^[a-z0-9][a-z0-9-]*$/.test(name),
    'acceptance data-directory names use lowercase letters, numbers, and hyphens',
  )
  return target
}

function assertDirectoryEmptyOrInitialized(directory) {
  const entries = readdirSync(directory)
  assert(
    entries.length === 0 || entries.includes('dara.sqlite3'),
    'acceptance directory is neither empty nor an initialized Dara directory',
  )
}

function sqliteRows(database, sql) {
  const output = run('sqlite3', ['-readonly', '-json', database, sql])
  return output ? JSON.parse(output) : []
}

function sqliteOne(database, sql) {
  const rows = sqliteRows(database, sql)
  assertEqual(rows.length, 1, `SQLite result row count for ${basename(database)}`)
  return rows[0]
}

function run(command, arguments_, options = {}) {
  const result = spawnSync(command, arguments_, {
    encoding: 'utf8',
    ...options,
  })
  requireSuccess(result, `${command} ${arguments_.join(' ')}`)
  return `${result.stdout ?? ''}${result.stderr ?? ''}`.trim()
}

function requireSuccess(result, label) {
  if (result.error) {
    throw result.error
  }
  if (result.status !== 0) {
    process.stderr.write(result.stdout ?? '')
    process.stderr.write(result.stderr ?? '')
    throw new Error(`${label} failed with status ${result.status}`)
  }
}

async function fileEvidence(path) {
  const metadata = statSync(path, { bigint: true })
  return {
    sha256: await sha256File(path),
    byteLength: metadata.size.toString(),
    modifiedAtNanoseconds: metadata.mtimeNs.toString(),
  }
}

async function sha256File(path) {
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(path)) {
    hash.update(chunk)
  }
  return hash.digest('hex')
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'))
}

function writeJson(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, {
    encoding: 'utf8',
    flag: 'wx',
  })
}

function writeJsonReplacing(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, 'utf8')
}

function requiredArgument(arguments_, index) {
  const value = arguments_[index]
  assert(value, `missing argument ${index + 1}`)
  return value
}

function assertEqual(actual, expected, label) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
      `Unexpected ${label}: ${JSON.stringify(actual)}; expected ${JSON.stringify(expected)}`,
    )
  }
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message)
  }
}

function printUsage() {
  console.info(`Dara packaged release acceptance

Usage:
  pnpm release:acceptance ${AcceptanceCommand.PrepareClean} <data-name>
  pnpm release:acceptance ${AcceptanceCommand.Launch} <data-name> [Dara.app]
  pnpm release:acceptance ${AcceptanceCommand.CheckInterruptedDownload} <data-name> [Dara.app]
  pnpm release:acceptance ${AcceptanceCommand.CheckClean} <data-name> [Dara.app]
  pnpm release:acceptance ${AcceptanceCommand.CheckpointRestart} <data-name> [Dara.app]
  pnpm release:acceptance ${AcceptanceCommand.CheckRestart} <data-name> [Dara.app]
  pnpm release:acceptance ${AcceptanceCommand.PrepareUpgrade} <data-name>
  pnpm release:acceptance ${AcceptanceCommand.CheckUpgrade} <data-name> [Dara.app]
  pnpm release:acceptance ${AcceptanceCommand.ProveUpgradeRestore} <data-name> <restore-name> [Dara.app]

All data names resolve to direct, non-existing or existing task directories beneath:
  ${dataRoot}

Default packaged app:
  ${defaultAppPath}

Full operator sequence:
  ${resolve(appRoot, 'tests/native/release-acceptance.md')}`)
}
