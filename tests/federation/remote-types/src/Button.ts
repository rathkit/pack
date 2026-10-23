export interface ButtonProps {
  label: string;
  size?: number;
}

export default function makeButton(props: ButtonProps): string {
  return `${props.label}:${props.size ?? 1}`;
}

export const version: number = 3;
