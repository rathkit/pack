export enum Color {
  Red,
  Green = 5,
  Blue,
  Neg = -3,
  Mask = 1 << 4,
  Mixed = Mask | Green,
  'dash-ed' = 9,
}

export enum Label {
  Short = 'S',
  Long = `L${1 + 1}`,
  Both = Short + Long,
}

export const enum Level {
  Low = 1,
  High = Low * 10,
}

export enum Computed {
  Known = 3,
  Unknown = Math.floor(Math.random() * 0) + 7,
  After,
}

export enum Unused {
  Never = 1,
}
