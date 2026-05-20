export type OutcomeClass = 'pass' | 'fail' | 'cancel' | 'skip' | 'invalid';

const OUTCOME_MAP: Record<string, OutcomeClass> = {
  pass: 'pass',
  fail: 'fail',
  cancelled: 'cancel',
  skipped: 'skip',
  skip: 'skip',
  invalid: 'invalid',
};

export function outcomeClass(outcome: string): OutcomeClass {
  return OUTCOME_MAP[outcome.toLowerCase()] ?? 'invalid';
}
