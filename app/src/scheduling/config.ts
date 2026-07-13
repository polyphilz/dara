import { checkParameters, clipParameters } from 'ts-fsrs'
import type {
  SchedulerConfigJsonV1,
  SchedulerConfigV1,
  SchedulerStep,
} from './types.ts'
import { SchedulingError } from './types.ts'

export const TS_FSRS_LIBRARY_VERSION = '5.4.1' as const
export const TS_FSRS_RUNTIME_VERSION = 'v5.4.1 using FSRS-6.0' as const

const DEFAULT_PARAMETERS = Object.freeze([
  0.212,
  1.2931,
  2.3065,
  8.2956,
  6.4133,
  0.8334,
  3.0194,
  0.001,
  1.8722,
  0.1666,
  0.796,
  1.4835,
  0.0614,
  0.2629,
  1.6483,
  0.6014,
  1.8729,
  0.5425,
  0.0912,
  0.0658,
  0.1542,
] as const)

const DEFAULT_LEARNING_STEPS = Object.freeze(['10m'] as const)
const DEFAULT_RELEARNING_STEPS = Object.freeze(['10m'] as const)

export const DEFAULT_SCHEDULER_CONFIG: SchedulerConfigV1 = Object.freeze({
  algorithm: 'FSRS',
  algorithmVersion: 6,
  schedulerLibrary: 'ts-fsrs',
  libraryVersion: TS_FSRS_LIBRARY_VERSION,
  configSchemaVersion: 1,
  config: Object.freeze({
    parameters: DEFAULT_PARAMETERS,
    desiredRetention: 0.9,
    maximumInterval: 36_500,
    learningSteps: DEFAULT_LEARNING_STEPS,
    relearningSteps: DEFAULT_RELEARNING_STEPS,
    fuzzEnabled: true,
    fuzzStrategyVersion: 1,
  }),
})

export function parseSchedulerConfig(value: unknown): SchedulerConfigV1 {
  const record = requireRecord(value, 'scheduler config')

  requireEqual(record.algorithm, 'FSRS', 'algorithm')
  requireEqual(record.algorithmVersion, 6, 'algorithmVersion')
  requireEqual(record.schedulerLibrary, 'ts-fsrs', 'schedulerLibrary')
  requireEqual(
    record.libraryVersion,
    TS_FSRS_LIBRARY_VERSION,
    'libraryVersion',
  )
  requireEqual(record.configSchemaVersion, 1, 'configSchemaVersion')

  const config = parseConfigJson(record.config)
  return {
    algorithm: 'FSRS',
    algorithmVersion: 6,
    schedulerLibrary: 'ts-fsrs',
    libraryVersion: TS_FSRS_LIBRARY_VERSION,
    configSchemaVersion: 1,
    config,
  }
}

function parseConfigJson(value: unknown): SchedulerConfigJsonV1 {
  const record = requireRecord(value, 'scheduler config JSON')
  const parameters = requireNumberArray(record.parameters, 'parameters')

  try {
    checkParameters(parameters)
  } catch (error) {
    throw new SchedulingError(errorMessage('invalid FSRS parameters', error))
  }
  if (parameters.length !== 21) {
    throw new SchedulingError('FSRS-6 requires exactly 21 parameters')
  }

  const desiredRetention = requireFiniteNumber(
    record.desiredRetention,
    'desiredRetention',
  )
  if (desiredRetention <= 0 || desiredRetention > 1) {
    throw new SchedulingError('desiredRetention must be in the range (0, 1]')
  }

  const maximumInterval = requireInteger(
    record.maximumInterval,
    'maximumInterval',
  )
  if (maximumInterval < 1) {
    throw new SchedulingError('maximumInterval must be positive')
  }

  const learningSteps = requireSteps(record.learningSteps, 'learningSteps')
  const relearningSteps = requireSteps(
    record.relearningSteps,
    'relearningSteps',
  )
  if (learningSteps.length !== 1 || learningSteps[0] !== '10m') {
    throw new SchedulingError('config schema v1 requires learningSteps ["10m"]')
  }
  if (relearningSteps.length !== 1 || relearningSteps[0] !== '10m') {
    throw new SchedulingError(
      'config schema v1 requires relearningSteps ["10m"]',
    )
  }

  if (typeof record.fuzzEnabled !== 'boolean') {
    throw new SchedulingError('fuzzEnabled must be a boolean')
  }
  requireEqual(record.fuzzStrategyVersion, 1, 'fuzzStrategyVersion')

  const clipped = clipParameters(parameters, relearningSteps.length, true)
  if (clipped.some((parameter, index) => parameter !== parameters[index])) {
    throw new SchedulingError('FSRS parameters fall outside supported bounds')
  }

  return {
    parameters,
    desiredRetention,
    maximumInterval,
    learningSteps,
    relearningSteps,
    fuzzEnabled: record.fuzzEnabled,
    fuzzStrategyVersion: 1,
  }
}

function requireRecord(value: unknown, name: string): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    throw new SchedulingError(`${name} must be an object`)
  }
  return value as Record<string, unknown>
}

function requireEqual<T>(value: unknown, expected: T, name: string): asserts value is T {
  if (value !== expected) {
    throw new SchedulingError(`${name} must be ${JSON.stringify(expected)}`)
  }
}

function requireFiniteNumber(value: unknown, name: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    throw new SchedulingError(`${name} must be a finite number`)
  }
  return value
}

function requireInteger(value: unknown, name: string): number {
  const number = requireFiniteNumber(value, name)
  if (!Number.isSafeInteger(number)) {
    throw new SchedulingError(`${name} must be a safe integer`)
  }
  return number
}

function requireNumberArray(value: unknown, name: string): number[] {
  if (!Array.isArray(value)) {
    throw new SchedulingError(`${name} must be an array`)
  }
  return value.map((item, index) =>
    requireFiniteNumber(item, `${name}[${index}]`),
  )
}

function requireSteps(value: unknown, name: string): SchedulerStep[] {
  if (!Array.isArray(value)) {
    throw new SchedulingError(`${name} must be an array`)
  }
  return value.map((item, index) => {
    if (typeof item !== 'string' || !/^[1-9]\d*[mhd]$/.test(item)) {
      throw new SchedulingError(`${name}[${index}] is not a valid step`)
    }
    return item as SchedulerStep
  })
}

function errorMessage(prefix: string, error: unknown): string {
  return error instanceof Error ? `${prefix}: ${error.message}` : prefix
}
