export type OutcomeClass = 'pass' | 'fail' | 'skip' | 'invalid';

const OUTCOME_MAP: Record<string, OutcomeClass> = {
  pass: 'pass',
  fail: 'fail',
  skipped: 'skip',
  skip: 'skip',
  invalid: 'invalid',
};

export function outcomeClass(outcome: string): OutcomeClass {
  return OUTCOME_MAP[outcome.toLowerCase()] ?? 'invalid';
}
